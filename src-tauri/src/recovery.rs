use std::path::Path;

use crate::{
    git,
    jobs::{self, BlockReason, BlockScope, JobRecord, JobStage, JobState},
    publication,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Trigger {
    Startup,
    Retry,
    Failure,
    Cancellation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Decision {
    Keep,
    Queue,
    Verify(String),
    Archive(String),
    Cancel,
    Block {
        reason: BlockReason,
        scope: BlockScope,
        message: String,
        commit: Option<String>,
    },
}

#[derive(Debug)]
pub struct Facts {
    pub clean: Result<bool, String>,
    pub commit: Result<Option<String>, String>,
    pub remote_contains: Result<bool, String>,
}

pub fn decide(record: &JobRecord, facts: &Facts, trigger: Trigger) -> Decision {
    if record.state == JobState::Succeeded {
        return Decision::Keep;
    }
    if trigger == Trigger::Startup
        && matches!(record.state, JobState::Failed | JobState::Cancelled)
        && record.active_run.is_none()
    {
        return Decision::Keep;
    }
    let block = |reason, scope, message: String, commit| Decision::Block {
        reason,
        scope,
        message,
        commit,
    };
    match &facts.clean {
        Ok(false) => {
            return block(
                BlockReason::RepositoryDirty,
                BlockScope::Repository,
                "Wiki has uncommitted changes".into(),
                record.task_commit.clone(),
            )
        }
        Err(error) => {
            return block(
                BlockReason::PublicationInvalid,
                BlockScope::Repository,
                error.clone(),
                record.task_commit.clone(),
            )
        }
        Ok(true) => {}
    }
    if trigger == Trigger::Cancellation || record.state == JobState::Cancelling {
        return Decision::Cancel;
    }
    let commit = match &facts.commit {
        Ok(commit) => commit,
        Err(error) => {
            return block(
                BlockReason::PublicationInvalid,
                BlockScope::Repository,
                error.clone(),
                record.task_commit.clone(),
            )
        }
    };
    if let Some(commit) = commit {
        return match &facts.remote_contains {
            Ok(true) => Decision::Archive(commit.clone()),
            Ok(false) if trigger != Trigger::Startup => Decision::Verify(commit.clone()),
            Ok(false) => block(
                BlockReason::RemoteUnavailable,
                BlockScope::Job,
                "Task commit exists but is not pushed; retry to resume publication".into(),
                Some(commit.clone()),
            ),
            Err(error) => block(
                BlockReason::RemoteUnavailable,
                BlockScope::Job,
                format!("Remote publication has not been confirmed: {error}"),
                Some(commit.clone()),
            ),
        };
    }
    if record.execution.is_none() && trigger != Trigger::Retry {
        return block(
            BlockReason::LegacySettings,
            BlockScope::Job,
            "Retry this job to capture processing settings".into(),
            None,
        );
    }
    if trigger == Trigger::Failure {
        return Decision::Keep;
    }
    if trigger == Trigger::Retry
        || matches!(
            record.state,
            JobState::Queued
                | JobState::Detected
                | JobState::Stabilizing
                | JobState::Preflight
                | JobState::Running
                | JobState::Verifying
        )
        || record.block_reason == Some(BlockReason::RepositoryDirty)
    {
        Decision::Queue
    } else {
        Decision::Keep
    }
}

pub async fn inspect(workspace: &Path, record: &JobRecord, trigger: Trigger) -> Decision {
    if record.state == JobState::Succeeded
        || (trigger == Trigger::Startup
            && matches!(record.state, JobState::Failed | JobState::Cancelled)
            && record.active_run.is_none())
    {
        return Decision::Keep;
    }
    let wiki = workspace.join("wiki");
    let clean = git::output(&wiki, &["status", "--porcelain"])
        .await
        .map(|status| status.is_empty());
    if trigger == Trigger::Cancellation || record.state == JobState::Cancelling {
        return decide(
            record,
            &Facts {
                clean,
                commit: Ok(None),
                remote_contains: Ok(false),
            },
            trigger,
        );
    }
    let mut commit = if clean == Ok(true) {
        publication::find_task_commit(&wiki, record).await
    } else {
        Ok(None)
    };
    if let Ok(Some(found)) = &commit {
        if let Err(error) = publication::validate_task_commit(&wiki, record, found).await {
            commit = Err(error);
        }
    }
    let remote_contains = match &commit {
        Ok(Some(commit)) => git::remote_contains(&wiki, commit).await,
        _ => Ok(false),
    };
    decide(
        record,
        &Facts {
            clean,
            commit,
            remote_contains,
        },
        trigger,
    )
}

pub async fn recover_all(workspace: &Path) -> Result<Vec<JobRecord>, String> {
    let root = workspace.to_path_buf();
    let records = tokio::task::spawn_blocking(move || jobs::load_all(&root))
        .await
        .map_err(|error| format!("join recovery loading: {error}"))??;
    let mut recovered = Vec::new();
    for record in records {
        let decision = inspect(workspace, &record, Trigger::Startup).await;
        let root = workspace.to_path_buf();
        recovered.push(
            tokio::task::spawn_blocking(move || {
                jobs::apply_recovery(&root, &record, decision, true)
            })
            .await
            .map_err(|error| format!("join recovery save: {error}"))??,
        );
    }
    Ok(recovered)
}

pub fn apply(record: &mut JobRecord, decision: Decision) {
    match decision {
        Decision::Keep => return,
        Decision::Queue => {
            record.state = JobState::Queued;
            record.stage = JobStage::Queued;
            record.base_commit = None;
            record.task_commit = None;
            record.remote_confirmed = false;
            record.agent = None;
        }
        Decision::Verify(commit) => {
            record.state = JobState::Verifying;
            record.stage = JobStage::Publishing;
            record.task_commit = Some(commit);
            record.remote_confirmed = false;
        }
        Decision::Archive(commit) => {
            record.state = JobState::Archiving;
            record.stage = JobStage::Archiving;
            record.task_commit = Some(commit);
            record.remote_confirmed = true;
        }
        Decision::Cancel => {
            record.state = JobState::Cancelled;
            record.phase = "Cancelled".into();
            record.error = None;
            return;
        }
        Decision::Block {
            reason,
            scope,
            message,
            commit,
        } => {
            record.state = JobState::Blocked;
            record.block_reason = Some(reason);
            record.block_scope = Some(scope);
            record.phase = message.clone();
            record.error = Some(message);
            record.task_commit = commit;
            return;
        }
    }
    let (_, phase, progress) = record.stage.presentation();
    record.phase = phase.into();
    record.progress = progress;
    record.error = None;
    record.block_scope = None;
    record.block_reason = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_uses_commit_evidence_and_typed_reasons_instead_of_display_text() {
        let root =
            std::env::temp_dir().join(format!("cognitio-recovery-plan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("inbox")).unwrap();
        let source = root.join("inbox/paper.pdf");
        std::fs::write(&source, b"%PDF-1.4\n").unwrap();
        let mut record =
            jobs::create(&root, &source, &crate::models::AppSettings::default()).unwrap();
        record.state = JobState::Blocked;
        record.block_reason = Some(BlockReason::RepositoryDirty);
        record.phase = "arbitrary translated presentation".into();
        let mut facts = Facts {
            clean: Ok(true),
            commit: Ok(None),
            remote_contains: Ok(false),
        };
        assert_eq!(decide(&record, &facts, Trigger::Startup), Decision::Queue);
        facts.commit = Ok(Some("task-commit".into()));
        assert_eq!(
            decide(&record, &facts, Trigger::Retry),
            Decision::Verify("task-commit".into())
        );
        assert!(matches!(
            decide(&record, &facts, Trigger::Startup),
            Decision::Block {
                reason: BlockReason::RemoteUnavailable,
                ..
            }
        ));
        facts.remote_contains = Ok(true);
        assert_eq!(
            decide(&record, &facts, Trigger::Failure),
            Decision::Archive("task-commit".into())
        );
        facts.remote_contains = Err("offline".into());
        assert!(matches!(
            decide(&record, &facts, Trigger::Retry),
            Decision::Block {
                reason: BlockReason::RemoteUnavailable,
                commit: Some(_),
                ..
            }
        ));
        facts.clean = Ok(false);
        assert!(matches!(
            decide(&record, &facts, Trigger::Cancellation),
            Decision::Block {
                reason: BlockReason::RepositoryDirty,
                ..
            }
        ));
        facts.clean = Ok(true);
        assert_eq!(
            decide(&record, &facts, Trigger::Cancellation),
            Decision::Cancel
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
