use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::{AppHandle, Manager};
use tokio::task::JoinSet;

#[cfg(test)]
use crate::models::DeploymentSummary;
use crate::{
    agent_process::{self, AgentOutcome},
    agents,
    app_events::{append_log, notify, publish_record},
    app_state::AppState,
    archive,
    git::output as git_output,
    job_runtime::{Execution, JobRuntime},
    jobs::{self, BlockReason, BlockScope, JobEvent, JobRecord, JobStage, JobState},
    parsing,
    publication::{self, PublicationOutcome},
    tools,
};

const MAX_CONCURRENT_JOBS: usize = 3;

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut tasks = JoinSet::new();
        let mut active: HashMap<tokio::task::Id, Execution> = HashMap::new();
        loop {
            while active.len() < MAX_CONCURRENT_JOBS {
                let lookup_app = app.clone();
                let active_ids = active
                    .values()
                    .map(|execution| execution.record.id.clone())
                    .collect();
                let record =
                    tokio::task::spawn_blocking(move || next_job(&lookup_app, &active_ids))
                        .await
                        .unwrap_or(None);
                let Some(record) = record else {
                    break;
                };
                let execution = record.clone();
                let task_app = app.clone();
                let handle = tasks.spawn(async move {
                    process(&task_app, &record).await;
                });
                active.insert(handle.id(), execution);
            }

            if tasks.is_empty() {
                tokio::time::sleep(Duration::from_secs(1)).await;
            } else {
                tokio::select! {
                    result = tasks.join_next_with_id() => {
                        if let Some(result) = result {
                            let task_id = match result {
                                Ok((task_id, ())) => task_id,
                                Err(error) => {
                                    append_log(
                                        &app,
                                        "error",
                                        &format!("Worker task stopped unexpectedly: {error}"),
                                        active.get(&error.id()).map(|execution| execution.record.id.as_str()),
                                    )
                                    .await;
                                    error.id()
                                }
                            };
                            if let Some(execution) = active.remove(&task_id) {
                                let runtime = app.state::<AppState>().jobs.clone();
                                let workspace = execution.workspace.clone();
                                match tokio::task::spawn_blocking(move || runtime.finish(&execution, None)).await {
                                    Ok(Ok(record)) => {
                                        publish_record(&app, &record);
                                        if record.state == JobState::Succeeded {
                                            notify("Added to Cognitio", &record.filename);
                                            crate::deployment::monitor(app.clone(), workspace, record.id.clone(), record.task_commit.clone());
                                        }
                                    },
                                    result => append_log(&app, "error", &format!("Could not finalize execution: {result:?}"), None).await,
                                }
                            }
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                }
            }
        }
    });
}

fn next_job(app: &AppHandle, active: &BTreeSet<String>) -> Option<Execution> {
    let state = app.state::<AppState>();
    let snapshot = state.snapshot.lock().ok()?;
    if !snapshot.configured || !snapshot.watching {
        return None;
    }
    let workspace = PathBuf::from(&snapshot.settings.workspace_root);
    drop(snapshot);
    let records = match jobs::load_all(&workspace) {
        Ok(records) => records,
        Err(error) => {
            if let Ok(mut snapshot) = state.snapshot.lock() {
                snapshot.watching = false;
            }
            crate::app_events::append_log_blocking(app, "error", &error, None);
            return None;
        }
    };
    if records.iter().any(|job| {
        job.state == JobState::Blocked && job.block_scope == Some(BlockScope::Repository)
    }) {
        return None;
    }
    let record = select_next_job(records, active)?;
    let execution = state.jobs.claim(&workspace, &record.id).ok()?;
    publish_record(app, &execution.record);
    Some(execution)
}

fn select_next_job(records: Vec<JobRecord>, active: &BTreeSet<String>) -> Option<JobRecord> {
    [JobState::Archiving, JobState::Verifying, JobState::Queued]
        .into_iter()
        .find_map(|state| {
            records
                .iter()
                .rev()
                .find(|job| job.state == state && !active.contains(&job.id))
                .cloned()
        })
}

async fn repository_blocked_by_other(workspace: &Path, job_id: &str) -> Result<bool, String> {
    let workspace = workspace.to_path_buf();
    let job_id = job_id.to_owned();
    tokio::task::spawn_blocking(move || {
        jobs::load_all(&workspace).map(|records| {
            records.into_iter().any(|job| {
                job.id != job_id
                    && job.state == JobState::Blocked
                    && job.block_scope == Some(BlockScope::Repository)
            })
        })
    })
    .await
    .map_err(|error| format!("join repository block lookup: {error}"))?
}

async fn process(app: &AppHandle, run: &Execution) {
    let runtime = app.state::<AppState>().jobs.clone();
    let result = run_pipeline(&runtime, run).await;
    if run.is_cancelled() {
        let _ = reconcile_interruption(&runtime, run, true).await;
        return;
    }
    if let Err(error) = result {
        match reconcile_interruption(&runtime, run, false).await {
            Ok(true) => return,
            Err(recovery_error) => run.log("error", &recovery_error),
            Ok(false) => {}
        }
        if let Err(update_error) = run.apply(JobEvent::Failed(error.clone())).await {
            run.log("error", &update_error);
        }
        run.log("error", &error);
    }
}

async fn load_current(run: &Execution) -> Result<JobRecord, String> {
    let workspace = run.workspace.clone();
    let id = run.record.id.clone();
    tokio::task::spawn_blocking(move || jobs::load(&workspace, &id))
        .await
        .map_err(|error| format!("join job load: {error}"))?
}

async fn reconcile_interruption(
    runtime: &JobRuntime,
    run: &Execution,
    cancelled: bool,
) -> Result<bool, String> {
    let current = load_current(run).await?;
    if current.base_commit.is_none() {
        return Ok(cancelled);
    }
    if !matches!(
        current.state,
        JobState::Running | JobState::Verifying | JobState::Cancelling
    ) {
        return Ok(cancelled);
    }
    let guard = runtime.repository.lock().await;
    let trigger = if cancelled {
        crate::recovery::Trigger::Cancellation
    } else {
        crate::recovery::Trigger::Failure
    };
    let decision = crate::recovery::inspect(&run.workspace, &current, trigger).await;
    if decision == crate::recovery::Decision::Keep {
        return Ok(false);
    }
    let workspace = run.workspace.clone();
    let updated = tokio::task::spawn_blocking(move || {
        jobs::apply_recovery(&workspace, &current, decision, false)
    })
    .await
    .map_err(|error| format!("join interruption recovery: {error}"))??;
    run.publish(&updated);
    if updated.state == JobState::Verifying {
        let outcome =
            publication::verify(&updated, &run.workspace, updated.base_commit.as_deref()).await?;
        drop(guard);
        finish_publication(run, outcome).await?;
    } else if updated.state == JobState::Archiving {
        drop(guard);
        archive_and_succeed(run, &updated).await?;
    }
    Ok(true)
}

async fn run_pipeline(runtime: &JobRuntime, run: &Execution) -> Result<(), String> {
    run_pipeline_with(runtime, run, &LocalAdapters).await
}

trait PipelineAdapters {
    async fn detect(&self) -> Result<Vec<crate::models::ToolCapability>, String>;
    async fn parse(&self, run: &Execution, output: &Path) -> Result<Option<PathBuf>, String>;
    async fn generate(
        &self,
        run: &Execution,
        agent: agents::AgentKind,
        version: &str,
        wiki: &Path,
        markdown: &Path,
    ) -> Result<AgentOutcome, String>;
}

struct LocalAdapters;

impl PipelineAdapters for LocalAdapters {
    async fn detect(&self) -> Result<Vec<crate::models::ToolCapability>, String> {
        tokio::task::spawn_blocking(tools::detect_all)
            .await
            .map_err(|error| format!("join agent detection: {error}"))
    }

    async fn parse(&self, run: &Execution, output: &Path) -> Result<Option<PathBuf>, String> {
        let settings = run
            .record
            .execution
            .as_ref()
            .ok_or("missing execution settings")?;
        parsing::prepare(run, output, settings.mineru_mode.clone()).await
    }

    async fn generate(
        &self,
        run: &Execution,
        agent: agents::AgentKind,
        version: &str,
        wiki: &Path,
        markdown: &Path,
    ) -> Result<AgentOutcome, String> {
        let settings = run
            .record
            .execution
            .as_ref()
            .ok_or("missing execution settings")?;
        agent_process::run(
            run,
            agent,
            version,
            wiki,
            markdown,
            settings.model.as_deref(),
        )
        .await
    }
}

async fn run_pipeline_with(
    runtime: &JobRuntime,
    run: &Execution,
    adapters: &impl PipelineAdapters,
) -> Result<(), String> {
    let record = &run.record;
    let workspace = &run.workspace;
    let wiki = workspace.join("wiki");
    let settings = record
        .execution
        .as_ref()
        .ok_or("job has no captured execution settings; retry it first")?;
    if record.state == JobState::Archiving {
        return archive_and_succeed(run, record).await;
    }
    if record.state == JobState::Verifying {
        let guard = runtime.repository.lock().await;
        let outcome = publication::verify(record, workspace, record.base_commit.as_deref()).await?;
        drop(guard);
        return finish_publication(run, outcome).await;
    }

    run.apply(JobEvent::Advance(JobStage::Preflight)).await?;
    let detected = adapters.detect().await?;
    let (agent, version) = agents::select_supported(&settings.agent_provider, &detected)?;
    run.apply(JobEvent::Advance(JobStage::Parsing)).await?;
    let output = workspace.join("processing").join(&record.id).join("parsed");
    let Some(markdown) = adapters.parse(run, &output).await? else {
        return Ok(());
    };
    run.apply(JobEvent::Advance(JobStage::WaitingForWiki))
        .await?;

    let guard = loop {
        if run.is_cancelled() {
            return Ok(());
        }
        let guard = tokio::select! {
            guard = runtime.repository.lock() => guard,
            _ = tokio::time::sleep(Duration::from_millis(100)) => continue,
        };
        if !repository_blocked_by_other(workspace, &record.id).await? {
            break guard;
        }
        drop(guard);
        tokio::time::sleep(Duration::from_secs(1)).await;
    };
    if run.is_cancelled() {
        return Ok(());
    }
    if !git_output(&wiki, &["status", "--porcelain"])
        .await?
        .is_empty()
    {
        return block(
            run,
            BlockReason::RepositoryDirty,
            BlockScope::Repository,
            "Wiki has uncommitted changes",
        )
        .await;
    }
    git_output(&wiki, &["pull", "--ff-only"]).await?;
    let baseline = git_output(&wiki, &["rev-parse", "HEAD"]).await?;
    run.apply(JobEvent::ExecutionMetadata {
        agent: agent.as_str().into(),
        base_commit: baseline.clone(),
    })
    .await?;
    run.apply(JobEvent::Advance(JobStage::Generating)).await?;
    if adapters
        .generate(run, agent, version, &wiki, &markdown)
        .await?
        == AgentOutcome::Cancelled
    {
        return Ok(());
    }
    // Publication is non-cancellable once accepted; persist this boundary before creating a commit.
    run.apply(JobEvent::Advance(JobStage::Publishing)).await?;
    publication::commit_agent_changes(&wiki, &record.id, &baseline).await?;
    let current = load_current(run).await?;
    let outcome = publication::verify(&current, workspace, Some(&baseline)).await?;
    drop(guard);
    finish_publication(run, outcome).await
}

async fn finish_publication(run: &Execution, outcome: PublicationOutcome) -> Result<(), String> {
    match outcome {
        PublicationOutcome::Published(record) => {
            run.publish(&record);
            archive_and_succeed(run, &record).await
        }
        PublicationOutcome::Blocked(record) => {
            run.publish(&record);
            run.log(
                "warn",
                record.error.as_deref().unwrap_or("Publication blocked"),
            );
            Ok(())
        }
    }
}

async fn archive_and_succeed(run: &Execution, record: &JobRecord) -> Result<(), String> {
    let settings = record
        .execution
        .as_ref()
        .ok_or("job has no captured execution settings")?;
    let completed = archive::complete(&run.workspace, record, &settings.after_processing).await?;
    run.publish(&completed);
    run.log("info", &format!("Published {}", record.filename));
    Ok(())
}

async fn block(
    run: &Execution,
    reason: BlockReason,
    scope: BlockScope,
    message: &str,
) -> Result<(), String> {
    run.apply(JobEvent::Blocked {
        reason,
        scope,
        message: message.into(),
    })
    .await?;
    run.log("warn", message);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelling_before_generation_does_not_wait_for_another_jobs_repository_lock() {
        let root =
            std::env::temp_dir().join(format!("cognitio-cancel-wait-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("inbox")).unwrap();
        let source = root.join("inbox/paper.pdf");
        std::fs::write(&source, b"PDF").unwrap();
        let job = jobs::create(&root, &source, &crate::models::AppSettings::default()).unwrap();
        let runtime = JobRuntime::default();
        let run = runtime.claim(&root, &job.id).unwrap();
        run.apply(JobEvent::Advance(JobStage::Preflight))
            .await
            .unwrap();
        run.apply(JobEvent::Advance(JobStage::Parsing))
            .await
            .unwrap();
        run.apply(JobEvent::Advance(JobStage::WaitingForWiki))
            .await
            .unwrap();
        let _guard = runtime.repository.lock().await;
        runtime.cancel(&root, &job.id).unwrap();
        assert!(tokio::time::timeout(
            Duration::from_secs(1),
            reconcile_interruption(&runtime, &run, true)
        )
        .await
        .unwrap()
        .unwrap());
        assert_eq!(
            runtime.finish(&run, None).unwrap().state,
            JobState::Cancelled
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    struct FakeAdapters {
        parses: std::sync::atomic::AtomicUsize,
        generations: std::sync::atomic::AtomicUsize,
    }

    impl PipelineAdapters for FakeAdapters {
        async fn detect(&self) -> Result<Vec<crate::models::ToolCapability>, String> {
            Ok(vec![crate::models::ToolCapability {
                id: "codex".into(),
                detected: true,
                version: Some("0.146.0".into()),
                authenticated: None,
                detail: None,
            }])
        }
        async fn parse(&self, _run: &Execution, output: &Path) -> Result<Option<PathBuf>, String> {
            self.parses
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::fs::create_dir_all(output).await.unwrap();
            let markdown = output.join("full.md");
            tokio::fs::write(&markdown, "# Parsed paper\n")
                .await
                .unwrap();
            Ok(Some(markdown))
        }
        async fn generate(
            &self,
            _run: &Execution,
            _agent: agents::AgentKind,
            _version: &str,
            wiki: &Path,
            _markdown: &Path,
        ) -> Result<AgentOutcome, String> {
            self.generations
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::fs::create_dir_all(wiki.join("content/papers"))
                .await
                .unwrap();
            tokio::fs::write(wiki.join("content/papers/paper.md"), "# Paper\n")
                .await
                .unwrap();
            Ok(AgentOutcome::Completed)
        }
    }

    #[tokio::test]
    async fn pipeline_retry_resumes_publication_without_repeating_parser_or_agent() {
        let root = std::env::temp_dir().join(format!("cognitio-pipeline-{}", uuid::Uuid::new_v4()));
        let workspace = root.join("workspace");
        let wiki = workspace.join("wiki");
        let remote = root.join("remote.git");
        std::fs::create_dir_all(&wiki).unwrap();
        std::fs::create_dir_all(workspace.join("inbox")).unwrap();
        let git = |cwd: &Path, args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(cwd)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&root, &["init", "--bare", remote.to_str().unwrap()]);
        git(&wiki, &["init", "--initial-branch", "main"]);
        git(&wiki, &["config", "user.name", "Cognitio Test"]);
        git(&wiki, &["config", "user.email", "test@cognitio.local"]);
        std::fs::write(wiki.join("README.md"), "base\n").unwrap();
        git(&wiki, &["add", "."]);
        git(&wiki, &["commit", "-m", "Base"]);
        git(
            &wiki,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&wiki, &["push", "-u", "origin", "main"]);
        git(
            &wiki,
            &[
                "remote",
                "set-url",
                "--push",
                "origin",
                root.join("missing.git").to_str().unwrap(),
            ],
        );
        let source = workspace.join("inbox/paper.pdf");
        std::fs::write(&source, b"%PDF-1.4\n").unwrap();
        let settings = crate::models::AppSettings::default();
        let job = jobs::create(&workspace, &source, &settings).unwrap();
        let runtime = JobRuntime::default();
        let adapters = FakeAdapters {
            parses: 0.into(),
            generations: 0.into(),
        };
        let first = runtime.claim(&workspace, &job.id).unwrap();
        run_pipeline_with(&runtime, &first, &adapters)
            .await
            .unwrap();
        let blocked = runtime.finish(&first, None).unwrap();
        assert_eq!(blocked.state, JobState::Blocked);
        assert!(source.is_file());
        assert!(job.input_path.is_file());
        git(
            &wiki,
            &[
                "remote",
                "set-url",
                "--push",
                "origin",
                remote.to_str().unwrap(),
            ],
        );
        runtime
            .retry(&workspace, &job.id, &settings, false)
            .await
            .unwrap();
        let second = runtime.claim(&workspace, &job.id).unwrap();
        run_pipeline_with(&runtime, &second, &adapters)
            .await
            .unwrap();
        let completed = runtime.finish(&second, None).unwrap();
        assert_eq!(completed.state, JobState::Succeeded);
        assert_eq!(completed.task_commit, blocked.task_commit);
        assert!(completed.remote_confirmed);
        assert_eq!(completed.deployment.status, "pending");
        assert!(!source.exists());
        assert!(workspace.join("done/paper.pdf").is_file());
        assert_eq!(adapters.parses.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            adapters
                .generations
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_work_is_scheduled_before_new_jobs() {
        let record = |id: &str, state: JobState, updated_at: &str| JobRecord {
            schema_version: 2,
            id: id.into(),
            filename: format!("{id}.pdf"),
            source_path: format!("inbox/{id}.pdf").into(),
            input_path: format!("processing/{id}/{id}.pdf").into(),
            sha256: id.into(),
            state,
            phase: "test".into(),
            progress: 0,
            agent: None,
            created_at: updated_at.into(),
            updated_at: updated_at.into(),
            error: None,
            base_commit: None,
            task_commit: None,
            execution: None,
            force_reparse: false,
            block_scope: None,
            archive_destination: None,
            deployment: DeploymentSummary::default(),
            revision: 0,
            active_run: None,
            stage: crate::jobs::JobStage::Queued,
            block_reason: None,
            remote_confirmed: false,
        };
        let records = vec![
            record("queued", JobState::Queued, "2026-01-03T00:00:00Z"),
            record("verifying", JobState::Verifying, "2026-01-02T00:00:00Z"),
            record("archiving", JobState::Archiving, "2026-01-01T00:00:00Z"),
        ];

        assert_eq!(MAX_CONCURRENT_JOBS, 3);
        assert_eq!(
            select_next_job(records.clone(), &BTreeSet::new())
                .expect("next job")
                .id,
            "archiving"
        );
        assert_eq!(
            select_next_job(records, &BTreeSet::from(["archiving".into()]))
                .expect("next non-active job")
                .id,
            "verifying"
        );
    }
}
