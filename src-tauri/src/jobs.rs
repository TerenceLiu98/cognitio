use std::{
    fs,
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::models::{
    AfterProcessing, AgentProvider, AppSettings, DeploymentSummary, JobSummary, MineruMode,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Detected,
    Stabilizing,
    Queued,
    Preflight,
    Running,
    Verifying,
    Archiving,
    Succeeded,
    Blocked,
    Failed,
    Cancelled,
    Cancelling,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Detected => "detected",
            Self::Stabilizing => "stabilizing",
            Self::Queued => "queued",
            Self::Preflight => "preflight",
            Self::Running => "running",
            Self::Verifying => "verifying",
            Self::Archiving => "archiving",
            Self::Succeeded => "succeeded",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Cancelling => "cancelling",
        }
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Detected, Self::Stabilizing)
                | (
                    Self::Stabilizing,
                    Self::Queued | Self::Failed | Self::Cancelled
                )
                | (
                    Self::Queued,
                    Self::Preflight | Self::Failed | Self::Cancelled | Self::Cancelling
                )
                | (
                    Self::Preflight,
                    Self::Running
                        | Self::Blocked
                        | Self::Failed
                        | Self::Cancelled
                        | Self::Cancelling
                )
                | (
                    Self::Running,
                    Self::Verifying
                        | Self::Blocked
                        | Self::Failed
                        | Self::Cancelled
                        | Self::Cancelling
                )
                | (
                    Self::Cancelling,
                    Self::Cancelled | Self::Blocked | Self::Failed
                )
                | (
                    Self::Verifying,
                    Self::Archiving | Self::Blocked | Self::Failed
                )
                | (
                    Self::Archiving,
                    Self::Succeeded | Self::Blocked | Self::Failed
                )
                | (
                    Self::Blocked | Self::Failed | Self::Cancelled,
                    Self::Queued | Self::Verifying
                )
        )
    }

    pub fn allowed_actions(self) -> Vec<String> {
        match self {
            Self::Detected | Self::Stabilizing | Self::Queued | Self::Preflight | Self::Running => {
                vec!["cancel".into()]
            }
            Self::Blocked | Self::Failed | Self::Cancelled => {
                vec!["retry".into(), "reparse".into()]
            }
            Self::Verifying | Self::Archiving | Self::Succeeded | Self::Cancelling => Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BlockScope {
    Job,
    Repository,
}

impl BlockScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::Job => "job",
            Self::Repository => "repository",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionSettings {
    pub agent_provider: AgentProvider,
    pub model: Option<String>,
    pub mineru_mode: MineruMode,
    pub after_processing: AfterProcessing,
}

impl From<&AppSettings> for ExecutionSettings {
    fn from(settings: &AppSettings) -> Self {
        Self {
            agent_provider: settings.agent_provider.clone(),
            model: settings.model.clone(),
            mineru_mode: settings.mineru_mode.clone(),
            after_processing: settings.after_processing.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRecord {
    pub schema_version: u32,
    pub id: String,
    pub filename: String,
    pub source_path: PathBuf,
    pub input_path: PathBuf,
    pub sha256: String,
    pub state: JobState,
    pub phase: String,
    pub progress: u8,
    pub agent: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub error: Option<String>,
    #[serde(default)]
    pub base_commit: Option<String>,
    #[serde(default, alias = "publishedCommit")]
    pub task_commit: Option<String>,
    #[serde(default)]
    pub execution: Option<ExecutionSettings>,
    #[serde(default)]
    pub force_reparse: bool,
    #[serde(default)]
    pub block_scope: Option<BlockScope>,
    #[serde(default)]
    pub archive_destination: Option<PathBuf>,
    #[serde(default)]
    pub deployment: DeploymentSummary,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub active_run: Option<String>,
    #[serde(default)]
    pub stage: JobStage,
    #[serde(default)]
    pub block_reason: Option<BlockReason>,
    #[serde(default)]
    pub remote_confirmed: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobStage {
    #[default]
    Queued,
    Preflight,
    Parsing,
    WaitingForWiki,
    Generating,
    Publishing,
    Archiving,
    Complete,
}

impl JobStage {
    pub fn presentation(self) -> (JobState, &'static str, u8) {
        match self {
            Self::Queued => (JobState::Queued, "Waiting for processor", 10),
            Self::Preflight => (JobState::Preflight, "Checking parser and agent", 15),
            Self::Parsing => (JobState::Running, "Parsing PDF with MinerU", 25),
            Self::WaitingForWiki => (JobState::Running, "Waiting for Wiki publication", 45),
            Self::Generating => (JobState::Running, "Generating linked knowledge pages", 55),
            Self::Publishing => (JobState::Verifying, "Verifying Git publication", 85),
            Self::Archiving => (JobState::Archiving, "Archiving source PDF", 95),
            Self::Complete => (JobState::Succeeded, "Published", 100),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockReason {
    RepositoryDirty,
    PublicationInvalid,
    RemoteUnavailable,
    LegacySettings,
}

#[derive(Clone, Debug)]
pub enum JobEvent {
    Advance(JobStage),
    ExecutionMetadata {
        agent: String,
        base_commit: String,
    },
    ParseReady,
    TaskCommit(String),
    PublicationConfirmed(String),
    ArchivePlanned(Option<PathBuf>),
    Archived,
    Blocked {
        reason: BlockReason,
        scope: BlockScope,
        message: String,
    },
    Failed(String),
}

pub fn apply_event(
    workspace: &Path,
    expected: &JobRecord,
    event: JobEvent,
) -> Result<JobRecord, String> {
    mutate(workspace, &expected.id, |record| {
        if record.active_run != expected.active_run {
            return Err("execution no longer owns this job".into());
        }
        match event {
            JobEvent::Advance(stage) => {
                if record.state == JobState::Cancelling {
                    return Err("execution is being cancelled".into());
                }
                let (state, phase, progress) = stage.presentation();
                if state != record.state {
                    record.transition(state, phase, progress)?;
                }
                record.stage = stage;
                record.phase = phase.into();
                record.progress = progress;
            }
            JobEvent::ExecutionMetadata { agent, base_commit } => {
                record.agent = Some(agent);
                record.base_commit = Some(base_commit);
            }
            JobEvent::ParseReady => record.force_reparse = false,
            JobEvent::TaskCommit(commit) => record.task_commit = Some(commit),
            JobEvent::PublicationConfirmed(commit) => {
                if record.task_commit.as_deref() != Some(&commit) {
                    return Err("remote confirmation does not match the task commit".into());
                }
                record.remote_confirmed = true;
                record.transition(JobState::Archiving, "Archiving source PDF", 95)?;
                record.stage = JobStage::Archiving;
            }
            JobEvent::ArchivePlanned(destination) => {
                if record.state != JobState::Archiving || !record.remote_confirmed {
                    return Err("source archival requires a confirmed publication".into());
                }
                record.archive_destination = destination;
            }
            JobEvent::Archived => {
                record.transition(JobState::Succeeded, "Published", 100)?;
                record.stage = JobStage::Complete;
                record.deployment = DeploymentSummary {
                    status: "pending".into(),
                    updated_at: Some(Utc::now().to_rfc3339()),
                    ..DeploymentSummary::default()
                };
            }
            JobEvent::Blocked {
                reason,
                scope,
                message,
            } => {
                record.transition(JobState::Blocked, &message, record.progress)?;
                record.error = Some(message);
                record.block_reason = Some(reason);
                record.block_scope = Some(scope);
            }
            JobEvent::Failed(message) => {
                record.transition(JobState::Failed, "Failed", record.progress)?;
                record.error = Some(message);
            }
        }
        Ok(())
    })
}

impl JobRecord {
    pub fn transition(
        &mut self,
        next: JobState,
        phase: impl Into<String>,
        progress: u8,
    ) -> Result<(), String> {
        if !self.state.can_transition_to(next) {
            return Err(format!(
                "invalid job transition: {} -> {}",
                self.state.as_str(),
                next.as_str()
            ));
        }
        self.state = next;
        self.phase = phase.into();
        self.progress = progress.min(100);
        if next == JobState::Queued {
            self.agent = None;
            self.error = None;
            self.base_commit = None;
            self.task_commit = None;
            self.block_scope = None;
            self.block_reason = None;
            self.remote_confirmed = false;
            self.stage = JobStage::Queued;
            self.archive_destination = None;
            self.deployment = DeploymentSummary::default();
        }
        self.updated_at = Utc::now().to_rfc3339();
        Ok(())
    }
}

impl From<&JobRecord> for JobSummary {
    fn from(record: &JobRecord) -> Self {
        let mut allowed_actions = record.state.allowed_actions();
        if record.active_run.is_some() {
            allowed_actions.retain(|action| action == "cancel");
        }
        if record.task_commit.is_some() {
            allowed_actions.retain(|action| action != "reparse");
        }
        Self {
            id: record.id.clone(),
            stage: record.stage,
            block_reason: record.block_reason,
            filename: record.filename.clone(),
            state: record.state.as_str().into(),
            phase: record.phase.clone(),
            progress: record.progress,
            agent: record.agent.clone(),
            created_at: record.created_at.clone(),
            updated_at: record.updated_at.clone(),
            error: record.error.clone(),
            allowed_actions,
            block_scope: record.block_scope.map(|scope| scope.as_str().into()),
            mineru_mode: record
                .execution
                .as_ref()
                .map(|execution| match execution.mineru_mode {
                    MineruMode::Precision => "precision".into(),
                    MineruMode::Flash => "flash".into(),
                }),
            deployment: record.deployment.clone(),
            revision: record.revision,
        }
    }
}

pub fn create(
    workspace: &Path,
    source: &Path,
    settings: &AppSettings,
) -> Result<JobRecord, String> {
    let filename = source
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "PDF filename is not valid UTF-8".to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let directory = workspace.join("processing").join(&id);
    fs::create_dir_all(&directory).map_err(|error| format!("create job directory: {error}"))?;
    let input_path = directory.join(filename);
    fs::copy(source, &input_path)
        .map_err(|error| format!("copy PDF into job directory: {error}"))?;
    let now = Utc::now().to_rfc3339();
    let captured_hash = sha256(&input_path)?;
    let record = JobRecord {
        schema_version: 3,
        id,
        filename: filename.into(),
        source_path: source.to_path_buf(),
        input_path,
        sha256: captured_hash,
        state: JobState::Queued,
        phase: "Waiting for processor".into(),
        progress: 10,
        agent: None,
        created_at: now.clone(),
        updated_at: now,
        error: None,
        base_commit: None,
        task_commit: None,
        execution: Some(settings.into()),
        force_reparse: false,
        block_scope: None,
        archive_destination: None,
        deployment: DeploymentSummary::default(),
        revision: 0,
        active_run: None,
        stage: JobStage::Queued,
        block_reason: None,
        remote_confirmed: false,
    };
    save(workspace, &record)?;
    Ok(record)
}

pub fn sha256(path: &Path) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|error| format!("open PDF for hashing: {error}"))?;
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("read PDF for hashing: {error}"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

static STORE_WRITE: Mutex<()> = Mutex::new(());

pub fn save(workspace: &Path, record: &JobRecord) -> Result<(), String> {
    let _guard = STORE_WRITE
        .lock()
        .map_err(|_| "job store lock is poisoned".to_string())?;
    save_unlocked(workspace, record)
}

fn save_unlocked(workspace: &Path, record: &JobRecord) -> Result<(), String> {
    let directory = workspace.join("processing").join(&record.id);
    fs::create_dir_all(&directory).map_err(|error| format!("create job directory: {error}"))?;
    let path = directory.join("job.json");
    let temporary = directory.join(".job.json.tmp");
    let bytes =
        serde_json::to_vec_pretty(record).map_err(|error| format!("serialize job: {error}"))?;
    let mut file =
        fs::File::create(&temporary).map_err(|error| format!("create temporary job: {error}"))?;
    file.write_all(&bytes)
        .map_err(|error| format!("write temporary job: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("sync temporary job: {error}"))?;
    fs::rename(temporary, path).map_err(|error| format!("replace job: {error}"))
}

pub fn load_all(workspace: &Path) -> Result<Vec<JobRecord>, String> {
    let directory = workspace.join("processing");
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let entries =
        fs::read_dir(&directory).map_err(|error| format!("read task directory: {error}"))?;
    let mut records = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("read task entry: {error}"))?;
        if !entry.path().is_dir() {
            continue;
        }
        let id = entry
            .file_name()
            .into_string()
            .map_err(|_| "task directory name is not UTF-8".to_string())?;
        records.push(load(workspace, &id).map_err(|error| format!("task {id}: {error}"))?);
    }
    records.sort_by(|left: &JobRecord, right: &JobRecord| right.updated_at.cmp(&left.updated_at));
    Ok(records)
}

pub fn load(workspace: &Path, id: &str) -> Result<JobRecord, String> {
    if !matches!(
        Path::new(id).components().collect::<Vec<_>>().as_slice(),
        [std::path::Component::Normal(_)]
    ) {
        return Err("invalid task id".into());
    }
    let path = workspace.join("processing").join(id).join("job.json");
    let json = fs::read_to_string(path).map_err(|error| format!("read job: {error}"))?;
    let mut record: JobRecord =
        serde_json::from_str(&json).map_err(|error| format!("parse job: {error}"))?;
    if record.id != id {
        return Err("task id does not match its directory".into());
    }
    if !(1..=3).contains(&record.schema_version) {
        return Err("unsupported job schema version".into());
    }
    if record.schema_version < 3 {
        record.stage = match record.state {
            JobState::Preflight => JobStage::Preflight,
            JobState::Running if record.base_commit.is_some() => JobStage::Generating,
            JobState::Running => JobStage::Parsing,
            JobState::Verifying => JobStage::Publishing,
            JobState::Archiving => JobStage::Archiving,
            JobState::Succeeded => JobStage::Complete,
            _ => JobStage::Queued,
        };
        // Interpret legacy prose only at the migration boundary.
        record.block_reason = match (record.block_scope, record.phase.as_str()) {
            (
                Some(BlockScope::Repository),
                "Wiki has uncommitted changes" | "Agent stopped with uncommitted Wiki changes",
            ) => Some(BlockReason::RepositoryDirty),
            (Some(BlockScope::Repository), _) => Some(BlockReason::PublicationInvalid),
            (Some(BlockScope::Job), _) if record.task_commit.is_some() => {
                Some(BlockReason::RemoteUnavailable)
            }
            _ => None,
        };
        record.schema_version = 3;
    }
    Ok(record)
}

#[cfg(test)]
pub fn set_execution(
    workspace: &Path,
    id: &str,
    agent: &str,
    base_commit: &str,
) -> Result<JobRecord, String> {
    mutate(workspace, id, |record| {
        record.agent = Some(agent.into());
        record.base_commit = Some(base_commit.into());
        Ok(())
    })
}

pub fn set_deployment(
    workspace: &Path,
    id: &str,
    deployment: DeploymentSummary,
) -> Result<JobRecord, String> {
    mutate(workspace, id, |record| {
        record.deployment = deployment;
        Ok(())
    })
}

#[cfg(test)]
pub fn block(
    workspace: &Path,
    id: &str,
    message: &str,
    scope: BlockScope,
) -> Result<JobRecord, String> {
    mutate(workspace, id, |record| {
        record.transition(JobState::Blocked, message, 0)?;
        record.error = Some(message.into());
        record.block_scope = Some(scope);
        Ok(())
    })
}

#[cfg(test)]
pub fn retry(
    workspace: &Path,
    id: &str,
    settings: &AppSettings,
    force_reparse: bool,
) -> Result<JobRecord, String> {
    mutate(workspace, id, |record| {
        if record.active_run.is_some() {
            return Err("wait for the current execution to stop before retrying".into());
        }
        if !matches!(
            record.state,
            JobState::Blocked | JobState::Failed | JobState::Cancelled
        ) {
            return Err(format!("{} cannot be retried", record.state.as_str()));
        }
        if force_reparse && record.task_commit.is_some() {
            return Err("a published task can only resume verification or archival".into());
        }
        if record.execution.is_none() {
            record.execution = Some(settings.into());
        }
        if force_reparse {
            if let Some(execution) = &mut record.execution {
                execution.mineru_mode = settings.mineru_mode.clone();
            }
        }
        record.force_reparse = force_reparse;
        let next = if record.task_commit.is_some() {
            JobState::Verifying
        } else {
            JobState::Queued
        };
        record.transition(next, "Waiting for processor", 10)
    })
}

#[cfg(test)]
pub fn update(
    workspace: &Path,
    id: &str,
    next: JobState,
    phase: &str,
    progress: u8,
) -> Result<JobRecord, String> {
    mutate(workspace, id, |record| {
        record.transition(next, phase, progress)
    })
}

fn mutate(
    workspace: &Path,
    id: &str,
    update: impl FnOnce(&mut JobRecord) -> Result<(), String>,
) -> Result<JobRecord, String> {
    let _guard = STORE_WRITE
        .lock()
        .map_err(|_| "job store lock is poisoned".to_string())?;
    let mut record = load(workspace, id)?;
    update(&mut record)?;
    record.revision += 1;
    record.updated_at = Utc::now().to_rfc3339();
    save_unlocked(workspace, &record)?;
    Ok(record)
}

pub fn apply_recovery(
    workspace: &Path,
    expected: &JobRecord,
    decision: crate::recovery::Decision,
    restart: bool,
) -> Result<JobRecord, String> {
    mutate(workspace, &expected.id, |record| {
        if record.revision != expected.revision || record.active_run != expected.active_run {
            return Err("job changed while recovery was inspecting it".into());
        }
        crate::recovery::apply(record, decision);
        if restart {
            record.active_run = None;
        }
        Ok(())
    })
}

pub fn apply_retry(
    workspace: &Path,
    expected: &JobRecord,
    settings: &AppSettings,
    force_reparse: bool,
    decision: crate::recovery::Decision,
) -> Result<JobRecord, String> {
    mutate(workspace, &expected.id, |record| {
        if record.active_run.is_some() || record.revision != expected.revision {
            return Err("job changed or still has an active execution".into());
        }
        if !matches!(
            record.state,
            JobState::Blocked | JobState::Failed | JobState::Cancelled
        ) {
            return Err("this job cannot be retried".into());
        }
        if force_reparse
            && (record.task_commit.is_some()
                || matches!(
                    &decision,
                    crate::recovery::Decision::Verify(_)
                        | crate::recovery::Decision::Archive(_)
                        | crate::recovery::Decision::Block {
                            commit: Some(_),
                            ..
                        }
                ))
        {
            return Err(
                "a task commit already exists; resume publication instead of reparsing".into(),
            );
        }
        if record.execution.is_none() {
            record.execution = Some(settings.into());
        }
        if force_reparse {
            if let Some(execution) = &mut record.execution {
                execution.mineru_mode = settings.mineru_mode.clone();
            }
        }
        record.force_reparse = force_reparse;
        crate::recovery::apply(record, decision);
        Ok(())
    })
}

pub fn fail(workspace: &Path, id: &str, error: &str) -> Result<JobRecord, String> {
    mutate(workspace, id, |record| {
        record.transition(JobState::Failed, "Failed", record.progress)?;
        record.error = Some(error.into());
        Ok(())
    })
}

pub fn claim(workspace: &Path, id: &str, run_id: &str) -> Result<JobRecord, String> {
    mutate(workspace, id, |record| {
        if record.active_run.is_some()
            || !matches!(
                record.state,
                JobState::Queued | JobState::Verifying | JobState::Archiving
            )
        {
            return Err("job is not available for execution".into());
        }
        record.active_run = Some(run_id.into());
        Ok(())
    })
}

pub fn request_cancel(workspace: &Path, id: &str) -> Result<JobRecord, String> {
    mutate(workspace, id, |record| {
        if !record
            .state
            .allowed_actions()
            .iter()
            .any(|action| action == "cancel")
        {
            return Err("this job cannot be cancelled at its current stage".into());
        }
        let next = if record.active_run.is_some() {
            JobState::Cancelling
        } else {
            JobState::Cancelled
        };
        record.transition(
            next,
            if next == JobState::Cancelling {
                "Stopping current execution"
            } else {
                "Cancelled"
            },
            record.progress,
        )
    })
}

pub fn finish_execution(
    workspace: &Path,
    id: &str,
    run_id: &str,
    error: Option<&str>,
) -> Result<JobRecord, String> {
    mutate(workspace, id, |record| {
        if record.active_run.as_deref() != Some(run_id) {
            return Err("execution no longer owns this job".into());
        }
        if record.state == JobState::Cancelling {
            record.transition(JobState::Cancelled, "Cancelled", record.progress)?;
        } else if !matches!(
            record.state,
            JobState::Succeeded | JobState::Blocked | JobState::Failed | JobState::Cancelled
        ) {
            record.transition(JobState::Failed, "Execution stopped", record.progress)?;
            record.error = Some(
                error
                    .unwrap_or("execution ended before reaching a terminal state")
                    .into(),
            );
        }
        record.active_run = None;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn legacy_records_migrate_once_and_invalid_journals_are_reported() {
        let root =
            std::env::temp_dir().join(format!("cognitio-job-migration-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("inbox")).unwrap();
        let source = root.join("inbox/paper.pdf");
        fs::write(&source, b"%PDF-1.4\n").unwrap();
        let record = create(&root, &source, &AppSettings::default()).unwrap();
        let path = root.join("processing").join(&record.id).join("job.json");
        let mut legacy = serde_json::to_value(&record).unwrap();
        let fields = legacy.as_object_mut().unwrap();
        fields.insert("schemaVersion".into(), 2.into());
        fields.insert("state".into(), "blocked".into());
        fields.insert("blockScope".into(), "job".into());
        fields.insert("publishedCommit".into(), "task-sha".into());
        for key in [
            "taskCommit",
            "stage",
            "blockReason",
            "revision",
            "activeRun",
            "remoteConfirmed",
        ] {
            fields.remove(key);
        }
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let mut migrated = load(&root, &record.id).unwrap();
        assert_eq!(migrated.schema_version, 3);
        assert_eq!(migrated.task_commit.as_deref(), Some("task-sha"));
        assert_eq!(migrated.block_reason, Some(BlockReason::RemoteUnavailable));
        assert!(!migrated.remote_confirmed);
        migrated.phase = "Different display text".into();
        save(&root, &migrated).unwrap();
        assert_eq!(
            load(&root, &record.id).unwrap().block_reason,
            migrated.block_reason
        );
        assert!(load(&root, "../outside").is_err());
        migrated.schema_version = 99;
        save(&root, &migrated).unwrap();
        assert!(load_all(&root)
            .unwrap_err()
            .contains("unsupported job schema"));
        fs::write(&path, b"{broken").unwrap();
        assert!(load_all(&root).unwrap_err().contains(&record.id));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn state_machine_rejects_skipped_phases_and_allows_retry() {
        assert!(JobState::Queued.can_transition_to(JobState::Preflight));
        assert!(!JobState::Queued.can_transition_to(JobState::Succeeded));
        assert!(JobState::Failed.can_transition_to(JobState::Queued));
        assert!(!JobState::Succeeded.can_transition_to(JobState::Queued));
        assert!(JobState::Verifying.can_transition_to(JobState::Archiving));
        assert!(JobState::Archiving.can_transition_to(JobState::Succeeded));
        assert_eq!(JobState::Blocked.allowed_actions(), ["retry", "reparse"]);
        assert!(JobState::Verifying.allowed_actions().is_empty());
        assert!(JobState::Succeeded.allowed_actions().is_empty());
    }

    #[test]
    fn running_job_persists_repository_block_and_can_retry() {
        let root = std::env::temp_dir().join(format!(
            "cognitio-running-block-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let settings = AppSettings::default();
        for (index, (message, base_commit)) in [
            ("Wiki has uncommitted changes", None),
            (
                "Agent stopped with uncommitted Wiki changes",
                Some("abc123"),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let workspace = root.join(index.to_string());
            fs::create_dir_all(workspace.join("inbox")).expect("inbox");
            let source = workspace.join("inbox/paper.pdf");
            fs::write(&source, b"%PDF-1.4\n").expect("PDF");
            let record = create(&workspace, &source, &settings).expect("job");
            update(&workspace, &record.id, JobState::Preflight, "Preflight", 15)
                .expect("preflight");
            update(&workspace, &record.id, JobState::Running, "Running", 25).expect("running");
            if let Some(base) = base_commit {
                set_execution(&workspace, &record.id, "codex", base).expect("execution metadata");
            }

            block(&workspace, &record.id, message, BlockScope::Repository)
                .expect("block running job");

            let blocked = load(&workspace, &record.id).expect("reload blocked job");
            assert_eq!(blocked.state, JobState::Blocked);
            assert_eq!(blocked.phase, message);
            assert_eq!(blocked.error.as_deref(), Some(message));
            assert_eq!(blocked.block_scope, Some(BlockScope::Repository));
            assert_eq!(blocked.base_commit.as_deref(), base_commit);
            assert_eq!(
                JobSummary::from(&blocked).allowed_actions,
                ["retry", "reparse"]
            );

            retry(&workspace, &record.id, &settings, false).expect("retry blocked job");
            let retried = load(&workspace, &record.id).expect("reload retried job");
            assert_eq!(retried.state, JobState::Queued);
            assert_eq!(retried.error, None);
            assert_eq!(retried.block_scope, None);
            assert_eq!(retried.base_commit, None);
        }
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn publication_lookup_requires_the_exact_job_trailer() {
        let root = std::env::temp_dir().join(format!(
            "cognitio-trailer-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let workspace = root.join("workspace");
        let wiki = workspace.join("wiki");
        for directory in ["inbox", "processing", "wiki/content/papers"] {
            fs::create_dir_all(workspace.join(directory)).expect("workspace directory");
        }
        git(&wiki, &["init", "--initial-branch", "main"]);
        git(&wiki, &["config", "user.name", "Cognitio Test"]);
        git(&wiki, &["config", "user.email", "test@cognitio.local"]);
        fs::write(wiki.join("content/papers/base.md"), "base\n").expect("base file");
        git(&wiki, &["add", "."]);
        git(&wiki, &["commit", "-m", "Base"]);
        let base = git_output(&wiki, &["rev-parse", "HEAD"]);
        let source = workspace.join("inbox/paper.pdf");
        fs::write(&source, b"%PDF-1.4\n").expect("PDF");
        let mut record = create(&workspace, &source, &AppSettings::default()).expect("job");
        record.base_commit = Some(base.clone());

        fs::write(wiki.join("content/papers/unrelated.md"), "unrelated\n").expect("unrelated file");
        git(&wiki, &["add", "."]);
        git(&wiki, &["commit", "-m", "Unrelated commit"]);
        assert_eq!(
            crate::publication::find_task_commit(&wiki, &record)
                .await
                .expect("lookup"),
            None
        );

        fs::write(wiki.join("content/papers/paper.md"), "paper\n").expect("paper file");
        git(&wiki, &["add", "."]);
        let trailer = format!("Cognitio-Job: {}", record.id);
        git(&wiki, &["commit", "-m", "Add paper", "-m", &trailer]);
        let task_commit = crate::publication::find_task_commit(&wiki, &record)
            .await
            .expect("lookup")
            .expect("task commit");

        record.base_commit = None;
        assert!(crate::publication::find_task_commit(&wiki, &record)
            .await
            .expect("history lookup")
            .is_some());
        record.task_commit = Some(task_commit);
        assert!(crate::publication::find_task_commit(&wiki, &record)
            .await
            .expect("recorded publication lookup")
            .is_some());
        record.task_commit = Some(base);
        assert!(crate::publication::find_task_commit(&wiki, &record)
            .await
            .is_err());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn retry_clears_previous_publication_metadata() {
        let mut record = JobRecord {
            schema_version: 2,
            id: "job".into(),
            filename: "paper.pdf".into(),
            source_path: "inbox/paper.pdf".into(),
            input_path: "processing/job/paper.pdf".into(),
            sha256: "hash".into(),
            state: JobState::Blocked,
            phase: "Agent did not create a commit".into(),
            progress: 0,
            agent: Some("codex".into()),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            error: Some("stale error".into()),
            base_commit: Some("abc123".into()),
            task_commit: None,
            execution: Some((&AppSettings::default()).into()),
            force_reparse: false,
            block_scope: Some(BlockScope::Job),
            archive_destination: None,
            deployment: DeploymentSummary::default(),
            revision: 0,
            active_run: None,
            stage: crate::jobs::JobStage::Queued,
            block_reason: None,
            remote_confirmed: false,
        };

        record
            .transition(JobState::Queued, "Waiting for processor", 10)
            .expect("retry transition");

        assert_eq!(record.agent, None);
        assert_eq!(record.error, None);
        assert_eq!(record.base_commit, None);
    }

    #[tokio::test]
    async fn published_interrupted_job_resumes_at_archiving() {
        let root = std::env::temp_dir().join(format!(
            "llmwiki-recovery-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let workspace = root.join("workspace");
        for directory in ["inbox", "processing", "done", "failed", "wiki"] {
            fs::create_dir_all(workspace.join(directory)).expect("workspace directory");
        }
        let wiki = workspace.join("wiki");
        let remote = root.join("remote.git");
        git(&root, &["init", "--bare", remote.to_str().expect("remote")]);
        git(&wiki, &["init", "--initial-branch", "main"]);
        git(&wiki, &["config", "user.name", "LLMWiki Test"]);
        git(&wiki, &["config", "user.email", "test@llmwiki.local"]);
        fs::write(wiki.join("index.md"), "base\n").expect("base file");
        git(&wiki, &["add", "."]);
        git(&wiki, &["commit", "-m", "Base"]);
        git(
            &wiki,
            &["remote", "add", "origin", remote.to_str().expect("remote")],
        );
        git(&wiki, &["push", "-u", "origin", "main"]);
        let base = git_output(&wiki, &["rev-parse", "HEAD"]);

        let source = workspace.join("inbox/paper.pdf");
        fs::write(&source, b"%PDF-1.4\n").expect("PDF");
        let record = create(&workspace, &source, &AppSettings::default()).expect("job");
        update(&workspace, &record.id, JobState::Preflight, "Preflight", 15).expect("preflight");
        set_execution(&workspace, &record.id, "codex", &base).expect("execution metadata");
        update(&workspace, &record.id, JobState::Running, "Running", 35).expect("running");
        fs::create_dir_all(wiki.join("content/papers")).expect("papers directory");
        fs::write(wiki.join("content/papers/paper.md"), "published\n").expect("published file");
        git(&wiki, &["add", "."]);
        let trailer = format!("Cognitio-Job: {}", record.id);
        git(&wiki, &["commit", "-m", "Publish paper", "-m", &trailer]);
        git(&wiki, &["push"]);

        let recovered = crate::recovery::recover_all(&workspace).await.unwrap();
        let job = recovered
            .iter()
            .find(|candidate| candidate.id == record.id)
            .expect("recovered job");
        assert_eq!(job.state, JobState::Archiving);
        assert!(job.remote_confirmed);
        fs::remove_dir_all(root).expect("cleanup");
    }

    fn git(cwd: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_output(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("run git");
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().into()
    }
}
