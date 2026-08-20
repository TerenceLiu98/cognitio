use std::{
    fs,
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
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
                    Self::Preflight | Self::Failed | Self::Cancelled
                )
                | (
                    Self::Preflight,
                    Self::Running | Self::Blocked | Self::Failed | Self::Cancelled
                )
                | (
                    Self::Running,
                    Self::Verifying | Self::Failed | Self::Cancelled
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
            Self::Verifying | Self::Archiving | Self::Succeeded => Vec::new(),
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
    #[serde(default)]
    pub published_commit: Option<String>,
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
            self.published_commit = None;
            self.block_scope = None;
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
        if record.published_commit.is_some() {
            allowed_actions.retain(|action| action != "reparse");
        }
        Self {
            id: record.id.clone(),
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
    let record = JobRecord {
        schema_version: 2,
        id,
        filename: filename.into(),
        source_path: source.to_path_buf(),
        input_path,
        sha256: sha256(source)?,
        state: JobState::Queued,
        phase: "Waiting for processor".into(),
        progress: 10,
        agent: None,
        created_at: now.clone(),
        updated_at: now,
        error: None,
        base_commit: None,
        published_commit: None,
        execution: Some(settings.into()),
        force_reparse: false,
        block_scope: None,
        archive_destination: None,
        deployment: DeploymentSummary::default(),
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

pub fn save(workspace: &Path, record: &JobRecord) -> Result<(), String> {
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

pub fn load_all(workspace: &Path) -> Vec<JobRecord> {
    let Ok(entries) = fs::read_dir(workspace.join("processing")) else {
        return Vec::new();
    };
    let mut records: Vec<_> = entries
        .flatten()
        .filter_map(|entry| fs::read_to_string(entry.path().join("job.json")).ok())
        .filter_map(|json| serde_json::from_str(&json).ok())
        .collect();
    records.sort_by(|left: &JobRecord, right: &JobRecord| right.updated_at.cmp(&left.updated_at));
    records
}

pub fn load(workspace: &Path, id: &str) -> Result<JobRecord, String> {
    let path = workspace.join("processing").join(id).join("job.json");
    let json = fs::read_to_string(path).map_err(|error| format!("read job: {error}"))?;
    serde_json::from_str(&json).map_err(|error| format!("parse job: {error}"))
}

pub fn recover(workspace: &Path) -> Vec<JobRecord> {
    let wiki = workspace.join("wiki");
    let wiki_clean =
        git_value(&wiki, &["status", "--porcelain"]).is_some_and(|value| value.is_empty());
    let mut records = load_all(workspace);
    for record in &mut records {
        record.schema_version = 2;
        if record.execution.is_none()
            && !matches!(
                record.state,
                JobState::Succeeded | JobState::Failed | JobState::Cancelled
            )
        {
            record.state = JobState::Blocked;
            record.phase = "Legacy job requires retry".into();
            record.error = Some("Retry this job to capture the current processing settings".into());
            record.block_scope = Some(BlockScope::Job);
            record.updated_at = Utc::now().to_rfc3339();
            let _ = save(workspace, record);
            continue;
        }
        if matches!(
            record.state,
            JobState::Succeeded | JobState::Failed | JobState::Cancelled
        ) {
            let _ = save(workspace, record);
            continue;
        }
        if !wiki_clean {
            record.state = JobState::Blocked;
            record.phase = "Wiki has uncommitted changes".into();
            record.error = Some("Clean or commit the Wiki worktree before retrying".into());
            record.block_scope = Some(BlockScope::Repository);
            record.updated_at = Utc::now().to_rfc3339();
            let _ = save(workspace, record);
            continue;
        }

        let matching = match find_job_commit(&wiki, record) {
            Ok(value) => value,
            Err(error) => {
                record.state = JobState::Blocked;
                record.phase = "Git publication requires review".into();
                record.error = Some(error);
                record.block_scope = Some(BlockScope::Repository);
                record.updated_at = Utc::now().to_rfc3339();
                let _ = save(workspace, record);
                continue;
            }
        };
        if let Some(commit) = matching {
            let published = commit_on_upstream(&wiki, &commit);
            record.published_commit = Some(commit);
            record.state = if published {
                JobState::Archiving
            } else {
                JobState::Blocked
            };
            record.phase = if published {
                "Resuming source archival"
            } else {
                "Task commit exists but is not pushed"
            }
            .into();
            record.error = (!published).then(|| "Retry to resume Git push".into());
            record.block_scope = (!published).then_some(BlockScope::Job);
            record.updated_at = Utc::now().to_rfc3339();
            let _ = save(workspace, record);
            continue;
        }

        match record.state {
            JobState::Detected | JobState::Stabilizing => {
                record.state = JobState::Queued;
                record.phase = "Recovered after restart".into();
                record.updated_at = Utc::now().to_rfc3339();
                let _ = save(workspace, record);
            }
            JobState::Preflight | JobState::Running | JobState::Verifying => {
                record.state = JobState::Queued;
                record.phase = "Recovered after restart".into();
                record.base_commit = None;
                record.agent = None;
                record.updated_at = Utc::now().to_rfc3339();
                let _ = save(workspace, record);
            }
            _ => {}
        }
    }
    records
}

pub fn find_job_commit(wiki: &Path, record: &JobRecord) -> Result<Option<String>, String> {
    let Some(base) = record.base_commit.as_deref() else {
        return Ok(record.published_commit.clone());
    };
    let commits = git_value(wiki, &["log", "--format=%H", &format!("{base}..HEAD")])
        .ok_or_else(|| "read commits created after job baseline".to_string())?;
    let trailer = format!("Cognitio-Job: {}", record.id);
    let mut matching = Vec::new();
    for commit in commits.lines().filter(|line| !line.trim().is_empty()) {
        let body = git_value(wiki, &["show", "-s", "--format=%B", commit])
            .ok_or_else(|| format!("read task commit {commit}"))?;
        if body.lines().any(|line| line.trim() == trailer) {
            matching.push(commit.to_owned());
        }
    }
    match matching.as_slice() {
        [] => Ok(None),
        [commit] => Ok(Some(commit.clone())),
        _ => Err("multiple commits claim the same Cognitio job id".into()),
    }
}

pub fn commit_on_upstream(wiki: &Path, commit: &str) -> bool {
    std::process::Command::new("git")
        .args(["merge-base", "--is-ancestor", commit, "@{u}"])
        .current_dir(wiki)
        .status()
        .is_ok_and(|status| status.success())
}

pub fn set_execution(
    workspace: &Path,
    id: &str,
    agent: &str,
    base_commit: &str,
) -> Result<JobRecord, String> {
    let path = workspace.join("processing").join(id).join("job.json");
    let json = fs::read_to_string(path).map_err(|error| format!("read job: {error}"))?;
    let mut record: JobRecord =
        serde_json::from_str(&json).map_err(|error| format!("parse job: {error}"))?;
    record.agent = Some(agent.into());
    record.base_commit = Some(base_commit.into());
    record.updated_at = Utc::now().to_rfc3339();
    save(workspace, &record)?;
    Ok(record)
}

pub fn set_published_commit(workspace: &Path, id: &str, commit: &str) -> Result<JobRecord, String> {
    mutate(workspace, id, |record| {
        record.published_commit = Some(commit.into());
        Ok(())
    })
}

pub fn set_archive_destination(
    workspace: &Path,
    id: &str,
    destination: Option<PathBuf>,
) -> Result<JobRecord, String> {
    mutate(workspace, id, |record| {
        record.archive_destination = destination;
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

pub fn clear_force_reparse(workspace: &Path, id: &str) -> Result<JobRecord, String> {
    mutate(workspace, id, |record| {
        record.force_reparse = false;
        Ok(())
    })
}

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

pub fn retry(
    workspace: &Path,
    id: &str,
    settings: &AppSettings,
    force_reparse: bool,
) -> Result<JobRecord, String> {
    mutate(workspace, id, |record| {
        if !matches!(
            record.state,
            JobState::Blocked | JobState::Failed | JobState::Cancelled
        ) {
            return Err(format!("{} cannot be retried", record.state.as_str()));
        }
        if force_reparse && record.published_commit.is_some() {
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
        let next = if record.published_commit.is_some() {
            JobState::Verifying
        } else {
            JobState::Queued
        };
        record.transition(next, "Waiting for processor", 10)
    })
}

fn git_value(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().into())
}

pub fn update(
    workspace: &Path,
    id: &str,
    next: JobState,
    phase: &str,
    progress: u8,
) -> Result<JobRecord, String> {
    let path = workspace.join("processing").join(id).join("job.json");
    let json = fs::read_to_string(path).map_err(|error| format!("read job: {error}"))?;
    let mut record: JobRecord =
        serde_json::from_str(&json).map_err(|error| format!("parse job: {error}"))?;
    record.transition(next, phase, progress)?;
    save(workspace, &record)?;
    Ok(record)
}

fn mutate(
    workspace: &Path,
    id: &str,
    update: impl FnOnce(&mut JobRecord) -> Result<(), String>,
) -> Result<JobRecord, String> {
    let path = workspace.join("processing").join(id).join("job.json");
    let json = fs::read_to_string(path).map_err(|error| format!("read job: {error}"))?;
    let mut record: JobRecord =
        serde_json::from_str(&json).map_err(|error| format!("parse job: {error}"))?;
    update(&mut record)?;
    record.updated_at = Utc::now().to_rfc3339();
    save(workspace, &record)?;
    Ok(record)
}

pub fn update_phase(
    workspace: &Path,
    id: &str,
    phase: &str,
    progress: u8,
) -> Result<JobRecord, String> {
    let path = workspace.join("processing").join(id).join("job.json");
    let json = fs::read_to_string(path).map_err(|error| format!("read job: {error}"))?;
    let mut record: JobRecord =
        serde_json::from_str(&json).map_err(|error| format!("parse job: {error}"))?;
    record.phase = phase.into();
    record.progress = progress.min(100);
    record.updated_at = Utc::now().to_rfc3339();
    save(workspace, &record)?;
    Ok(record)
}

pub fn fail(workspace: &Path, id: &str, error: &str) -> Result<JobRecord, String> {
    let path = workspace.join("processing").join(id).join("job.json");
    let json = fs::read_to_string(path).map_err(|read_error| format!("read job: {read_error}"))?;
    let mut record: JobRecord =
        serde_json::from_str(&json).map_err(|parse_error| format!("parse job: {parse_error}"))?;
    record.transition(JobState::Failed, "Failed", record.progress)?;
    record.error = Some(error.into());
    save(workspace, &record)?;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

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
    fn publication_lookup_requires_the_exact_job_trailer() {
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
        record.base_commit = Some(base);

        fs::write(wiki.join("content/papers/unrelated.md"), "unrelated\n").expect("unrelated file");
        git(&wiki, &["add", "."]);
        git(&wiki, &["commit", "-m", "Unrelated commit"]);
        assert_eq!(find_job_commit(&wiki, &record).expect("lookup"), None);

        fs::write(wiki.join("content/papers/paper.md"), "paper\n").expect("paper file");
        git(&wiki, &["add", "."]);
        let trailer = format!("Cognitio-Job: {}", record.id);
        git(&wiki, &["commit", "-m", "Add paper", "-m", &trailer]);
        assert!(find_job_commit(&wiki, &record).expect("lookup").is_some());
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
            published_commit: None,
            execution: Some((&AppSettings::default()).into()),
            force_reparse: false,
            block_scope: Some(BlockScope::Job),
            archive_destination: None,
            deployment: DeploymentSummary::default(),
        };

        record
            .transition(JobState::Queued, "Waiting for processor", 10)
            .expect("retry transition");

        assert_eq!(record.agent, None);
        assert_eq!(record.error, None);
        assert_eq!(record.base_commit, None);
    }

    #[test]
    fn published_interrupted_job_resumes_at_archiving() {
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

        let recovered = recover(&workspace);
        let job = recovered
            .iter()
            .find(|candidate| candidate.id == record.id)
            .expect("recovered job");
        assert_eq!(job.state, JobState::Archiving);
        assert_eq!(job.phase, "Resuming source archival");
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
