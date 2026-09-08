use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use crate::jobs::{self, JobEvent, JobRecord};

#[derive(Clone)]
pub struct Execution {
    pub record: JobRecord,
    pub workspace: PathBuf,
    cancelled: Arc<AtomicBool>,
    events: tokio::sync::broadcast::Sender<RuntimeEvent>,
}

#[derive(Clone, Debug)]
pub enum RuntimeEvent {
    JobUpdated {
        workspace: PathBuf,
        record: Box<JobRecord>,
    },
    Log {
        level: String,
        message: String,
        job_id: String,
    },
}

impl Execution {
    pub async fn apply(&self, event: JobEvent) -> Result<JobRecord, String> {
        let workspace = self.workspace.clone();
        let expected = self.record.clone();
        let record =
            tokio::task::spawn_blocking(move || jobs::apply_event(&workspace, &expected, event))
                .await
                .map_err(|error| format!("join execution update: {error}"))??;
        self.publish(&record);
        Ok(record)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn publish(&self, record: &JobRecord) {
        let _ = self.events.send(RuntimeEvent::JobUpdated {
            workspace: self.workspace.clone(),
            record: Box::new(record.clone()),
        });
    }

    pub fn log(&self, level: &str, message: &str) {
        let _ = self.events.send(RuntimeEvent::Log {
            level: level.into(),
            message: message.into(),
            job_id: self.record.id.clone(),
        });
    }
}

pub struct JobRuntime {
    active: Mutex<BTreeMap<(PathBuf, String), Execution>>,
    events: tokio::sync::broadcast::Sender<RuntimeEvent>,
    pub repository: tokio::sync::Mutex<()>,
}

impl Default for JobRuntime {
    fn default() -> Self {
        let (events, _) = tokio::sync::broadcast::channel(512);
        Self {
            active: Mutex::new(BTreeMap::new()),
            events,
            repository: tokio::sync::Mutex::new(()),
        }
    }
}

impl JobRuntime {
    pub async fn retry(
        &self,
        workspace: &Path,
        id: &str,
        settings: &crate::models::AppSettings,
        force_reparse: bool,
    ) -> Result<JobRecord, String> {
        let _guard = self.repository.lock().await;
        let root = workspace.to_path_buf();
        let id = id.to_owned();
        let current = tokio::task::spawn_blocking(move || jobs::load(&root, &id))
            .await
            .map_err(|error| format!("join retry load: {error}"))??;
        if current.active_run.is_some() {
            return Err("wait for the current execution to stop before retrying".into());
        }
        let decision =
            crate::recovery::inspect(workspace, &current, crate::recovery::Trigger::Retry).await;
        let root = workspace.to_path_buf();
        let settings = settings.clone();
        let record = tokio::task::spawn_blocking(move || {
            jobs::apply_retry(&root, &current, &settings, force_reparse, decision)
        })
        .await
        .map_err(|error| format!("join retry save: {error}"))??;
        let _ = self.events.send(RuntimeEvent::JobUpdated {
            workspace: workspace.to_path_buf(),
            record: Box::new(record.clone()),
        });
        Ok(record)
    }
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<RuntimeEvent> {
        self.events.subscribe()
    }

    pub fn claim(&self, workspace: &Path, id: &str) -> Result<Execution, String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "execution registry lock is poisoned".to_string())?;
        let key = (workspace.to_path_buf(), id.to_owned());
        if active.contains_key(&key) {
            return Err("job already has an active execution".into());
        }
        let record = jobs::claim(workspace, id, &uuid::Uuid::new_v4().to_string())?;
        let execution = Execution {
            record,
            workspace: workspace.to_path_buf(),
            cancelled: Arc::new(AtomicBool::new(false)),
            events: self.events.clone(),
        };
        active.insert(key, execution.clone());
        execution.publish(&execution.record);
        Ok(execution)
    }

    pub fn cancel(&self, workspace: &Path, id: &str) -> Result<JobRecord, String> {
        let active = self
            .active
            .lock()
            .map_err(|_| "execution registry lock is poisoned".to_string())?;
        let record = jobs::request_cancel(workspace, id)?;
        if let Some(execution) = active.get(&(workspace.to_path_buf(), id.to_owned())) {
            execution.cancelled.store(true, Ordering::Release);
            execution.publish(&record);
        }
        Ok(record)
    }

    pub fn finish(&self, execution: &Execution, error: Option<&str>) -> Result<JobRecord, String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "execution registry lock is poisoned".to_string())?;
        let key = (execution.workspace.clone(), execution.record.id.clone());
        let run_id = execution
            .record
            .active_run
            .as_deref()
            .ok_or("execution has no identity")?;
        let record =
            jobs::finish_execution(&execution.workspace, &execution.record.id, run_id, error)?;
        active.remove(&key);
        execution.publish(&record);
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        jobs::JobState,
        models::{AppSettings, JobSummary},
    };

    #[test]
    fn cancellation_requires_acknowledgement_and_old_execution_cannot_finish_retry() {
        let root =
            std::env::temp_dir().join(format!("cognitio-cancellation-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("inbox")).unwrap();
        let source = root.join("inbox/paper.pdf");
        std::fs::write(&source, b"%PDF-1.4\n").unwrap();
        let settings = AppSettings::default();
        let job = jobs::create(&root, &source, &settings).unwrap();
        let runtime = JobRuntime::default();
        let first = runtime.claim(&root, &job.id).unwrap();
        assert!(runtime.claim(&root, &job.id).is_err());
        let cancelling = runtime.cancel(&root, &job.id).unwrap();
        assert!(first.is_cancelled());
        assert_eq!(cancelling.state, JobState::Cancelling);
        assert!(JobSummary::from(&cancelling).allowed_actions.is_empty());
        assert!(jobs::retry(&root, &job.id, &settings, false).is_err());
        let stopped = runtime.finish(&first, None).unwrap();
        assert_eq!(stopped.state, JobState::Cancelled);
        jobs::retry(&root, &job.id, &settings, false).unwrap();
        let second = runtime.claim(&root, &job.id).unwrap();
        assert!(!second.is_cancelled());
        assert!(runtime.finish(&first, None).is_err());
        assert!(
            jobs::apply_event(&root, &first.record, JobEvent::TaskCommit("stale".into())).is_err()
        );
        assert_eq!(
            jobs::load(&root, &job.id).unwrap().active_run,
            second.record.active_run
        );
        runtime.cancel(&root, &job.id).unwrap();
        runtime.finish(&second, None).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_execution_updates_preserve_both_fields_and_revisions() {
        let root = std::env::temp_dir().join(format!("cognitio-updates-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("inbox")).unwrap();
        let source = root.join("inbox/paper.pdf");
        std::fs::write(&source, b"%PDF-1.4\n").unwrap();
        let job = jobs::create(&root, &source, &AppSettings::default()).unwrap();
        let runtime = JobRuntime::default();
        let execution = runtime.claim(&root, &job.id).unwrap();
        std::thread::scope(|scope| {
            for field in 0..2 {
                let root = &root;
                let expected = &execution.record;
                scope.spawn(move || {
                    for index in 0..25 {
                        let event = if field == 0 {
                            JobEvent::ExecutionMetadata {
                                agent: "codex".into(),
                                base_commit: format!("base-{index}"),
                            }
                        } else {
                            JobEvent::TaskCommit(format!("commit-{index}"))
                        };
                        jobs::apply_event(root, expected, event).unwrap();
                    }
                });
            }
        });
        let saved = jobs::load(&root, &job.id).unwrap();
        assert_eq!(saved.base_commit.as_deref(), Some("base-24"));
        assert_eq!(saved.task_commit.as_deref(), Some("commit-24"));
        assert_eq!(saved.revision, execution.record.revision + 50);
        std::fs::remove_dir_all(root).unwrap();
    }
}
