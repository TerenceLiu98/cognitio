use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::models::JobSummary;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Detected,
    Stabilizing,
    Queued,
    Preflight,
    Running,
    Verifying,
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
                    Self::Succeeded | Self::Blocked | Self::Failed
                )
                | (Self::Blocked | Self::Failed | Self::Cancelled, Self::Queued)
        )
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
        }
        self.updated_at = Utc::now().to_rfc3339();
        Ok(())
    }
}

impl From<&JobRecord> for JobSummary {
    fn from(record: &JobRecord) -> Self {
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
        }
    }
}

pub fn create(workspace: &Path, source: &Path) -> Result<JobRecord, String> {
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
        schema_version: 1,
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
    };
    save(workspace, &record)?;
    Ok(record)
}

pub fn sha256(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| format!("read PDF for hashing: {error}"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
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

pub fn recover(workspace: &Path) -> Vec<JobRecord> {
    let wiki = workspace.join("wiki");
    let wiki_clean =
        git_value(&wiki, &["status", "--porcelain"]).is_some_and(|value| value.is_empty());
    let head = git_value(&wiki, &["rev-parse", "HEAD"]);
    let upstream = git_value(&wiki, &["rev-parse", "@{u}"]);
    let published = head.is_some() && head == upstream;
    let mut records = load_all(workspace);
    for record in &mut records {
        match record.state {
            JobState::Detected | JobState::Stabilizing => {
                record.state = if wiki_clean {
                    JobState::Queued
                } else {
                    JobState::Blocked
                };
                record.phase = if wiki_clean {
                    "Recovered after restart"
                } else {
                    "Wiki changed during interrupted job"
                }
                .into();
                record.updated_at = Utc::now().to_rfc3339();
                let _ = save(workspace, record);
            }
            JobState::Preflight | JobState::Running => {
                let commit_created = record
                    .base_commit
                    .as_ref()
                    .zip(head.as_ref())
                    .is_some_and(|(base, current)| base != current);
                record.state = if !wiki_clean {
                    JobState::Blocked
                } else if commit_created && published {
                    JobState::Verifying
                } else if commit_created {
                    JobState::Blocked
                } else {
                    JobState::Queued
                };
                record.phase = match record.state {
                    JobState::Verifying => "Resuming publication verification",
                    JobState::Blocked => "Interrupted Git publication requires review",
                    _ => "Recovered after restart",
                }
                .into();
                record.updated_at = Utc::now().to_rfc3339();
                let _ = save(workspace, record);
            }
            JobState::Verifying => {
                record.state = if wiki_clean && published {
                    JobState::Verifying
                } else {
                    JobState::Blocked
                };
                record.phase = match record.state {
                    JobState::Verifying => "Resuming publication verification",
                    _ => "Publication requires review",
                }
                .into();
                record.updated_at = Utc::now().to_rfc3339();
                let _ = save(workspace, record);
            }
            _ => {}
        }
    }
    records
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
    }

    #[test]
    fn retry_clears_previous_execution_metadata() {
        let mut record = JobRecord {
            schema_version: 1,
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
        };

        record
            .transition(JobState::Queued, "Waiting for processor", 10)
            .expect("retry transition");

        assert_eq!(record.agent, None);
        assert_eq!(record.error, None);
        assert_eq!(record.base_commit, None);
    }

    #[test]
    fn published_interrupted_job_resumes_at_verification() {
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
        let record = create(&workspace, &source).expect("job");
        update(&workspace, &record.id, JobState::Preflight, "Preflight", 15).expect("preflight");
        set_execution(&workspace, &record.id, "codex", &base).expect("execution metadata");
        update(&workspace, &record.id, JobState::Running, "Running", 35).expect("running");
        fs::write(wiki.join("index.md"), "published\n").expect("published file");
        git(&wiki, &["add", "."]);
        git(&wiki, &["commit", "-m", "Publish paper"]);
        git(&wiki, &["push"]);

        let recovered = recover(&workspace);
        let job = recovered
            .iter()
            .find(|candidate| candidate.id == record.id)
            .expect("recovered job");
        assert_eq!(job.state, JobState::Verifying);
        assert_eq!(job.phase, "Resuming publication verification");
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
