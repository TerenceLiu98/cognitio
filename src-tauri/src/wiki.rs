use std::{fs, io::Cursor, path::Path, process::Stdio, time::Duration};

use sha2::{Digest, Sha256};
use tokio::{process::Command, sync::watch};

use crate::models::RepositoryVisibility;

pub const QUARTZ_COMMIT: &str = "9cf87ff1c248a8ca551093214b0fec3b31415009";
const TEMPLATE_BYTES: &[u8] = include_bytes!("../resources/quartz-template.zip");
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5 * 60);

pub async fn install_template(
    workspace: &Path,
    slug: &str,
    site_title: &str,
) -> Result<(), String> {
    let site_title = normalize_site_title(site_title)?;
    let workspace = workspace.to_path_buf();
    let slug = slug.to_owned();
    tokio::task::spawn_blocking(move || install_template_blocking(&workspace, &slug, &site_title))
        .await
        .map_err(|error| format!("join template installation: {error}"))?
}

fn install_template_blocking(workspace: &Path, slug: &str, site_title: &str) -> Result<(), String> {
    cleanup_legacy_staging(workspace)?;
    let wiki = workspace.join("wiki");
    let resumable = wiki.join(".llmwiki-template.json").is_file();
    if resumable && wiki.join("package-lock.json").is_file() && wiki.join("quartz").is_dir() {
        write_overlay(&wiki, slug, site_title)?;
        return Ok(());
    }
    if !resumable
        && fs::read_dir(&wiki)
            .map_err(|error| format!("read Wiki directory: {error}"))?
            .next()
            .is_some()
    {
        return Err("Wiki directory is not empty and has no Cognitio template marker".into());
    }
    let staging = workspace
        .join("processing")
        .join(format!(".llmwiki-template-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&staging)
        .map_err(|error| format!("create template staging directory: {error}"))?;
    let result = (|| {
        let mut archive = zip::ZipArchive::new(Cursor::new(TEMPLATE_BYTES))
            .map_err(|error| format!("open bundled Quartz template: {error}"))?;
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|error| format!("read bundled template entry: {error}"))?;
            let relative = entry
                .enclosed_name()
                .ok_or_else(|| "bundled template contains an unsafe path".to_string())?
                .to_path_buf();
            let destination = staging.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(&destination)
                    .map_err(|error| format!("create template directory: {error}"))?;
                continue;
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("create template parent: {error}"))?;
            }
            let mut output = fs::File::create(&destination)
                .map_err(|error| format!("create template file: {error}"))?;
            std::io::copy(&mut entry, &mut output)
                .map_err(|error| format!("extract template file: {error}"))?;
            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;

                fs::set_permissions(&destination, fs::Permissions::from_mode(mode))
                    .map_err(|error| format!("restore template file permissions: {error}"))?;
            }
        }
        for entry in fs::read_dir(&staging)
            .map_err(|error| format!("read template staging directory: {error}"))?
        {
            let entry = entry.map_err(|error| format!("read template entry: {error}"))?;
            let destination = wiki.join(entry.file_name());
            if destination.is_dir() {
                fs::remove_dir_all(&destination)
                    .map_err(|error| format!("replace incomplete template directory: {error}"))?;
            } else if destination.exists() {
                fs::remove_file(&destination)
                    .map_err(|error| format!("replace incomplete template file: {error}"))?;
            }
            fs::rename(entry.path(), destination)
                .map_err(|error| format!("install template entry: {error}"))?;
        }
        write_overlay(&wiki, slug, site_title)
    })();
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn cleanup_legacy_staging(workspace: &Path) -> Result<(), String> {
    let processing = workspace.join("processing");
    let entries =
        fs::read_dir(&processing).map_err(|error| format!("read processing directory: {error}"))?;
    for entry in entries.flatten() {
        let is_legacy = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(".quartz-template-"));
        if is_legacy && entry.path().is_dir() {
            fs::remove_dir_all(entry.path())
                .map_err(|error| format!("remove interrupted legacy template: {error}"))?;
        }
    }
    Ok(())
}

fn write_overlay(wiki: &Path, slug: &str, site_title: &str) -> Result<(), String> {
    for directory in [
        "content/papers",
        "content/concepts",
        "assets/papers",
        ".github/workflows",
    ] {
        fs::create_dir_all(wiki.join(directory))
            .map_err(|error| format!("create Wiki directory {directory}: {error}"))?;
    }
    fs::write(wiki.join("content/index.md"), homepage(site_title)?)
        .map_err(|error| format!("write Wiki homepage: {error}"))?;
    fs::write(wiki.join("references.bib"), "")
        .map_err(|error| format!("write bibliography: {error}"))?;
    fs::write(
        wiki.join(".llmwiki-template.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "quartzCommit": QUARTZ_COMMIT,
            "sha256": format!("{:x}", Sha256::digest(TEMPLATE_BYTES)),
            "templateInstalled": true
        }))
        .map_err(|error| format!("serialize template marker: {error}"))?,
    )
    .map_err(|error| format!("write template marker: {error}"))?;
    let mut ignore = fs::read_to_string(wiki.join(".gitignore")).unwrap_or_default();
    for pattern in [".llmwiki-work/", "*.pdf", "public/"] {
        if !ignore.lines().any(|line| line.trim() == pattern) {
            if !ignore.ends_with('\n') && !ignore.is_empty() {
                ignore.push('\n');
            }
            ignore.push_str(pattern);
            ignore.push('\n');
        }
    }
    fs::write(wiki.join(".gitignore"), ignore)
        .map_err(|error| format!("write Wiki gitignore: {error}"))?;
    fs::write(
        wiki.join(".github/workflows/deploy.yml"),
        github_pages_workflow(),
    )
    .map_err(|error| format!("write GitHub Pages workflow: {error}"))?;
    configure_site(wiki, slug, site_title)
}

fn configure_site(wiki: &Path, slug: &str, site_title: &str) -> Result<(), String> {
    let source = wiki.join("quartz.config.default.yaml");
    let path = wiki.join("quartz.config.yaml");
    let mut contents = fs::read_to_string(&source)
        .map_err(|error| format!("read Quartz configuration: {error}"))?;
    let mut parts = slug.split('/');
    let owner = parts.next().unwrap_or_default();
    let repository = parts.next().unwrap_or_default();
    let base_url = if repository.eq_ignore_ascii_case(&format!("{owner}.github.io")) {
        format!("{owner}.github.io")
    } else {
        format!("{owner}.github.io/{repository}")
    };
    replace_yaml_scalar(&mut contents, "baseUrl", &base_url, false)?;
    replace_yaml_scalar(&mut contents, "pageTitle", site_title, true)?;
    fs::write(path, contents).map_err(|error| format!("configure Quartz site: {error}"))
}

pub fn normalize_site_title(site_title: &str) -> Result<String, String> {
    let title = site_title.trim();
    if title.is_empty() {
        return Err("site title cannot be empty".into());
    }
    if title.chars().count() > 80 {
        return Err("site title cannot exceed 80 characters".into());
    }
    if title.chars().any(char::is_control) {
        return Err("site title cannot contain control characters".into());
    }
    Ok(title.into())
}

fn replace_yaml_scalar(
    contents: &mut String,
    key: &str,
    value: &str,
    quote: bool,
) -> Result<bool, String> {
    let prefix = format!("{key}:");
    let old = contents
        .lines()
        .find(|line| line.trim_start().starts_with(&prefix))
        .ok_or_else(|| format!("Quartz configuration has no {key}"))?
        .to_owned();
    let indentation = old.len() - old.trim_start().len();
    let value = if quote {
        serde_json::to_string(value).map_err(|error| format!("encode site title: {error}"))?
    } else {
        value.to_owned()
    };
    let new = format!("{}{key}: {value}", " ".repeat(indentation));
    if old == new {
        return Ok(false);
    }
    *contents = contents.replacen(&old, &new, 1);
    Ok(true)
}

fn homepage(site_title: &str) -> Result<String, String> {
    let yaml_title = serde_json::to_string(site_title)
        .map_err(|error| format!("encode homepage title: {error}"))?;
    Ok(format!(
        "---\ntitle: {yaml_title}\n---\n\n# {site_title}\n\nA connected library of papers and concepts.\n"
    ))
}

pub async fn update_site_title(workspace: &Path, site_title: &str) -> Result<(), String> {
    let site_title = normalize_site_title(site_title)?;
    let wiki = workspace.join("wiki");
    let (_cancel_sender, mut cancel) = watch::channel(false);
    run(&wiki, "git", &["pull", "--ff-only"], &mut cancel).await?;
    let status = run_optional(&wiki, "git", &["status", "--porcelain"], &mut cancel)
        .await?
        .ok_or_else(|| "could not inspect Wiki worktree".to_string())?;
    if !status.is_empty() {
        return Err(
            "Wiki has uncommitted changes; finish or discard them before changing the site title"
                .into(),
        );
    }
    let update_wiki = wiki.clone();
    let changed =
        tokio::task::spawn_blocking(move || update_site_title_blocking(&update_wiki, &site_title))
            .await
            .map_err(|error| format!("join site title update: {error}"))??;
    if changed {
        run(
            &wiki,
            "git",
            &[
                "add",
                "--",
                "quartz.config.yaml",
                "content/index.md",
                ".github/workflows/deploy.yml",
            ],
            &mut cancel,
        )
        .await?;
        run(
            &wiki,
            "git",
            &["commit", "-m", "Update site title"],
            &mut cancel,
        )
        .await?;
    }
    run(&wiki, "git", &["push"], &mut cancel).await
}

fn update_site_title_blocking(wiki: &Path, site_title: &str) -> Result<bool, String> {
    let config_path = wiki.join("quartz.config.yaml");
    let mut config = fs::read_to_string(&config_path)
        .map_err(|error| format!("read Quartz configuration: {error}"))?;
    let mut changed = replace_yaml_scalar(&mut config, "pageTitle", site_title, true)?;
    if changed {
        fs::write(&config_path, config)
            .map_err(|error| format!("write Quartz configuration: {error}"))?;
    }

    let index_path = wiki.join("content/index.md");
    let legacy_homepage =
        "---\ntitle: LLMWiki\n---\n\n# LLMWiki\n\nA connected library of papers and concepts.\n";
    if fs::read_to_string(&index_path).ok().as_deref() == Some(legacy_homepage) {
        fs::write(&index_path, homepage(site_title)?)
            .map_err(|error| format!("write Wiki homepage: {error}"))?;
        changed = true;
    }
    let workflow_path = wiki.join(".github/workflows/deploy.yml");
    if let Ok(workflow) = fs::read_to_string(&workflow_path) {
        let migrated = workflow.replacen(
            "name: Deploy LLMWiki to GitHub Pages",
            "name: Deploy site to GitHub Pages",
            1,
        );
        if migrated != workflow {
            fs::write(&workflow_path, migrated)
                .map_err(|error| format!("write GitHub Pages workflow: {error}"))?;
            changed = true;
        }
    }
    Ok(changed)
}

pub async fn initialize_git(
    workspace: &Path,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(), String> {
    let wiki = workspace.join("wiki");
    if !wiki.join(".git").is_dir() {
        run(&wiki, "git", &["init", "--initial-branch", "main"], cancel).await?;
    }
    if run_optional(&wiki, "git", &["rev-parse", "HEAD"], cancel)
        .await?
        .is_none()
    {
        run(&wiki, "git", &["add", "."], cancel).await?;
        run(
            &wiki,
            "git",
            &["commit", "-m", "Initialize knowledge site"],
            cancel,
        )
        .await?;
    }
    Ok(())
}

pub async fn configure_repository(
    workspace: &Path,
    remote: &str,
    visibility: &RepositoryVisibility,
    cancel: &mut watch::Receiver<bool>,
) -> Result<String, String> {
    let wiki = workspace.join("wiki");
    let slug = repository_slug(remote)?;
    if run_optional(&wiki, "git", &["remote", "get-url", "origin"], cancel)
        .await?
        .is_none()
    {
        if run_optional(
            &wiki,
            "gh",
            &["repo", "view", &slug, "--json", "nameWithOwner"],
            cancel,
        )
        .await?
        .is_some()
        {
            let url = format!("https://github.com/{slug}.git");
            run(&wiki, "git", &["remote", "add", "origin", &url], cancel).await?;
        } else {
            let visibility_flag = match visibility {
                RepositoryVisibility::Private => "--private",
                RepositoryVisibility::Public => "--public",
            };
            run(
                &wiki,
                "gh",
                &[
                    "repo",
                    "create",
                    &slug,
                    visibility_flag,
                    "--source",
                    ".",
                    "--remote",
                    "origin",
                ],
                cancel,
            )
            .await?;
        }
    }
    Ok(slug)
}

pub async fn configure_pages(
    slug: &str,
    workspace: &Path,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(), String> {
    let endpoint = format!("repos/{slug}/pages");
    let wiki = workspace.join("wiki");
    let create = run_optional(
        &wiki,
        "gh",
        &[
            "api",
            "--method",
            "POST",
            &endpoint,
            "-f",
            "build_type=workflow",
        ],
        cancel,
    )
    .await?;
    if create.is_none() {
        run(
            &wiki,
            "gh",
            &[
                "api",
                "--method",
                "PUT",
                &endpoint,
                "-f",
                "build_type=workflow",
            ],
            cancel,
        )
        .await?;
    }
    ensure_pages_branch_policy(slug, &wiki, cancel).await?;
    Ok(())
}

async fn ensure_pages_branch_policy(
    slug: &str,
    wiki: &Path,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(), String> {
    let endpoint = format!("repos/{slug}/environments/github-pages/deployment-branch-policies");
    let policies = run_optional(wiki, "gh", &["api", &endpoint], cancel)
        .await?
        .ok_or_else(|| "could not inspect the GitHub Pages deployment branch policy".to_string())?;
    if pages_policy_allows_main(&policies)? {
        return Ok(());
    }
    run(
        wiki,
        "gh",
        &[
            "api",
            "--method",
            "POST",
            &endpoint,
            "-f",
            "name=main",
            "-f",
            "type=branch",
        ],
        cancel,
    )
    .await
}

fn pages_policy_allows_main(json: &str) -> Result<bool, String> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|error| format!("parse GitHub Pages branch policies: {error}"))?;
    Ok(value
        .get("branch_policies")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|policies| {
            policies.iter().any(|policy| {
                policy.get("type").and_then(serde_json::Value::as_str) == Some("branch")
                    && policy.get("name").and_then(serde_json::Value::as_str) == Some("main")
            })
        }))
}

pub async fn push_main(workspace: &Path, cancel: &mut watch::Receiver<bool>) -> Result<(), String> {
    run(
        &workspace.join("wiki"),
        "git",
        &["push", "-u", "origin", "main"],
        cancel,
    )
    .await
}

pub fn repository_slug(remote: &str) -> Result<String, String> {
    validate_remote(remote)?;
    let path = remote
        .trim()
        .strip_prefix("https://github.com/")
        .or_else(|| remote.trim().strip_prefix("git@github.com:"))
        .unwrap_or(remote.trim())
        .trim_end_matches(".git");
    Ok(path.to_owned())
}

pub fn pages_url(slug: &str) -> String {
    let mut parts = slug.split('/');
    let owner = parts.next().unwrap_or_default();
    let repository = parts.next().unwrap_or_default();
    if repository.eq_ignore_ascii_case(&format!("{owner}.github.io")) {
        format!("https://{owner}.github.io/")
    } else {
        format!("https://{owner}.github.io/{repository}/")
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
    let remote = remote.trim();
    if valid_path(remote)
        || remote
            .strip_prefix("https://github.com/")
            .is_some_and(valid_path)
        || remote
            .strip_prefix("git@github.com:")
            .is_some_and(valid_path)
    {
        Ok(())
    } else {
        Err("GitHub repository must be owner/name or a github.com Git URL".into())
    }
}

async fn run(
    cwd: &Path,
    executable: &str,
    args: &[&str],
    cancel: &mut watch::Receiver<bool>,
) -> Result<(), String> {
    match command_output(cwd, executable, args, cancel, false).await? {
        Some(_) => Ok(()),
        None => Err(format!("{executable} {} failed", args.join(" "))),
    }
}

async fn run_optional(
    cwd: &Path,
    executable: &str,
    args: &[&str],
    cancel: &mut watch::Receiver<bool>,
) -> Result<Option<String>, String> {
    command_output(cwd, executable, args, cancel, true).await
}

async fn command_output(
    cwd: &Path,
    executable: &str,
    args: &[&str],
    cancel: &mut watch::Receiver<bool>,
    allow_failure: bool,
) -> Result<Option<String>, String> {
    if *cancel.borrow() {
        return Err("initialization cancelled".into());
    }
    let child = Command::new(executable)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("start {executable}: {error}"))?;
    let output = child.wait_with_output();
    tokio::pin!(output);
    tokio::select! {
        output = tokio::time::timeout(COMMAND_TIMEOUT, &mut output) => {
            let output = output
                .map_err(|_| format!("{executable} {} timed out", args.join(" ")))?
                .map_err(|error| format!("wait for {executable}: {error}"))?;
            if output.status.success() {
                Ok(Some(String::from_utf8_lossy(&output.stdout).trim().into()))
            } else {
                let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                if allow_failure {
                    Ok(None)
                } else {
                    Err(format!("{executable} {} failed: {error}", args.join(" ")))
                }
            }
        }
        changed = cancel.changed() => {
            let _ = changed;
            Err("initialization cancelled".into())
        }
    }
}

fn github_pages_workflow() -> &'static str {
    r#"name: Deploy site to GitHub Pages
on:
  push:
    branches: [main]
  workflow_dispatch:
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
          cache: npm
      - uses: actions/configure-pages@v5
      - run: npm ci
      - run: npx quartz plugin install
      - run: npx quartz build
      - uses: actions/upload-pages-artifact@v4
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
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_github_remotes_and_extracts_slug() {
        assert_eq!(repository_slug("owner/repo").unwrap(), "owner/repo");
        assert_eq!(
            repository_slug("https://github.com/owner/repo.git").unwrap(),
            "owner/repo"
        );
        assert!(validate_remote("https://example.com/owner/repo").is_err());
    }

    #[test]
    fn pages_url_handles_user_and_project_sites() {
        assert_eq!(
            pages_url("owner/owner.github.io"),
            "https://owner.github.io/"
        );
        assert_eq!(pages_url("owner/wiki"), "https://owner.github.io/wiki/");
    }

    #[test]
    fn workflow_uses_official_pages_artifact_deployment() {
        let workflow = github_pages_workflow();
        assert!(workflow.contains("actions/upload-pages-artifact@v4"));
        assert!(workflow.contains("actions/deploy-pages@v4"));
        assert!(!workflow.contains("cloudflare"));
        assert!(!workflow.contains("gh-pages"));
    }

    #[test]
    fn recognizes_main_pages_environment_policy() {
        assert!(pages_policy_allows_main(
            r#"{"branch_policies":[{"name":"main","type":"branch"}]}"#
        )
        .unwrap());
        assert!(!pages_policy_allows_main(
            r#"{"branch_policies":[{"name":"master","type":"branch"}]}"#
        )
        .unwrap());
    }

    #[tokio::test]
    async fn installs_bundled_template_without_network_commands() {
        let root = std::env::temp_dir().join(format!(
            "llmwiki-bundled-template-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        crate::workspace::initialize(&root).expect("workspace");
        let legacy = root.join("processing/.quartz-template-interrupted");
        fs::create_dir_all(&legacy).expect("legacy staging");
        fs::write(
            root.join("wiki/.llmwiki-template.json"),
            r#"{"templateInstalled":false}"#,
        )
        .expect("legacy marker");
        fs::create_dir_all(root.join("wiki/content")).expect("content");
        fs::write(root.join("wiki/content/user-note.md"), "keep me").expect("user note");
        install_template(&root, "owner/research", "Research: \"Notes\"")
            .await
            .expect("template");
        assert!(root.join("wiki/package-lock.json").is_file());
        assert!(root.join("wiki/.github/workflows/deploy.yml").is_file());
        let config = fs::read_to_string(root.join("wiki/quartz.config.yaml")).expect("config");
        assert!(config.contains("baseUrl: owner.github.io/research"));
        assert!(config.contains("pageTitle: \"Research: \\\"Notes\\\"\""));
        let homepage = fs::read_to_string(root.join("wiki/content/index.md")).expect("homepage");
        assert!(homepage.contains("title: \"Research: \\\"Notes\\\"\""));
        assert!(!homepage.contains("LLMWiki"));
        assert!(!root.join("wiki/node_modules").exists());
        assert!(!legacy.exists());
        assert!(root.join("wiki/content/user-note.md").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = fs::metadata(root.join("wiki/quartz/bootstrap-cli.mjs"))
                .expect("bootstrap metadata")
                .permissions()
                .mode();
            assert_ne!(mode & 0o111, 0);
        }
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn validates_site_titles() {
        assert_eq!(normalize_site_title("  研究笔记  ").unwrap(), "研究笔记");
        assert!(normalize_site_title(" ").is_err());
        assert!(normalize_site_title(&"x".repeat(81)).is_err());
        assert!(normalize_site_title("line\nbreak").is_err());
    }

    #[tokio::test]
    async fn updates_existing_site_title_and_pushes() {
        let root = std::env::temp_dir().join(format!(
            "cognitio-site-title-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let workspace = root.join("workspace");
        let wiki = workspace.join("wiki");
        let remote = root.join("remote.git");
        fs::create_dir_all(wiki.join("content")).expect("Wiki directories");
        fs::create_dir_all(wiki.join(".github/workflows")).expect("workflow directory");
        git(&root, &["init", "--bare", remote.to_str().expect("remote")]);
        git(&wiki, &["init", "--initial-branch", "main"]);
        git(&wiki, &["config", "user.name", "Cognitio Test"]);
        git(&wiki, &["config", "user.email", "test@cognitio.local"]);
        fs::write(
            wiki.join("quartz.config.yaml"),
            "configuration:\n  pageTitle: Quartz 5\n",
        )
        .expect("Quartz config");
        fs::write(
            wiki.join("content/index.md"),
            "---\ntitle: LLMWiki\n---\n\n# LLMWiki\n\nA connected library of papers and concepts.\n",
        )
        .expect("legacy homepage");
        fs::write(
            wiki.join(".github/workflows/deploy.yml"),
            "name: Deploy LLMWiki to GitHub Pages\n",
        )
        .expect("legacy workflow");
        git(&wiki, &["add", "."]);
        git(&wiki, &["commit", "-m", "Base"]);
        git(
            &wiki,
            &["remote", "add", "origin", remote.to_str().expect("remote")],
        );
        git(&wiki, &["push", "-u", "origin", "main"]);

        update_site_title(&workspace, "研究: \"Notes\"")
            .await
            .expect("site title update");

        let config = fs::read_to_string(wiki.join("quartz.config.yaml")).expect("config");
        let homepage = fs::read_to_string(wiki.join("content/index.md")).expect("homepage");
        let workflow =
            fs::read_to_string(wiki.join(".github/workflows/deploy.yml")).expect("workflow");
        assert!(config.contains("pageTitle: \"研究: \\\"Notes\\\"\""));
        assert!(homepage.contains("# 研究: \"Notes\""));
        assert!(!homepage.contains("LLMWiki"));
        assert!(!workflow.contains("LLMWiki"));
        assert_eq!(
            git_output(&wiki, &["rev-parse", "HEAD"]),
            git_output(&wiki, &["rev-parse", "@{u}"])
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    fn git(cwd: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .expect("run Git");
        assert!(status.success(), "git {}", args.join(" "));
    }

    fn git_output(cwd: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("run Git");
        assert!(output.status.success(), "git {}", args.join(" "));
        String::from_utf8_lossy(&output.stdout).trim().into()
    }
}
