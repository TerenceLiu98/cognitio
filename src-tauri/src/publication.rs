use std::{fs, path::Path};

use crate::{
    git::{output as git_output, output_bytes as git_output_bytes, remote_contains},
    jobs::{self, BlockReason, BlockScope, JobEvent, JobRecord},
};

const LIBRARY_HOME_LINK: &str = "[[index|Library home]]";

pub async fn find_task_commit(wiki: &Path, record: &JobRecord) -> Result<Option<String>, String> {
    let trailer = format!("Cognitio-Job: {}", record.id);
    if let Some(commit) = &record.task_commit {
        let body = git_output(wiki, &["show", "-s", "--format=%B", commit]).await?;
        return if body.lines().any(|line| line.trim() == trailer) {
            Ok(Some(commit.clone()))
        } else {
            Err("recorded task commit is missing the Cognitio-Job trailer".into())
        };
    }
    let range = record
        .base_commit
        .as_deref()
        .map_or_else(|| "HEAD".to_string(), |base| format!("{base}..HEAD"));
    let commits = git_output(wiki, &["log", "--format=%H", &range]).await?;
    let mut matching = Vec::new();
    for commit in commits.lines().filter(|line| !line.trim().is_empty()) {
        let body = git_output(wiki, &["show", "-s", "--format=%B", commit]).await?;
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

#[derive(Debug)]
pub enum PublicationOutcome {
    Published(JobRecord),
    Blocked(JobRecord),
}

pub async fn validate_task_commit(
    wiki: &Path,
    record: &JobRecord,
    commit: &str,
) -> Result<(), String> {
    let base = record
        .base_commit
        .as_deref()
        .ok_or("job has no recorded baseline commit")?;
    if !crate::git::success(wiki, &["merge-base", "--is-ancestor", base, commit]).await?
        || git_output(wiki, &["rev-list", "--count", &format!("{base}..{commit}")]).await? != "1"
    {
        return Err("the task commit must be exactly one commit after its baseline".into());
    }
    let changes = git_output_bytes(
        wiki,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-status",
            "--no-renames",
            "-z",
            "-r",
            commit,
        ],
    )
    .await?;
    let parts: Vec<_> = changes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() || parts.len() % 2 != 0 {
        return Err("task commit has no valid Wiki changes".into());
    }
    for change in parts.chunks_exact(2) {
        let path =
            std::str::from_utf8(change[1]).map_err(|_| "task commit contains a non-UTF-8 path")?;
        if !matches!(change[0], b"A" | b"M") || !allowed_wiki_path(path) {
            return Err(format!("task commit contains a disallowed change: {path}"));
        }
    }
    Ok(())
}

async fn apply(workspace: &Path, record: &JobRecord, event: JobEvent) -> Result<JobRecord, String> {
    let workspace = workspace.to_path_buf();
    let record = record.clone();
    tokio::task::spawn_blocking(move || jobs::apply_event(&workspace, &record, event))
        .await
        .map_err(|error| format!("join publication update: {error}"))?
}

async fn block(
    workspace: &Path,
    record: &JobRecord,
    message: &str,
    scope: BlockScope,
    reason: BlockReason,
) -> Result<PublicationOutcome, String> {
    let record = apply(
        workspace,
        record,
        JobEvent::Blocked {
            reason,
            scope,
            message: message.into(),
        },
    )
    .await?;
    Ok(PublicationOutcome::Blocked(record))
}

pub async fn verify(
    record: &JobRecord,
    workspace: &Path,
    base_commit: Option<&str>,
) -> Result<PublicationOutcome, String> {
    let wiki = workspace.join("wiki");
    if !git_output(&wiki, &["status", "--porcelain"])
        .await?
        .is_empty()
    {
        return block(
            workspace,
            record,
            "Wiki is not clean after agent completion",
            BlockScope::Repository,
            BlockReason::RepositoryDirty,
        )
        .await;
    }

    if record.task_commit.is_none() {
        let Some(base_commit) = base_commit else {
            return block(
                workspace,
                record,
                "Job has no recorded baseline commit",
                BlockScope::Job,
                BlockReason::PublicationInvalid,
            )
            .await;
        };
        let range = format!("{base_commit}..HEAD");
        let commit_count = git_output(&wiki, &["rev-list", "--count", &range])
            .await?
            .parse::<u32>()
            .map_err(|error| format!("parse task commit count: {error}"))?;
        if commit_count == 0 {
            return block(
                workspace,
                record,
                "Agent did not create a commit",
                BlockScope::Job,
                BlockReason::PublicationInvalid,
            )
            .await;
        }
        if commit_count != 1 {
            return block(
                workspace,
                record,
                "The job must produce exactly one commit",
                BlockScope::Repository,
                BlockReason::PublicationInvalid,
            )
            .await;
        }
    }

    let commit = find_task_commit(&wiki, record).await?;
    let Some(commit) = commit else {
        return block(
            workspace,
            record,
            "Task commit is missing the Cognitio-Job trailer",
            BlockScope::Repository,
            BlockReason::PublicationInvalid,
        )
        .await;
    };
    if let Err(error) = validate_task_commit(&wiki, record, &commit).await {
        return block(
            workspace,
            record,
            &error,
            BlockScope::Repository,
            BlockReason::PublicationInvalid,
        )
        .await;
    }

    apply(workspace, record, JobEvent::TaskCommit(commit.clone())).await?;
    if !match remote_contains(&wiki, &commit).await {
        Ok(confirmed) => confirmed,
        Err(error) => {
            return block(
                workspace,
                record,
                &format!("Remote publication has not been confirmed: {error}"),
                BlockScope::Job,
                BlockReason::RemoteUnavailable,
            )
            .await
        }
    } {
        if let Err(error) = git_output(&wiki, &["push"]).await {
            return block(
                workspace,
                record,
                &format!("Task commit exists but push failed: {error}"),
                BlockScope::Job,
                BlockReason::RemoteUnavailable,
            )
            .await;
        }
    }
    if !match remote_contains(&wiki, &commit).await {
        Ok(confirmed) => confirmed,
        Err(error) => {
            return block(
                workspace,
                record,
                &format!("Remote publication has not been confirmed: {error}"),
                BlockScope::Job,
                BlockReason::RemoteUnavailable,
            )
            .await
        }
    } {
        return block(
            workspace,
            record,
            "Remote branch does not contain the task commit",
            BlockScope::Job,
            BlockReason::RemoteUnavailable,
        )
        .await;
    }

    let published = apply(workspace, record, JobEvent::PublicationConfirmed(commit)).await?;
    Ok(PublicationOutcome::Published(published))
}

fn allowed_wiki_path(path: &str) -> bool {
    if Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
    {
        return false;
    }
    path == "references.bib"
        || ["content/papers/", "content/concepts/", "assets/papers/"]
            .iter()
            .any(|prefix| path.starts_with(prefix))
}

pub(crate) async fn commit_agent_changes(
    wiki: &Path,
    job_id: &str,
    base_commit: &str,
) -> Result<(), String> {
    let range = format!("{base_commit}..HEAD");
    let commit_count = git_output(wiki, &["rev-list", "--count", &range])
        .await?
        .parse::<u32>()
        .map_err(|error| format!("parse task commit count: {error}"))?;
    if commit_count != 0 {
        return Ok(());
    }

    let status = git_output_bytes(
        wiki,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    .await?;
    let paths = agent_change_paths(&status)?;
    if paths.is_empty() {
        return Ok(());
    }
    ensure_paper_library_links(wiki, &paths)?;

    let mut add_args = vec!["add", "--"];
    add_args.extend(paths.iter().map(String::as_str));
    git_output(wiki, &add_args).await?;
    git_output(
        wiki,
        &[
            "-c",
            "core.whitespace=trailing-space,-blank-at-eof",
            "diff",
            "--cached",
            "--check",
        ],
    )
    .await?;

    let trailer = format!("Cognitio-Job: {job_id}");
    git_output(
        wiki,
        &["commit", "-m", "Add paper knowledge pages", "-m", &trailer],
    )
    .await?;
    Ok(())
}

fn ensure_paper_library_links(wiki: &Path, paths: &[String]) -> Result<(), String> {
    for relative in paths
        .iter()
        .filter(|path| path.starts_with("content/papers/") && path.ends_with(".md"))
    {
        let path = wiki.join(relative);
        let mut contents = fs::read_to_string(&path)
            .map_err(|error| format!("read Paper page {relative}: {error}"))?;
        if contents.contains("[[index]]") || contents.contains("[[index|") {
            continue;
        }
        if !contents.ends_with('\n') {
            contents.push('\n');
        }
        contents.push('\n');
        contents.push_str(LIBRARY_HOME_LINK);
        contents.push('\n');
        fs::write(&path, contents)
            .map_err(|error| format!("link Paper page {relative} to library home: {error}"))?;
    }
    Ok(())
}

fn agent_change_paths(status: &[u8]) -> Result<Vec<String>, String> {
    let mut paths = Vec::new();
    for entry in status
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        if entry.len() < 4 || entry[2] != b' ' {
            return Err("Git returned an invalid worktree status".into());
        }
        let state = &entry[..2];
        if state
            .iter()
            .any(|value| matches!(*value, b'D' | b'R' | b'C' | b'T' | b'U'))
        {
            return Err("Agent deleted, renamed, copied, or conflicted with a Wiki file".into());
        }
        if !state
            .iter()
            .all(|value| matches!(*value, b' ' | b'M' | b'A' | b'?'))
        {
            return Err("Agent produced an unsupported Git worktree change".into());
        }
        let path = std::str::from_utf8(&entry[3..])
            .map_err(|_| "Agent changed a path that is not valid UTF-8".to_string())?;
        if !allowed_wiki_path(path) {
            return Err(format!("Agent changed a disallowed path: {path}"));
        }
        paths.push(path.to_owned());
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::JobState;

    #[tokio::test]
    async fn failed_push_preserves_pdf_and_retry_only_publishes_existing_commit() {
        let root = std::env::temp_dir().join(format!("cognitio-publish-{}", uuid::Uuid::new_v4()));
        let workspace = root.join("workspace");
        let wiki = workspace.join("wiki");
        let remote = root.join("remote.git");
        fs::create_dir_all(wiki.join("content/papers")).unwrap();
        fs::create_dir_all(workspace.join("inbox")).unwrap();
        fs::create_dir_all(workspace.join("done")).unwrap();
        test_git(&root, &["init", "--bare", remote.to_str().unwrap()]);
        test_git(&wiki, &["init", "--initial-branch", "main"]);
        test_git(&wiki, &["config", "user.name", "Cognitio Test"]);
        test_git(&wiki, &["config", "user.email", "test@cognitio.local"]);
        fs::write(wiki.join("README.md"), "base\n").unwrap();
        test_git(&wiki, &["add", "."]);
        test_git(&wiki, &["commit", "-m", "Base"]);
        test_git(
            &wiki,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        test_git(&wiki, &["push", "-u", "origin", "main"]);
        let base = test_git_output(&wiki, &["rev-parse", "HEAD"]);
        let source = workspace.join("inbox/paper.pdf");
        fs::write(&source, b"%PDF-1.4\n").unwrap();
        let settings = crate::models::AppSettings::default();
        let job = jobs::create(&workspace, &source, &settings).unwrap();
        jobs::update(&workspace, &job.id, JobState::Preflight, "preflight", 15).unwrap();
        jobs::update(&workspace, &job.id, JobState::Running, "running", 55).unwrap();
        jobs::set_execution(&workspace, &job.id, "codex", &base).unwrap();
        fs::write(wiki.join("content/papers/paper.md"), "# Paper\n").unwrap();
        commit_agent_changes(&wiki, &job.id, &base).await.unwrap();
        let commit = test_git_output(&wiki, &["rev-parse", "HEAD"]);
        let job = jobs::update(&workspace, &job.id, JobState::Verifying, "verifying", 85).unwrap();
        test_git(
            &wiki,
            &[
                "remote",
                "set-url",
                "--push",
                "origin",
                root.join("missing.git").to_str().unwrap(),
            ],
        );

        let PublicationOutcome::Blocked(blocked) =
            verify(&job, &workspace, Some(&base)).await.unwrap()
        else {
            panic!("a failed push must block publication");
        };
        assert_eq!(blocked.task_commit.as_deref(), Some(commit.as_str()));
        assert!(
            crate::archive::complete(&workspace, &blocked, &settings.after_processing)
                .await
                .is_err()
        );
        assert!(source.is_file());
        assert!(job.input_path.is_file());

        test_git(
            &wiki,
            &[
                "remote",
                "set-url",
                "--push",
                "origin",
                remote.to_str().unwrap(),
            ],
        );
        let retry = jobs::retry(&workspace, &job.id, &settings, false).unwrap();
        let PublicationOutcome::Published(published) =
            verify(&retry, &workspace, Some(&base)).await.unwrap()
        else {
            panic!("retry must publish the existing task commit");
        };
        assert_eq!(test_git_output(&wiki, &["rev-parse", "HEAD"]), commit);
        let completed =
            crate::archive::complete(&workspace, &published, &settings.after_processing)
                .await
                .unwrap();
        assert_eq!(completed.state, JobState::Succeeded);
        assert!(!source.exists());
        assert!(!job.input_path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn agent_changes_are_limited_to_non_destructive_wiki_paths() {
        let status = b"?? content/papers/paper.md\0 M content/concepts/concept.md\0";
        assert_eq!(
            agent_change_paths(status).expect("allowed changes"),
            [
                "content/papers/paper.md".to_string(),
                "content/concepts/concept.md".to_string()
            ]
        );
        assert!(agent_change_paths(b"?? quartz.config.ts\0").is_err());
        assert!(agent_change_paths(b" D content/papers/paper.md\0").is_err());
    }

    #[tokio::test]
    async fn app_commits_validated_agent_changes_with_job_trailer() {
        let root = std::env::temp_dir().join(format!(
            "cognitio-agent-commit-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        tokio::fs::create_dir_all(root.join("content/papers"))
            .await
            .expect("paper directory");
        test_git(&root, &["init", "--initial-branch", "main"]);
        test_git(&root, &["config", "user.name", "Cognitio Test"]);
        test_git(&root, &["config", "user.email", "test@cognitio.local"]);
        tokio::fs::write(root.join("README.md"), b"base\n")
            .await
            .expect("base file");
        test_git(&root, &["add", "README.md"]);
        test_git(&root, &["commit", "-m", "Base"]);
        let base = test_git_output(&root, &["rev-parse", "HEAD"]);
        tokio::fs::write(root.join("content/papers/paper.md"), b"# Paper\n\n")
            .await
            .expect("paper page");

        commit_agent_changes(&root, "job-123", &base)
            .await
            .expect("Cognitio commit");

        assert_eq!(test_git_output(&root, &["status", "--porcelain"]), "");
        assert!(
            tokio::fs::read_to_string(root.join("content/papers/paper.md"))
                .await
                .expect("paper page")
                .contains(LIBRARY_HOME_LINK)
        );
        let body = test_git_output(&root, &["show", "-s", "--format=%B", "HEAD"]);
        assert!(body.lines().any(|line| line == "Cognitio-Job: job-123"));
        tokio::fs::remove_dir_all(root).await.expect("cleanup");
    }

    #[test]
    fn paper_library_links_are_idempotent_and_preserve_existing_aliases() {
        let root = std::env::temp_dir().join(format!(
            "cognitio-library-links-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(root.join("content/papers")).expect("paper directory");
        fs::write(root.join("content/papers/new.md"), "# New Paper\n").expect("new paper");
        fs::write(
            root.join("content/papers/linked.md"),
            "# Linked Paper\n\n[[index|Research Library]]\n",
        )
        .expect("linked paper");
        let paths = vec![
            "content/papers/new.md".to_string(),
            "content/papers/linked.md".to_string(),
            "content/concepts/ignored.md".to_string(),
        ];

        ensure_paper_library_links(&root, &paths).expect("first link pass");
        ensure_paper_library_links(&root, &paths).expect("second link pass");

        let new_page = fs::read_to_string(root.join("content/papers/new.md")).expect("new page");
        assert_eq!(new_page.matches(LIBRARY_HOME_LINK).count(), 1);
        let linked_page =
            fs::read_to_string(root.join("content/papers/linked.md")).expect("linked page");
        assert!(!linked_page.contains(LIBRARY_HOME_LINK));
        assert!(linked_page.contains("[[index|Research Library]]"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    fn test_git(cwd: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("run Git");
        assert!(
            output.status.success(),
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn test_git_output(cwd: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("run Git");
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().into()
    }
}
