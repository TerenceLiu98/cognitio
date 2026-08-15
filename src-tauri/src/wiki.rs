use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::models::{HostingProvider, RepositoryVisibility};

pub const QUARTZ_REPOSITORY: &str = "https://github.com/jackyzha0/quartz.git";
pub const QUARTZ_COMMIT: &str = "9cf87ff1c248a8ca551093214b0fec3b31415009";

pub fn initialize(
    workspace: &Path,
    remote: &str,
    visibility: &RepositoryVisibility,
    hosting: &HostingProvider,
) -> Result<(), String> {
    let wiki = workspace.join("wiki");
    if wiki.join(".git").is_dir() {
        if run_optional(&wiki, "git", &["rev-parse", "HEAD"])?.is_none() {
            run(&wiki, "git", &["add", "."])?;
            run(&wiki, "git", &["commit", "-m", "Initialize LLMWiki"])?;
        }
        configure_remote(&wiki, remote, visibility)?;
        return Ok(());
    }
    let resumable = wiki.join(".llmwiki-template.json").is_file();
    if !resumable
        && fs::read_dir(&wiki)
            .map_err(|error| format!("read Wiki directory: {error}"))?
            .next()
            .is_some()
    {
        return Err("Wiki directory is not empty and is not a Git repository".into());
    }

    if !resumable {
        let staging = workspace
            .join("processing")
            .join(format!(".quartz-template-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&staging)
            .map_err(|error| format!("create Quartz staging directory: {error}"))?;
        run(&staging, "git", &["init"])?;
        run(
            &staging,
            "git",
            &["remote", "add", "origin", QUARTZ_REPOSITORY],
        )?;
        run(
            &staging,
            "git",
            &["fetch", "--depth", "1", "origin", QUARTZ_COMMIT],
        )?;
        run(&staging, "git", &["checkout", "--detach", "FETCH_HEAD"])?;
        fs::remove_dir_all(staging.join(".git"))
            .map_err(|error| format!("remove Quartz Git metadata: {error}"))?;
        if staging.join("content").exists() {
            fs::remove_dir_all(staging.join("content"))
                .map_err(|error| format!("remove template content: {error}"))?;
        }
        write_pending_marker(&wiki, &staging)?;
        move_template_entries(&staging, &wiki)?;
        fs::remove_dir_all(&staging)
            .map_err(|error| format!("remove Quartz staging directory: {error}"))?;
        write_overlay(&wiki, hosting)?;
    } else {
        resume_template_install(workspace, &wiki, hosting)?;
    }
    run(&wiki, "npm", &["ci"])?;
    run(&wiki, "npx", &["quartz", "plugin", "install"])?;
    run(&wiki, "npx", &["quartz", "build"])?;
    run(&wiki, "git", &["init", "--initial-branch", "main"])?;
    run(&wiki, "git", &["add", "."])?;
    run(&wiki, "git", &["commit", "-m", "Initialize LLMWiki"])?;
    configure_remote(&wiki, remote, visibility)
}

fn write_overlay(wiki: &Path, hosting: &HostingProvider) -> Result<(), String> {
    fs::create_dir_all(wiki.join("content/papers"))
        .map_err(|error| format!("create papers directory: {error}"))?;
    fs::create_dir_all(wiki.join("content/concepts"))
        .map_err(|error| format!("create concepts directory: {error}"))?;
    fs::create_dir_all(wiki.join("assets/papers"))
        .map_err(|error| format!("create paper assets directory: {error}"))?;
    fs::write(
        wiki.join("content/index.md"),
        "---\ntitle: LLMWiki\n---\n\n# LLMWiki\n\nA connected library of papers and concepts.\n",
    )
    .map_err(|error| format!("write Wiki homepage: {error}"))?;
    fs::write(wiki.join("references.bib"), "")
        .map_err(|error| format!("write bibliography: {error}"))?;
    fs::write(
        wiki.join(".llmwiki-template.json"),
        format!("{{\"quartzCommit\":\"{QUARTZ_COMMIT}\",\"templateInstalled\":true}}\n"),
    )
    .map_err(|error| format!("write template marker: {error}"))?;
    let mut ignore = fs::read_to_string(wiki.join(".gitignore")).unwrap_or_default();
    for pattern in ["\n.llmwiki-work/\n", "\n*.pdf\n", "\npublic/\n"] {
        if !ignore.contains(pattern.trim()) {
            ignore.push_str(pattern);
        }
    }
    fs::write(wiki.join(".gitignore"), ignore)
        .map_err(|error| format!("write Wiki gitignore: {error}"))?;
    match hosting {
        HostingProvider::Cloudflare => fs::write(wiki.join("LLMWIKI_HOSTING.md"), "# Cloudflare Pages\n\nProduction branch: `main`\n\nBuild command: `npx quartz plugin install && npx quartz build`\n\nOutput directory: `public`\n").map_err(|error| format!("write Cloudflare instructions: {error}"))?,
        HostingProvider::GithubPages => {
            fs::create_dir_all(wiki.join(".github/workflows")).map_err(|error| format!("create workflow directory: {error}"))?;
            fs::write(wiki.join(".github/workflows/deploy.yml"), GITHUB_PAGES_WORKFLOW).map_err(|error| format!("write GitHub Pages workflow: {error}"))?;
        }
    }
    Ok(())
}

fn write_pending_marker(wiki: &Path, staging: &Path) -> Result<(), String> {
    let marker = serde_json::json!({
        "quartzCommit": QUARTZ_COMMIT,
        "templateInstalled": false,
        "stagingPath": staging,
    });
    fs::write(
        wiki.join(".llmwiki-template.json"),
        serde_json::to_vec_pretty(&marker)
            .map_err(|error| format!("serialize template marker: {error}"))?,
    )
    .map_err(|error| format!("write pending template marker: {error}"))
}

fn resume_template_install(
    workspace: &Path,
    wiki: &Path,
    hosting: &HostingProvider,
) -> Result<(), String> {
    let marker_path = wiki.join(".llmwiki-template.json");
    let marker: serde_json::Value = serde_json::from_slice(
        &fs::read(&marker_path).map_err(|error| format!("read template marker: {error}"))?,
    )
    .map_err(|error| format!("parse template marker: {error}"))?;
    if marker
        .get("templateInstalled")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return Ok(());
    }
    let staging = marker
        .get("stagingPath")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| "incomplete template marker has no staging path".to_string())?;
    let valid_staging = staging.parent() == Some(workspace.join("processing").as_path())
        && staging
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with(".quartz-template-"));
    if !valid_staging {
        return Err("template marker contains an unsafe staging path".into());
    }
    if staging.is_dir() {
        move_template_entries(&staging, wiki)?;
        fs::remove_dir_all(&staging)
            .map_err(|error| format!("remove Quartz staging directory: {error}"))?;
    }
    write_overlay(wiki, hosting)
}

fn move_template_entries(staging: &Path, wiki: &Path) -> Result<(), String> {
    for entry in fs::read_dir(staging)
        .map_err(|error| format!("read Quartz template: {error}"))?
        .flatten()
    {
        fs::rename(entry.path(), wiki.join(entry.file_name()))
            .map_err(|error| format!("install Quartz template: {error}"))?;
    }
    Ok(())
}

fn configure_remote(
    wiki: &Path,
    remote: &str,
    visibility: &RepositoryVisibility,
) -> Result<(), String> {
    let remote = remote.trim();
    if remote.is_empty() {
        return Ok(());
    }
    validate_remote(remote)?;
    if wiki.join(".git").is_dir()
        && run_optional(wiki, "git", &["remote", "get-url", "origin"])?.is_some()
    {
        return Ok(());
    }
    if remote.contains("://") || remote.starts_with("git@") {
        run(wiki, "git", &["remote", "add", "origin", remote])?;
        run(wiki, "git", &["push", "-u", "origin", "main"])
    } else {
        let visibility_flag = match visibility {
            RepositoryVisibility::Private => "--private",
            RepositoryVisibility::Public => "--public",
        };
        run(
            wiki,
            "gh",
            &[
                "repo",
                "create",
                remote,
                visibility_flag,
                "--source",
                ".",
                "--remote",
                "origin",
                "--push",
            ],
        )
    }
}

pub(crate) fn validate_remote(remote: &str) -> Result<(), String> {
    let valid_path = |path: &str| {
        let parts: Vec<_> = path.trim_end_matches(".git").split('/').collect();
        parts.len() == 2
            && parts.iter().all(|part| {
                !part.is_empty()
                    && part.chars().all(|character| {
                        character.is_ascii_alphanumeric() || ".-_".contains(character)
                    })
            })
    };
    let slug = valid_path(remote);
    let url = remote
        .strip_prefix("https://github.com/")
        .is_some_and(valid_path)
        || remote
            .strip_prefix("git@github.com:")
            .is_some_and(valid_path);
    if slug || url {
        Ok(())
    } else {
        Err("GitHub repository must be owner/name or a github.com Git URL".into())
    }
}

fn run(cwd: &Path, executable: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(executable)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("start {executable}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{executable} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn run_optional(cwd: &Path, executable: &str, args: &[&str]) -> Result<Option<String>, String> {
    let output = Command::new(executable)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("start {executable}: {error}"))?;
    Ok(output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().into()))
}

const GITHUB_PAGES_WORKFLOW: &str = r#"name: Deploy Quartz site to GitHub Pages
on:
  push:
    branches: [main]
permissions:
  contents: read
  pages: write
  id-token: write
concurrency:
  group: pages
  cancel-in-progress: false
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0
      - uses: actions/setup-node@v6
        with:
          node-version: 24
      - run: npm ci
      - run: npx quartz plugin install
      - run: npx quartz build
      - uses: actions/upload-pages-artifact@v3
        with:
          path: public
  deploy:
    needs: build
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    runs-on: ubuntu-latest
    steps:
      - id: deployment
        uses: actions/deploy-pages@v4
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_github_remotes() {
        assert!(validate_remote("owner/repo").is_ok());
        assert!(validate_remote("https://github.com/owner/repo.git").is_ok());
        assert!(validate_remote("https://example.com/repo.git").is_err());
        assert!(validate_remote("owner/repo;rm").is_err());
    }

    #[test]
    fn resumes_an_interrupted_template_move() {
        let root = std::env::temp_dir().join(format!(
            "llmwiki-quartz-resume-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let wiki = root.join("wiki");
        let staging = root.join("processing").join(".quartz-template-interrupted");
        fs::create_dir_all(&wiki).expect("wiki");
        fs::create_dir_all(&staging).expect("staging");
        fs::write(wiki.join("already-moved.txt"), "first").expect("moved file");
        fs::write(staging.join("remaining.txt"), "second").expect("remaining file");
        write_pending_marker(&wiki, &staging).expect("pending marker");

        resume_template_install(&root, &wiki, &HostingProvider::Cloudflare).expect("resume");

        assert!(wiki.join("already-moved.txt").is_file());
        assert!(wiki.join("remaining.txt").is_file());
        assert!(wiki.join("content/index.md").is_file());
        assert!(!staging.exists());
        let marker = fs::read_to_string(wiki.join(".llmwiki-template.json")).expect("marker");
        assert!(marker.contains("\"templateInstalled\":true"));
        fs::remove_dir_all(root).expect("cleanup");
    }
}
