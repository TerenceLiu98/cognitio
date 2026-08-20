use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::mpsc,
};

use crate::{
    agents,
    commands::AppState,
    jobs::{self, BlockScope, JobRecord, JobState},
    logging, mineru,
    models::{AfterProcessing, AppSnapshot, DeploymentSummary},
    tray,
};

const AGENT_TOTAL_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const AGENT_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ParseManifest {
    schema_version: u32,
    source_sha256: String,
    mode: String,
    profile_version: String,
    markdown_sha256: String,
    markdown_size: u64,
    completed_at: String,
}

enum AgentStreamEvent {
    Stdout(String),
    Stderr(String),
    Error(String),
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let lookup_app = app.clone();
            let record = tokio::task::spawn_blocking(move || next_job(&lookup_app))
                .await
                .unwrap_or(None);
            if let Some(record) = record {
                process(&app, record).await;
            } else {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    });
}

pub fn resume_deployment_monitors(app: AppHandle, workspace: PathBuf) {
    let records = jobs::load_all(&workspace);
    for record in records.into_iter().filter(|record| {
        record.state == JobState::Succeeded
            && matches!(
                record.deployment.status.as_str(),
                "pending" | "running" | "unknown"
            )
    }) {
        monitor_deployment(
            app.clone(),
            workspace.clone(),
            record.id,
            record.published_commit,
        );
    }
}

fn next_job(app: &AppHandle) -> Option<JobRecord> {
    let state = app.state::<AppState>();
    let snapshot = state.snapshot.lock().ok()?;
    if !snapshot.configured || !snapshot.watching {
        return None;
    }
    let records = jobs::load_all(Path::new(&snapshot.settings.workspace_root));
    if records.iter().any(|job| {
        job.state == JobState::Blocked && job.block_scope == Some(BlockScope::Repository)
    }) {
        return None;
    }
    records.into_iter().rev().find(|job| {
        matches!(
            job.state,
            JobState::Queued | JobState::Verifying | JobState::Archiving
        )
    })
}

async fn process(app: &AppHandle, record: JobRecord) {
    let state = app.state::<AppState>();
    let _repository_guard = state.repository_operation.lock().await;
    if let Err(error) = run_pipeline(app, &record).await {
        if recover_pipeline_error(app, &record).await.unwrap_or(false) {
            return;
        }
        let failure_app = app.clone();
        let failure_record = record.clone();
        let _ =
            tokio::task::spawn_blocking(move || fail_job(&failure_app, &failure_record, &error))
                .await;
    }
}

async fn recover_pipeline_error(app: &AppHandle, record: &JobRecord) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let settings = state
        .snapshot
        .lock()
        .map_err(|_| "application state lock is poisoned".to_string())?
        .settings
        .clone();
    let workspace = PathBuf::from(settings.workspace_root);
    let load_workspace = workspace.clone();
    let load_id = record.id.clone();
    let current = tokio::task::spawn_blocking(move || jobs::load(&load_workspace, &load_id))
        .await
        .map_err(|error| format!("join job reload: {error}"))??;
    if current.state == JobState::Cancelled {
        return Ok(true);
    }
    if current.base_commit.is_none() || current.state != JobState::Running {
        return Ok(false);
    }
    let wiki = workspace.join("wiki");
    if !git_output(&wiki, &["status", "--porcelain"])
        .await?
        .is_empty()
    {
        block_job(
            app,
            &workspace,
            &record.id,
            "Agent stopped with uncommitted Wiki changes",
            BlockScope::Repository,
        )
        .await?;
        return Ok(true);
    }
    let lookup_wiki = wiki.clone();
    let lookup_record = current.clone();
    let commit =
        tokio::task::spawn_blocking(move || jobs::find_job_commit(&lookup_wiki, &lookup_record))
            .await
            .map_err(|error| format!("join interrupted commit lookup: {error}"))??;
    if commit.is_none() {
        return Ok(false);
    }
    transition(
        app,
        &workspace,
        &record.id,
        JobState::Verifying,
        "Resuming Git publication",
        85,
    )
    .await?;
    let execution = current
        .execution
        .as_ref()
        .ok_or_else(|| "job has no captured execution settings".to_string())?;
    verify_publication(
        app,
        &current,
        &workspace,
        current.base_commit.as_deref(),
        &execution.after_processing,
    )
    .await?;
    Ok(true)
}

async fn run_pipeline(app: &AppHandle, record: &JobRecord) -> Result<(), String> {
    let state = app.state::<AppState>();
    let app_settings = state
        .snapshot
        .lock()
        .map_err(|_| "application state lock is poisoned".to_string())?
        .settings
        .clone();
    let workspace = PathBuf::from(&app_settings.workspace_root);
    let wiki = workspace.join("wiki");
    let execution = record
        .execution
        .clone()
        .ok_or_else(|| "job has no captured execution settings; retry it first".to_string())?;
    if record.state == JobState::Archiving {
        return archive_and_succeed(app, record, &workspace, &execution.after_processing).await;
    }
    if record.state == JobState::Verifying {
        return verify_publication(
            app,
            record,
            &workspace,
            record.base_commit.as_deref(),
            &execution.after_processing,
        )
        .await;
    }
    transition(
        app,
        &workspace,
        &record.id,
        JobState::Preflight,
        "Checking Wiki and agent",
        15,
    )
    .await?;

    if !git_output(&wiki, &["status", "--porcelain"])
        .await?
        .is_empty()
    {
        return block_job(
            app,
            &workspace,
            &record.id,
            "Wiki has uncommitted changes",
            BlockScope::Repository,
        )
        .await;
    }
    git_output(&wiki, &["pull", "--ff-only"]).await?;
    let before = git_output(&wiki, &["rev-parse", "HEAD"]).await?;
    let mut detected = Vec::new();
    for kind in [
        agents::AgentKind::Codex,
        agents::AgentKind::Claude,
        agents::AgentKind::Opencode,
    ] {
        if let Some(version) = command_version(kind).await {
            detected.push((kind, version));
        }
    }
    let selected = agents::select(&execution.agent_provider, |kind| {
        detected.iter().any(|(candidate, _)| *candidate == kind)
    })?;
    let version = detected
        .iter()
        .find(|(kind, _)| *kind == selected)
        .map(|(_, version)| version.clone())
        .ok_or_else(|| "selected agent version is unavailable".to_string())?;
    let execution_workspace = workspace.clone();
    let execution_id = record.id.clone();
    let execution_agent = selected.as_str().to_owned();
    let execution_before = before.trim().to_owned();
    tokio::task::spawn_blocking(move || {
        jobs::set_execution(
            &execution_workspace,
            &execution_id,
            &execution_agent,
            &execution_before,
        )
    })
    .await
    .map_err(|error| format!("join execution metadata save: {error}"))??;
    let parse_output = workspace.join("processing").join(&record.id).join("parsed");
    transition(
        app,
        &workspace,
        &record.id,
        JobState::Running,
        "Parsing PDF with MinerU",
        25,
    )
    .await?;
    let Some(parsed_markdown) =
        prepare_markdown(app, record, &parse_output, execution.mineru_mode.clone()).await?
    else {
        return Ok(());
    };
    update_phase(
        app,
        &workspace,
        &record.id,
        &format!("Running {}", selected.as_str()),
        45,
    )
    .await?;
    let prompt = format!("Use $llmwiki to process the already parsed Markdown at {}. The job id is {}. Do not invoke MinerU, upload a document, or run any parser. Validate the content, commit, and push before returning success. Do not install dependencies or run Quartz locally; GitHub Actions owns the site build.", parsed_markdown.display(), record.id);
    let mut env = minimal_environment(selected);
    env.insert(
        "LLMWIKI_PARSED_MARKDOWN".into(),
        parsed_markdown.to_string_lossy().into_owned(),
    );
    env.insert("LLMWIKI_JOB_ID".into(), record.id.clone());
    let spec = agents::build_command(
        selected,
        &version,
        &wiki,
        &prompt,
        execution.model.as_deref(),
        env,
    )?;
    let mut command = Command::new(&spec.executable);
    command
        .args(&spec.args)
        .current_dir(&wiki)
        .env_clear()
        .envs(&spec.env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|error| format!("start {}: {error}", selected.as_str()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "agent stdout is unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "agent stderr is unavailable".to_string())?;
    let (output_tx, mut output_rx) = mpsc::unbounded_channel();
    let stdout_tx = output_tx.clone();
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if stdout_tx.send(AgentStreamEvent::Stdout(line)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = stdout_tx.send(AgentStreamEvent::Error(format!(
                        "read agent stdout: {error}"
                    )));
                    break;
                }
            }
        }
    });
    let stderr_tx = output_tx.clone();
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if stderr_tx.send(AgentStreamEvent::Stderr(line)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = stderr_tx.send(AgentStreamEvent::Error(format!(
                        "read agent stderr: {error}"
                    )));
                    break;
                }
            }
        }
    });
    drop(output_tx);
    let started = std::time::Instant::now();
    let mut last_activity = started;
    loop {
        tokio::select! {
            output = output_rx.recv() => match output {
                Some(AgentStreamEvent::Stdout(line)) => {
                    last_activity = std::time::Instant::now();
                    let event = agents::parse_event(&line);
                    if let Some(message) = event.message {
                        append_log(app, "info", &message, Some(&record.id)).await;
                    }
                }
                Some(AgentStreamEvent::Stderr(line)) => {
                    last_activity = std::time::Instant::now();
                    append_log(app, "warn", &line, Some(&record.id)).await;
                }
                Some(AgentStreamEvent::Error(error)) => {
                    stop_agent(&mut child, "stop agent after output failure").await?;
                    return Err(error);
                }
                None => break,
            },
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                if is_cancelled(app, &record.id) {
                    stop_agent(&mut child, "cancel agent").await?;
                    return Ok(());
                }
                if let Some(error) = agent_timeout_error(started.elapsed(), last_activity.elapsed()) {
                    stop_agent(&mut child, "stop timed-out agent").await?;
                    return Err(error);
                }
            }
        }
    }
    let status = child
        .wait()
        .await
        .map_err(|error| format!("wait for agent: {error}"))?;
    if !status.success() {
        return Err(format!("{} exited with status {status}", selected.as_str()));
    }
    if is_cancelled(app, &record.id) {
        return Ok(());
    }

    transition(
        app,
        &workspace,
        &record.id,
        JobState::Verifying,
        "Verifying Git publication",
        85,
    )
    .await?;
    verify_publication(
        app,
        record,
        &workspace,
        Some(before.trim()),
        &execution.after_processing,
    )
    .await
}

async fn prepare_markdown(
    app: &AppHandle,
    record: &JobRecord,
    output: &Path,
    mode: crate::models::MineruMode,
) -> Result<Option<PathBuf>, String> {
    if record.force_reparse && output.exists() {
        tokio::fs::remove_dir_all(output)
            .await
            .map_err(|error| format!("clear previous MinerU output: {error}"))?;
    }
    if !record.force_reparse {
        if let Some(existing) = reusable_markdown(output, record, &mode).await {
            append_log(
                app,
                "info",
                "Reusing verified MinerU Markdown",
                Some(&record.id),
            )
            .await;
            return Ok(Some(existing));
        }
    }
    let parse_mode = match mode {
        crate::models::MineruMode::Precision => mineru::ParseMode::Precision,
        crate::models::MineruMode::Flash => mineru::ParseMode::Flash,
    };
    let parse = mineru::parse(&record.input_path, output, parse_mode);
    tokio::pin!(parse);
    let parsed = loop {
        tokio::select! {
            result = &mut parse => break result?,
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                if is_cancelled(app, &record.id) {
                    return Ok(None);
                }
            }
        }
    };
    let hash_path = parsed.clone();
    let markdown_sha256 = tokio::task::spawn_blocking(move || jobs::sha256(&hash_path))
        .await
        .map_err(|error| format!("join Markdown checksum: {error}"))??;
    let markdown_size = tokio::fs::metadata(&parsed)
        .await
        .map_err(|error| format!("read parsed Markdown metadata: {error}"))?
        .len();
    if markdown_size == 0 {
        return Err("MinerU returned empty Markdown".into());
    }
    let manifest = ParseManifest {
        schema_version: 1,
        source_sha256: record.sha256.clone(),
        mode: mineru_mode_name(&mode).into(),
        profile_version: mineru::PROFILE_VERSION.into(),
        markdown_sha256,
        markdown_size,
        completed_at: chrono::Utc::now().to_rfc3339(),
    };
    let temporary = output.join(".manifest.json.tmp");
    tokio::fs::write(
        &temporary,
        serde_json::to_vec_pretty(&manifest)
            .map_err(|error| format!("serialize MinerU manifest: {error}"))?,
    )
    .await
    .map_err(|error| format!("write MinerU manifest: {error}"))?;
    tokio::fs::rename(&temporary, output.join("manifest.json"))
        .await
        .map_err(|error| format!("finish MinerU manifest: {error}"))?;
    let clear_workspace = record
        .input_path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    if let Some(workspace) = clear_workspace {
        let clear_id = record.id.clone();
        if let Ok(Ok(updated)) =
            tokio::task::spawn_blocking(move || jobs::clear_force_reparse(&workspace, &clear_id))
                .await
        {
            publish_record(app, &updated);
        }
    }
    Ok(Some(parsed))
}

async fn reusable_markdown(
    output: &Path,
    record: &JobRecord,
    mode: &crate::models::MineruMode,
) -> Option<PathBuf> {
    let manifest: ParseManifest =
        serde_json::from_slice(&tokio::fs::read(output.join("manifest.json")).await.ok()?).ok()?;
    if manifest.schema_version != 1
        || manifest.source_sha256 != record.sha256
        || manifest.mode != mineru_mode_name(mode)
        || manifest.profile_version != mineru::PROFILE_VERSION
    {
        return None;
    }
    let markdown = output.join("full.md");
    let metadata = tokio::fs::metadata(&markdown).await.ok()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() != manifest.markdown_size {
        return None;
    }
    let hash_path = markdown.clone();
    let checksum = tokio::task::spawn_blocking(move || jobs::sha256(&hash_path))
        .await
        .ok()?
        .ok()?;
    (checksum == manifest.markdown_sha256).then_some(markdown)
}

fn mineru_mode_name(mode: &crate::models::MineruMode) -> &'static str {
    match mode {
        crate::models::MineruMode::Precision => "precision",
        crate::models::MineruMode::Flash => "flash",
    }
}

fn agent_timeout_error(total: Duration, idle: Duration) -> Option<String> {
    if total >= AGENT_TOTAL_TIMEOUT {
        Some("agent timed out after 60 minutes".into())
    } else if idle >= AGENT_IDLE_TIMEOUT {
        Some("agent produced no output for 10 minutes".into())
    } else {
        None
    }
}

async fn stop_agent(child: &mut Child, action: &str) -> Result<(), String> {
    #[cfg(unix)]
    if let Some(pid) = child
        .id()
        .and_then(|pid| i32::try_from(pid).ok())
        .and_then(rustix::process::Pid::from_raw)
    {
        if rustix::process::kill_process_group(pid, rustix::process::Signal::TERM).is_ok() {
            match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
                Ok(result) => {
                    result.map_err(|error| format!("{action}: {error}"))?;
                    let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
                    return Ok(());
                }
                Err(_) => {
                    let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
                    child
                        .wait()
                        .await
                        .map_err(|error| format!("{action}: {error}"))?;
                    return Ok(());
                }
            }
        }
    }
    child
        .kill()
        .await
        .map_err(|error| format!("{action}: {error}"))
}

async fn verify_publication(
    app: &AppHandle,
    record: &JobRecord,
    workspace: &Path,
    base_commit: Option<&str>,
    after_processing: &AfterProcessing,
) -> Result<(), String> {
    let wiki = workspace.join("wiki");
    let Some(base_commit) = base_commit else {
        return block_job(
            app,
            workspace,
            &record.id,
            "Job has no recorded baseline commit",
            BlockScope::Job,
        )
        .await;
    };
    if !git_output(&wiki, &["status", "--porcelain"])
        .await?
        .is_empty()
    {
        return block_job(
            app,
            workspace,
            &record.id,
            "Wiki is not clean after agent completion",
            BlockScope::Repository,
        )
        .await;
    }

    let range = format!("{base_commit}..HEAD");
    let commit_count = git_output(&wiki, &["rev-list", "--count", &range])
        .await?
        .parse::<u32>()
        .map_err(|error| format!("parse task commit count: {error}"))?;
    if commit_count == 0 {
        return block_job(
            app,
            workspace,
            &record.id,
            "Agent did not create a commit",
            BlockScope::Job,
        )
        .await;
    }
    if commit_count != 1 {
        return block_job(
            app,
            workspace,
            &record.id,
            "The job must produce exactly one commit",
            BlockScope::Repository,
        )
        .await;
    }

    let lookup_wiki = wiki.clone();
    let lookup_record = record.clone();
    let commit =
        tokio::task::spawn_blocking(move || jobs::find_job_commit(&lookup_wiki, &lookup_record))
            .await
            .map_err(|error| format!("join task commit lookup: {error}"))??;
    let Some(commit) = commit else {
        return block_job(
            app,
            workspace,
            &record.id,
            "Task commit is missing the Cognitio-Job trailer",
            BlockScope::Repository,
        )
        .await;
    };
    let changed = git_output(
        &wiki,
        &["diff-tree", "--no-commit-id", "--name-only", "-r", &commit],
    )
    .await?;
    if let Some(path) = changed.lines().find(|path| !allowed_wiki_path(path)) {
        return block_job(
            app,
            workspace,
            &record.id,
            &format!("Task commit changed a disallowed path: {path}"),
            BlockScope::Repository,
        )
        .await;
    }

    let publish_workspace = workspace.to_path_buf();
    let publish_id = record.id.clone();
    let publish_commit = commit.clone();
    let published = tokio::task::spawn_blocking(move || {
        jobs::set_published_commit(&publish_workspace, &publish_id, &publish_commit)
    })
    .await
    .map_err(|error| format!("join published commit save: {error}"))??;
    publish_record(app, &published);

    if !git_success(&wiki, &["merge-base", "--is-ancestor", &commit, "@{u}"]).await? {
        if let Err(error) = git_output(&wiki, &["push"]).await {
            return block_job(
                app,
                workspace,
                &record.id,
                &format!("Task commit exists but push failed: {error}"),
                BlockScope::Job,
            )
            .await;
        }
    }
    if !git_success(&wiki, &["merge-base", "--is-ancestor", &commit, "@{u}"]).await? {
        return block_job(
            app,
            workspace,
            &record.id,
            "Remote branch does not contain the task commit",
            BlockScope::Job,
        )
        .await;
    }

    transition(
        app,
        workspace,
        &record.id,
        JobState::Archiving,
        "Archiving source PDF",
        95,
    )
    .await?;
    archive_and_succeed(app, record, workspace, after_processing).await
}

async fn archive_and_succeed(
    app: &AppHandle,
    record: &JobRecord,
    workspace: &Path,
    after_processing: &AfterProcessing,
) -> Result<(), String> {
    let plan_source = record.source_path.clone();
    let plan_workspace = workspace.to_path_buf();
    let plan_job_id = record.id.clone();
    let plan_behavior = after_processing.clone();
    let existing_destination = record.archive_destination.clone();
    let destination = tokio::task::spawn_blocking(move || -> Result<Option<PathBuf>, String> {
        if let Some(destination) = existing_destination {
            Ok(Some(destination))
        } else {
            planned_archive_destination(&plan_source, &plan_workspace, &plan_job_id, &plan_behavior)
        }
    })
    .await
    .map_err(|error| format!("join archive planning: {error}"))??;
    let receipt_workspace = workspace.to_path_buf();
    let receipt_id = record.id.clone();
    let receipt_destination = destination.clone();
    let receipt = tokio::task::spawn_blocking(move || {
        jobs::set_archive_destination(&receipt_workspace, &receipt_id, receipt_destination)
    })
    .await
    .map_err(|error| format!("join archive receipt save: {error}"))??;
    publish_record(app, &receipt);
    let archive_source_path = record.source_path.clone();
    let archive_input = record.input_path.clone();
    let archive_behavior = after_processing.clone();
    let archive_destination = destination.clone();
    tokio::task::spawn_blocking(move || {
        archive_source(
            &archive_source_path,
            archive_destination.as_deref(),
            &archive_behavior,
        )?;
        if archive_input != archive_source_path
            && archive_input.exists()
            && (!matches!(archive_behavior, AfterProcessing::Keep) || archive_source_path.exists())
        {
            fs::remove_file(&archive_input)
                .map_err(|error| format!("remove processing PDF: {error}"))?;
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|error| format!("join source archival: {error}"))??;
    transition(
        app,
        workspace,
        &record.id,
        JobState::Succeeded,
        "Published",
        100,
    )
    .await?;
    let deployment_workspace = workspace.to_path_buf();
    let deployment_id = record.id.clone();
    let pending = DeploymentSummary {
        status: "pending".into(),
        url: None,
        error: None,
        updated_at: Some(chrono::Utc::now().to_rfc3339()),
    };
    if let Ok(Ok(updated)) = tokio::task::spawn_blocking(move || {
        jobs::set_deployment(&deployment_workspace, &deployment_id, pending)
    })
    .await
    {
        publish_record(app, &updated);
    }
    append_log(
        app,
        "info",
        &format!("Published {}", record.filename),
        Some(&record.id),
    )
    .await;
    notify("Added to Cognitio", &record.filename);
    monitor_deployment(
        app.clone(),
        workspace.to_path_buf(),
        record.id.clone(),
        receipt.published_commit.clone(),
    );
    Ok(())
}

fn allowed_wiki_path(path: &str) -> bool {
    path == "references.bib"
        || ["content/papers/", "content/concepts/", "assets/papers/"]
            .iter()
            .any(|prefix| path.starts_with(prefix))
}

fn monitor_deployment(app: AppHandle, workspace: PathBuf, job_id: String, commit: Option<String>) {
    tauri::async_runtime::spawn(async move {
        let Some(commit) = commit else {
            update_deployment(
                &app,
                &workspace,
                &job_id,
                "unknown",
                None,
                Some("Published commit is unavailable".into()),
            )
            .await;
            return;
        };
        tokio::time::sleep(Duration::from_secs(5)).await;
        for _ in 0..60 {
            if let Ok(value) = github_run(&workspace.join("wiki"), &commit).await {
                let status = value.get("status").and_then(serde_json::Value::as_str);
                let conclusion = value.get("conclusion").and_then(serde_json::Value::as_str);
                let url = value
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                if status == Some("completed") {
                    let successful = conclusion == Some("success");
                    update_deployment(
                        &app,
                        &workspace,
                        &job_id,
                        if successful { "succeeded" } else { "failed" },
                        url,
                        (!successful).then(|| {
                            format!(
                                "GitHub Pages concluded with {}",
                                conclusion.unwrap_or("unknown")
                            )
                        }),
                    )
                    .await;
                    if !successful {
                        notify(
                            "Cognitio site deployment failed",
                            "Open the task details for the GitHub Actions run",
                        );
                    }
                    return;
                }
                update_deployment(&app, &workspace, &job_id, "running", url, None).await;
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
        update_deployment(
            &app,
            &workspace,
            &job_id,
            "unknown",
            None,
            Some("GitHub Pages status was not available within 10 minutes".into()),
        )
        .await;
    });
}

async fn github_run(wiki: &Path, commit: &str) -> Result<serde_json::Value, String> {
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        Command::new("gh")
            .args([
                "run",
                "list",
                "--workflow",
                "deploy.yml",
                "--commit",
                commit,
                "--limit",
                "1",
                "--json",
                "status,conclusion,url",
            ])
            .current_dir(wiki)
            .stdin(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| "GitHub Actions status timed out".to_string())?
    .map_err(|error| format!("query GitHub Actions status: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().into());
    }
    let values: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("decode GitHub Actions status: {error}"))?;
    values
        .into_iter()
        .next()
        .ok_or_else(|| "GitHub Pages workflow has not started".into())
}

async fn update_deployment(
    app: &AppHandle,
    workspace: &Path,
    job_id: &str,
    status: &str,
    url: Option<String>,
    error: Option<String>,
) {
    let update_workspace = workspace.to_path_buf();
    let update_job_id = job_id.to_owned();
    let deployment = DeploymentSummary {
        status: status.into(),
        url,
        error,
        updated_at: Some(chrono::Utc::now().to_rfc3339()),
    };
    if let Ok(Ok(record)) = tokio::task::spawn_blocking(move || {
        jobs::set_deployment(&update_workspace, &update_job_id, deployment)
    })
    .await
    {
        publish_record(app, &record);
    }
}

async fn transition(
    app: &AppHandle,
    workspace: &Path,
    id: &str,
    next: JobState,
    phase: &str,
    progress: u8,
) -> Result<(), String> {
    let workspace = workspace.to_path_buf();
    let id = id.to_owned();
    let phase = phase.to_owned();
    let record =
        tokio::task::spawn_blocking(move || jobs::update(&workspace, &id, next, &phase, progress))
            .await
            .map_err(|error| format!("join job transition: {error}"))??;
    publish_record(app, &record);
    Ok(())
}

async fn update_phase(
    app: &AppHandle,
    workspace: &Path,
    id: &str,
    phase: &str,
    progress: u8,
) -> Result<(), String> {
    let workspace = workspace.to_path_buf();
    let id = id.to_owned();
    let phase = phase.to_owned();
    let record =
        tokio::task::spawn_blocking(move || jobs::update_phase(&workspace, &id, &phase, progress))
            .await
            .map_err(|error| format!("join job phase update: {error}"))??;
    publish_record(app, &record);
    Ok(())
}

async fn block_job(
    app: &AppHandle,
    workspace: &Path,
    id: &str,
    message: &str,
    scope: BlockScope,
) -> Result<(), String> {
    let update_workspace = workspace.to_path_buf();
    let update_id = id.to_owned();
    let update_message = message.to_owned();
    let record = tokio::task::spawn_blocking(move || {
        jobs::block(&update_workspace, &update_id, &update_message, scope)
    })
    .await
    .map_err(|error| format!("join blocked job save: {error}"))??;
    publish_record(app, &record);
    append_log(app, "warn", message, Some(id)).await;
    Ok(())
}

fn fail_job(app: &AppHandle, record: &JobRecord, error: &str) {
    let state = app.state::<AppState>();
    let Ok(snapshot) = state.snapshot.lock() else {
        return;
    };
    let workspace = PathBuf::from(&snapshot.settings.workspace_root);
    drop(snapshot);
    if let Ok(failed) = jobs::fail(&workspace, &record.id, error) {
        publish_record(app, &failed);
        let job_dir = workspace.join("processing").join(&record.id);
        let _ = fs::write(job_dir.join("error.txt"), logging::redact(error));
    }
    if let Ok(entry) = logging::append(&state.config_dir, "error", error, Some(&record.id)) {
        if let Ok(mut snapshot) = state.snapshot.lock() {
            snapshot.logs.push(entry);
            trim_snapshot_logs(&mut snapshot);
        }
    }
    notify("Cognitio could not complete this paper", &record.filename);
}

fn publish_record(app: &AppHandle, record: &JobRecord) {
    let state = app.state::<AppState>();
    let Ok(mut snapshot) = state.snapshot.lock() else {
        return;
    };
    let summary = record.into();
    if let Some(existing) = snapshot.jobs.iter_mut().find(|job| job.id == record.id) {
        *existing = summary;
    } else {
        snapshot.jobs.insert(0, summary);
    }
    let current: AppSnapshot = snapshot.clone();
    drop(snapshot);
    let _ = app.emit("app-snapshot", current.clone());
    tray::refresh(app, &current);
}

async fn append_log(app: &AppHandle, level: &str, message: &str, job_id: Option<&str>) {
    let state = app.state::<AppState>();
    let config_dir = state.config_dir.clone();
    let level = level.to_owned();
    let message = message.to_owned();
    let job_id = job_id.map(str::to_owned);
    if let Ok(Ok(entry)) = tokio::task::spawn_blocking(move || {
        logging::append(&config_dir, &level, &message, job_id.as_deref())
    })
    .await
    {
        if let Ok(mut snapshot) = state.snapshot.lock() {
            snapshot.logs.push(entry);
            trim_snapshot_logs(&mut snapshot);
        }
    }
}

fn trim_snapshot_logs(snapshot: &mut AppSnapshot) {
    const MAX_LOGS: usize = 200;
    if snapshot.logs.len() > MAX_LOGS {
        snapshot.logs.drain(..snapshot.logs.len() - MAX_LOGS);
    }
}

fn is_cancelled(app: &AppHandle, id: &str) -> bool {
    app.state::<AppState>()
        .snapshot
        .lock()
        .is_ok_and(|snapshot| {
            snapshot
                .jobs
                .iter()
                .any(|job| job.id == id && job.state == "cancelled")
        })
}

async fn command_version(kind: agents::AgentKind) -> Option<String> {
    tokio::time::timeout(
        Duration::from_secs(15),
        Command::new(kind.executable()).arg("--version").output(),
    )
    .await
    .ok()?
    .ok()
    .filter(|output| output.status.success())
    .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn minimal_environment(kind: agents::AgentKind) -> BTreeMap<String, String> {
    let mut keys = vec![
        "PATH",
        "HOME",
        "TMPDIR",
        "LANG",
        "LC_ALL",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "SSH_AUTH_SOCK",
    ];
    match kind {
        agents::AgentKind::Codex => keys.push("CODEX_HOME"),
        agents::AgentKind::Claude => keys.push("ANTHROPIC_API_KEY"),
        agents::AgentKind::Opencode => keys.push("OPENCODE_CONFIG"),
    }
    keys.into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.into(), value)))
        .collect()
}

async fn git_output(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = tokio::time::timeout(
        Duration::from_secs(5 * 60),
        Command::new("git")
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| format!("git {} timed out", args.join(" ")))?
    .map_err(|error| format!("run git {}: {error}", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().into())
}

async fn git_success(cwd: &Path, args: &[&str]) -> Result<bool, String> {
    tokio::time::timeout(
        Duration::from_secs(5 * 60),
        Command::new("git")
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .status(),
    )
    .await
    .map_err(|_| format!("git {} timed out", args.join(" ")))?
    .map(|status| status.success())
    .map_err(|error| format!("run git {}: {error}", args.join(" ")))
}

fn notify(title: &str, body: &str) {
    let script =
        "on run argv\ndisplay notification (item 2 of argv) with title (item 1 of argv)\nend run";
    let title = title.to_owned();
    let body = body.to_owned();
    tauri::async_runtime::spawn(async move {
        let _ = Command::new("osascript")
            .args(["-e", script, &title, &body])
            .stdin(Stdio::null())
            .status()
            .await;
    });
}

fn planned_archive_destination(
    source: &Path,
    workspace: &Path,
    job_id: &str,
    behavior: &AfterProcessing,
) -> Result<Option<PathBuf>, String> {
    if matches!(behavior, AfterProcessing::Keep) {
        return Ok(None);
    }
    let filename = source
        .file_name()
        .ok_or_else(|| "source PDF has no filename".to_string())?;
    let destination_root = match behavior {
        AfterProcessing::MoveToDone => workspace.join("done"),
        AfterProcessing::Trash => {
            PathBuf::from(std::env::var("HOME").map_err(|_| "HOME is unavailable".to_string())?)
                .join(".Trash")
        }
        AfterProcessing::Keep => unreachable!(),
    };
    fs::create_dir_all(&destination_root)
        .map_err(|error| format!("create archive directory: {error}"))?;
    let mut destination = destination_root.join(filename);
    if destination.exists() {
        destination = destination_root.join(format!("{job_id}-{}", filename.to_string_lossy()));
    }
    Ok(Some(destination))
}

fn archive_source(
    source: &Path,
    destination: Option<&Path>,
    behavior: &AfterProcessing,
) -> Result<(), String> {
    if matches!(behavior, AfterProcessing::Keep) {
        return Ok(());
    }
    let destination =
        destination.ok_or_else(|| "archive destination is unavailable".to_string())?;
    if !source.exists() {
        return if destination.exists() {
            Ok(())
        } else {
            Err("source PDF and planned archive destination are both missing".into())
        };
    }
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(18) => {
            let mut input = fs::File::open(source)
                .map_err(|copy_error| format!("open source PDF for archival: {copy_error}"))?;
            let temporary = destination.with_extension("cognitio-copying");
            let mut output = fs::File::create(&temporary)
                .map_err(|copy_error| format!("create archive PDF: {copy_error}"))?;
            std::io::copy(&mut input, &mut output)
                .map_err(|copy_error| format!("copy source PDF to archive: {copy_error}"))?;
            output
                .sync_all()
                .map_err(|copy_error| format!("sync archived PDF: {copy_error}"))?;
            fs::rename(&temporary, destination)
                .map_err(|copy_error| format!("finish archived PDF: {copy_error}"))?;
            fs::remove_file(source)
                .map_err(|copy_error| format!("remove archived source PDF: {copy_error}"))
        }
        Err(error) => Err(format!("archive source PDF: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_timeout_distinguishes_total_and_idle_limits() {
        assert!(agent_timeout_error(
            AGENT_TOTAL_TIMEOUT - Duration::from_secs(1),
            AGENT_IDLE_TIMEOUT - Duration::from_secs(1)
        )
        .is_none());
        assert_eq!(
            agent_timeout_error(AGENT_TOTAL_TIMEOUT, Duration::ZERO).as_deref(),
            Some("agent timed out after 60 minutes")
        );
        assert_eq!(
            agent_timeout_error(Duration::ZERO, AGENT_IDLE_TIMEOUT).as_deref(),
            Some("agent produced no output for 10 minutes")
        );
    }

    #[tokio::test]
    async fn reuses_only_verified_task_markdown() {
        let root = std::env::temp_dir().join(format!(
            "cognitio-markdown-reuse-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let workspace = root.join("workspace");
        for directory in ["inbox", "processing"] {
            tokio::fs::create_dir_all(workspace.join(directory))
                .await
                .expect("workspace directory");
        }
        let source = workspace.join("inbox/paper.pdf");
        tokio::fs::write(&source, b"%PDF-1.4\n")
            .await
            .expect("source PDF");
        let record =
            jobs::create(&workspace, &source, &crate::models::AppSettings::default()).expect("job");
        let output = workspace.join("processing").join(&record.id).join("parsed");
        tokio::fs::create_dir_all(&output)
            .await
            .expect("parse output directory");
        let markdown = output.join("full.md");

        tokio::fs::write(&markdown, b"# Paper\n")
            .await
            .expect("parsed markdown");
        assert_eq!(
            reusable_markdown(&output, &record, &crate::models::MineruMode::Precision).await,
            None
        );

        let manifest = ParseManifest {
            schema_version: 1,
            source_sha256: record.sha256.clone(),
            mode: "flash".into(),
            profile_version: mineru::PROFILE_VERSION.into(),
            markdown_sha256: jobs::sha256(&markdown).expect("checksum"),
            markdown_size: 8,
            completed_at: chrono::Utc::now().to_rfc3339(),
        };
        tokio::fs::write(
            output.join("manifest.json"),
            serde_json::to_vec(&manifest).expect("manifest"),
        )
        .await
        .expect("write manifest");
        assert_eq!(
            reusable_markdown(&output, &record, &crate::models::MineruMode::Precision).await,
            None
        );
        assert_eq!(
            reusable_markdown(&output, &record, &crate::models::MineruMode::Flash).await,
            Some(markdown)
        );
        tokio::fs::remove_dir_all(root).await.expect("cleanup");
    }
}
