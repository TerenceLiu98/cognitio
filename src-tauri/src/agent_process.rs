use crate::{agents, job_runtime::Execution, tools};
use std::{collections::BTreeMap, path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Child,
    sync::mpsc,
};

const AGENT_TOTAL_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const AGENT_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Eq, PartialEq)]
pub enum AgentOutcome {
    Completed,
    Cancelled,
}

enum AgentStreamEvent {
    Stdout(String),
    Stderr(String),
    Error(String),
}

pub async fn run(
    run: &Execution,
    selected: agents::AgentKind,
    version: &str,
    wiki: &Path,
    parsed_markdown: &Path,
    model: Option<&str>,
) -> Result<AgentOutcome, String> {
    let record = &run.record;
    let prompt = format!("Use $llmwiki to process the already parsed Markdown at {}. The job id is {}. Do not invoke MinerU, upload a document, or run any parser. Create and validate the intended Wiki files, but do not stage, commit, or push; Cognitio owns Git publication. Do not install dependencies or run Quartz locally; GitHub Actions owns the site build.", parsed_markdown.display(), record.id);
    let mut env = minimal_environment(selected);
    env.insert(
        "LLMWIKI_PARSED_MARKDOWN".into(),
        parsed_markdown.to_string_lossy().into_owned(),
    );
    env.insert("LLMWIKI_JOB_ID".into(), record.id.clone());
    let spec = agents::build_command(selected, version, wiki, &prompt, model, env)?;
    let mut command = tools::tokio_command(&spec.executable)?;
    command
        .args(&spec.args)
        .current_dir(wiki)
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
    supervise(&mut child, run, selected.as_str()).await
}

async fn supervise(
    child: &mut Child,
    run: &Execution,
    agent: &str,
) -> Result<AgentOutcome, String> {
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
    tokio::spawn(async move {
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
    tokio::spawn(async move {
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
    let mut output_open = true;
    let status = loop {
        tokio::select! {
            output = output_rx.recv(), if output_open => match output {
                Some(AgentStreamEvent::Stdout(line)) => {
                    last_activity = std::time::Instant::now();
                    let event = agents::parse_event(&line);
                    if let Some(message) = event.message {
                        run.log("info", &message);
                    }
                }
                Some(AgentStreamEvent::Stderr(line)) => {
                    last_activity = std::time::Instant::now();
                    run.log("warn", &line);
                }
                Some(AgentStreamEvent::Error(error)) => {
                    stop_agent(child, "stop agent after output failure").await?;
                    return Err(error);
                }
                None => output_open = false,
            },
            status = child.wait(), if !output_open => break status.map_err(|error| format!("wait for agent: {error}"))?,
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                if run.is_cancelled() {
                    stop_agent(child, "cancel agent").await?;
                    return Ok(AgentOutcome::Cancelled);
                }
                if let Some(error) = agent_timeout_error(started.elapsed(), last_activity.elapsed()) {
                    stop_agent(child, "stop timed-out agent").await?;
                    return Err(error);
                }
            }
        }
    };
    if !status.success() {
        return Err(format!("{agent} exited with status {status}"));
    }
    if run.is_cancelled() {
        return Ok(AgentOutcome::Cancelled);
    }

    Ok(AgentOutcome::Completed)
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

fn minimal_environment(kind: agents::AgentKind) -> BTreeMap<String, String> {
    let mut keys = vec![
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
    let mut environment: BTreeMap<String, String> = keys
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.into(), value)))
        .collect();
    environment.insert(
        "PATH".into(),
        tools::effective_path().to_string_lossy().into_owned(),
    );
    environment
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn cancellation_still_stops_an_agent_after_both_output_streams_close() {
        let root = std::env::temp_dir().join(format!("cognitio-process-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("inbox")).unwrap();
        let source = root.join("inbox/paper.pdf");
        std::fs::write(&source, b"%PDF-1.4\n").unwrap();
        let job =
            crate::jobs::create(&root, &source, &crate::models::AppSettings::default()).unwrap();
        let runtime = crate::job_runtime::JobRuntime::default();
        let execution = runtime.claim(&root, &job.id).unwrap();
        let mut child = tokio::process::Command::new("/bin/sh")
            .args(["-c", "exec 1>&- 2>&-; sleep 30"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let task_run = execution.clone();
        let task = tokio::spawn(async move { supervise(&mut child, &task_run, "fixture").await });
        tokio::time::sleep(Duration::from_millis(150)).await;
        runtime.cancel(&root, &job.id).unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(outcome, AgentOutcome::Cancelled);
        runtime.finish(&execution, None).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
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
}
