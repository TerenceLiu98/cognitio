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
    process::Command,
};

use crate::{
    agents,
    commands::AppState,
    jobs::{self, JobRecord, JobState},
    logging,
    models::{AfterProcessing, AppSnapshot},
    tray,
};

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            if let Some(record) = next_job(&app) {
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
    if let Err(error) = run_pipeline(app, &record).await {
        fail_job(app, &record, &error);
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
        );
    }
    transition(
        app,
        &workspace,
        &record.id,
        JobState::Preflight,
        "Checking Wiki and agent",
        15,
    )?;

    let before = git_output(&wiki, &["rev-parse", "HEAD"])?;
    if !git_output(&wiki, &["status", "--porcelain"])?.is_empty() {
        return block_job(app, &workspace, &record.id, "Wiki has uncommitted changes");
    }
    let selected = agents::select(&settings.agent_provider, |kind| {
        command_version(kind).is_some()
    })?;
    let version = command_version(selected)
        .ok_or_else(|| "selected agent version is unavailable".to_string())?;
    jobs::set_execution(&workspace, &record.id, selected.as_str(), before.trim())?;
    let parse_output = workspace.join("processing").join(&record.id).join("parsed");
    let prompt = format!("Use $llmwiki to process the PDF at {}. The job id is {}. Complete validation, commit, and push before returning success.", record.input_path.display(), record.id);
    let mut env = minimal_environment();
    env.insert(
        "LLMWIKI_INPUT".into(),
        record.input_path.to_string_lossy().into_owned(),
    );
    env.insert(
        "LLMWIKI_PARSE_OUTPUT".into(),
        parse_output.to_string_lossy().into_owned(),
    );
    env.insert(
        "LLMWIKI_MINERU_MODE".into(),
        match settings.mineru_mode {
            crate::models::MineruMode::Precision => "precision",
            crate::models::MineruMode::Flash => "flash",
        }
        .into(),
    );
    env.insert("LLMWIKI_JOB_ID".into(), record.id.clone());
    env.insert(
        "LLMWIKI_BIN".into(),
        std::env::current_exe()
            .map_err(|error| format!("resolve LLMWiki executable: {error}"))?
            .to_string_lossy()
            .into_owned(),
    );
    let spec = agents::build_command(
        selected,
        &version,
        &wiki,
        &prompt,
        settings.model.as_deref(),
        env,
    )?;
    transition(
        app,
        &workspace,
        &record.id,
        JobState::Running,
        &format!("Running {}", selected.as_str()),
        35,
    )?;

    let mut child = Command::new(&spec.executable)
        .args(&spec.args)
        .current_dir(&wiki)
        .env_clear()
        .envs(&spec.env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
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
    let error_app = app.clone();
    let error_job_id = record.id.clone();
    let stderr_task = tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            append_log(&error_app, "warn", &line, Some(&error_job_id));
        }
    });
    let mut lines = BufReader::new(stdout).lines();
    let started = std::time::Instant::now();
    loop {
        tokio::select! {
            line = lines.next_line() => match line.map_err(|error| format!("read agent output: {error}"))? {
                Some(line) => { let event = agents::parse_event(&line); if let Some(message) = event.message { append_log(app, "info", &message, Some(&record.id)); } }
                None => break,
            },
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                if is_cancelled(app, &record.id) { child.kill().await.map_err(|error| format!("cancel agent: {error}"))?; return Ok(()); }
                if started.elapsed() >= Duration::from_secs(2 * 60 * 60) { child.kill().await.map_err(|error| format!("stop timed-out agent: {error}"))?; return Err("agent timed out after two hours".into()); }
            }
        }
    }
    let status = child
        .wait()
        .await
        .map_err(|error| format!("wait for agent: {error}"))?;
    let _ = stderr_task.await;
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
    )?;
    verify_publication(
        app,
        record,
        &workspace,
        Some(before.trim()),
        &settings.after_processing,
    )
}

fn verify_publication(
    app: &AppHandle,
    record: &JobRecord,
    workspace: &Path,
    base_commit: Option<&str>,
    after_processing: &AfterProcessing,
) -> Result<(), String> {
    let wiki = workspace.join("wiki");
    let after = git_output(&wiki, &["rev-parse", "HEAD"])?;
    let Some(base_commit) = base_commit else {
        return block_job(
            app,
            workspace,
            &record.id,
            "Job has no recorded baseline commit",
        );
    };
    if base_commit == after {
        return block_job(app, workspace, &record.id, "Agent did not create a commit");
    }
    if !git_output(&wiki, &["status", "--porcelain"])?.is_empty() {
        return block_job(
            app,
            workspace,
            &record.id,
            "Wiki is not clean after agent completion",
        );
    }
    if !git_success(&wiki, &["merge-base", "--is-ancestor", &after, "@{u}"])? {
        return block_job(
            app,
            workspace,
            &record.id,
            "Remote branch does not contain the new commit",
        );
    }
    archive_source(&record.source_path, workspace, &record.id, after_processing)?;
    transition(
        app,
        workspace,
        &record.id,
        JobState::Succeeded,
        "Published",
        100,
    )?;
    append_log(
        app,
        "info",
        &format!("Published {}", record.filename),
        Some(&record.id),
    );
    notify("Added to LLMWiki", &record.filename);
    Ok(())
}

fn transition(
    app: &AppHandle,
    workspace: &Path,
    id: &str,
    next: JobState,
    phase: &str,
    progress: u8,
) -> Result<(), String> {
    let record = jobs::update(workspace, id, next, phase, progress)?;
    publish_record(app, &record);
    Ok(())
}

fn block_job(app: &AppHandle, workspace: &Path, id: &str, message: &str) -> Result<(), String> {
    let record = jobs::update(workspace, id, JobState::Blocked, message, 0)?;
    publish_record(app, &record);
    append_log(app, "warn", message, Some(id));
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
    append_log(app, "error", error, Some(&record.id));
    notify("LLMWiki could not complete this paper", &record.filename);
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

fn append_log(app: &AppHandle, level: &str, message: &str, job_id: Option<&str>) {
    let state = app.state::<AppState>();
    if let Ok(entry) = logging::append(&state.config_dir, level, message, job_id) {
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

fn command_version(kind: agents::AgentKind) -> Option<String> {
    std::process::Command::new(kind.executable())
        .arg("--version")
        .output()
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

fn git_output(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
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

fn git_success(cwd: &Path, args: &[&str]) -> Result<bool, String> {
    std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .status()
        .map(|status| status.success())
        .map_err(|error| format!("run git {}: {error}", args.join(" ")))
}

fn notify(title: &str, body: &str) {
    let script =
        "on run argv\ndisplay notification (item 2 of argv) with title (item 1 of argv)\nend run";
    let _ = std::process::Command::new("osascript")
        .args(["-e", script, title, body])
        .stdin(Stdio::null())
        .spawn();
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
