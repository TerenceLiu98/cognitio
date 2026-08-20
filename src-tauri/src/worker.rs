use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use tauri::{AppHandle, Emitter, Manager};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::mpsc,
};

use crate::{
    agents,
    commands::AppState,
    jobs::{self, JobRecord, JobState},
    logging, mineru,
    models::{AfterProcessing, AppSnapshot},
    tray,
};

const AGENT_TOTAL_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const AGENT_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

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

fn next_job(app: &AppHandle) -> Option<JobRecord> {
    let state = app.state::<AppState>();
    let snapshot = state.snapshot.lock().ok()?;
    if !snapshot.configured || !snapshot.watching {
        return None;
    }
    jobs::load_all(Path::new(&snapshot.settings.workspace_root))
        .into_iter()
        .rev()
        .find(|job| matches!(job.state, JobState::Queued | JobState::Verifying))
}

async fn process(app: &AppHandle, record: JobRecord) {
    let state = app.state::<AppState>();
    let _repository_guard = state.repository_operation.lock().await;
    if let Err(error) = run_pipeline(app, &record).await {
        let failure_app = app.clone();
        let failure_record = record.clone();
        let _ =
            tokio::task::spawn_blocking(move || fail_job(&failure_app, &failure_record, &error))
                .await;
    }
}

async fn run_pipeline(app: &AppHandle, record: &JobRecord) -> Result<(), String> {
    let state = app.state::<AppState>();
    let settings = state
        .snapshot
        .lock()
        .map_err(|_| "application state lock is poisoned".to_string())?
        .settings
        .clone();
    let workspace = PathBuf::from(&settings.workspace_root);
    let wiki = workspace.join("wiki");
    if record.state == JobState::Verifying {
        return verify_publication(
            app,
            record,
            &workspace,
            record.base_commit.as_deref(),
            &settings.after_processing,
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

    let before = git_output(&wiki, &["rev-parse", "HEAD"]).await?;
    if !git_output(&wiki, &["status", "--porcelain"])
        .await?
        .is_empty()
    {
        return block_job(app, &workspace, &record.id, "Wiki has uncommitted changes").await;
    }
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
    let selected = agents::select(&settings.agent_provider, |kind| {
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
        prepare_markdown(app, record, &parse_output, settings.mineru_mode).await?
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
    let prompt = format!("Use $llmwiki to process the already parsed Markdown at {} for the PDF at {}. The job id is {}. Do not invoke MinerU, upload the PDF, or run any parser. Validate the content, commit, and push before returning success. Do not install dependencies or run Quartz locally; GitHub Actions owns the site build.", parsed_markdown.display(), record.input_path.display(), record.id);
    let mut env = minimal_environment();
    env.insert(
        "LLMWIKI_INPUT".into(),
        record.input_path.to_string_lossy().into_owned(),
    );
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
        settings.model.as_deref(),
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
        &settings.after_processing,
    )
    .await
}

async fn prepare_markdown(
    app: &AppHandle,
    record: &JobRecord,
    output: &Path,
    mode: crate::models::MineruMode,
) -> Result<Option<PathBuf>, String> {
    if let Some(existing) = reusable_markdown(output).await {
        append_log(
            app,
            "info",
            "Reusing existing MinerU Markdown",
            Some(&record.id),
        )
        .await;
        return Ok(Some(existing));
    }
    let mode = match mode {
        crate::models::MineruMode::Precision => mineru::ParseMode::Precision,
        crate::models::MineruMode::Flash => mineru::ParseMode::Flash,
    };
    let parse = mineru::parse(&record.input_path, output, mode);
    tokio::pin!(parse);
    loop {
        tokio::select! {
            result = &mut parse => return result.map(Some),
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                if is_cancelled(app, &record.id) {
                    return Ok(None);
                }
            }
        }
    }
}

async fn reusable_markdown(output: &Path) -> Option<PathBuf> {
    let markdown = output.join("full.md");
    tokio::fs::metadata(&markdown)
        .await
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
        .then_some(markdown)
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
    let after = git_output(&wiki, &["rev-parse", "HEAD"]).await?;
    let Some(base_commit) = base_commit else {
        return block_job(
            app,
            workspace,
            &record.id,
            "Job has no recorded baseline commit",
        )
        .await;
    };
    if base_commit == after {
        return block_job(app, workspace, &record.id, "Agent did not create a commit").await;
    }
    if !git_output(&wiki, &["status", "--porcelain"])
        .await?
        .is_empty()
    {
        return block_job(
            app,
            workspace,
            &record.id,
            "Wiki is not clean after agent completion",
        )
        .await;
    }
    if !git_success(&wiki, &["merge-base", "--is-ancestor", &after, "@{u}"]).await? {
        return block_job(
            app,
            workspace,
            &record.id,
            "Remote branch does not contain the new commit",
        )
        .await;
    }
    let archive_source_path = record.source_path.clone();
    let archive_workspace = workspace.to_path_buf();
    let archive_job_id = record.id.clone();
    let archive_behavior = after_processing.clone();
    tokio::task::spawn_blocking(move || {
        archive_source(
            &archive_source_path,
            &archive_workspace,
            &archive_job_id,
            &archive_behavior,
        )
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
    append_log(
        app,
        "info",
        &format!("Published {}", record.filename),
        Some(&record.id),
    )
    .await;
    notify("Added to Cognitio", &record.filename);
    Ok(())
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
) -> Result<(), String> {
    let update_workspace = workspace.to_path_buf();
    let update_id = id.to_owned();
    let update_message = message.to_owned();
    let record = tokio::task::spawn_blocking(move || {
        jobs::update(
            &update_workspace,
            &update_id,
            JobState::Blocked,
            &update_message,
            0,
        )
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
    }
    let failed_dir = workspace.join("failed").join(&record.id);
    if fs::create_dir_all(&failed_dir).is_ok() {
        let _ = fs::copy(&record.input_path, failed_dir.join(&record.filename));
        let _ = fs::write(failed_dir.join("error.txt"), logging::redact(error));
    }
    if let Ok(entry) = logging::append(&state.config_dir, "error", error, Some(&record.id)) {
        if let Ok(mut snapshot) = state.snapshot.lock() {
            snapshot.logs.push(entry);
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
        }
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

fn minimal_environment() -> BTreeMap<String, String> {
    [
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
        "CODEX_HOME",
        "ANTHROPIC_API_KEY",
        "OPENCODE_CONFIG",
    ]
    .into_iter()
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

fn archive_source(
    source: &Path,
    workspace: &Path,
    job_id: &str,
    behavior: &AfterProcessing,
) -> Result<(), String> {
    if matches!(behavior, AfterProcessing::Keep) {
        return Ok(());
    }
    if !source.exists() {
        return Ok(());
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
    fs::rename(source, destination).map_err(|error| format!("archive source PDF: {error}"))
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
    async fn reuses_only_non_empty_task_markdown() {
        let root = std::env::temp_dir().join(format!(
            "cognitio-markdown-reuse-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        tokio::fs::create_dir_all(&root)
            .await
            .expect("parse output directory");
        let markdown = root.join("full.md");

        tokio::fs::write(&markdown, b"")
            .await
            .expect("empty markdown");
        assert_eq!(reusable_markdown(&root).await, None);

        tokio::fs::write(&markdown, b"# Paper\n")
            .await
            .expect("parsed markdown");
        assert_eq!(reusable_markdown(&root).await, Some(markdown));
        tokio::fs::remove_dir_all(root).await.expect("cleanup");
    }
}
