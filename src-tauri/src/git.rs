use std::{path::Path, process::Stdio, time::Duration};

use crate::tools;

pub async fn output(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = output_bytes(cwd, args).await?;
    Ok(String::from_utf8_lossy(&output).trim().into())
}

pub async fn output_bytes(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = tokio::time::timeout(
        Duration::from_secs(5 * 60),
        tools::tokio_command("git")?
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| format!("git {} timed out", args.join(" ")))?
    .map_err(|error| format!("run git {}: {error}", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

pub async fn success(cwd: &Path, args: &[&str]) -> Result<bool, String> {
    tokio::time::timeout(
        Duration::from_secs(5 * 60),
        tools::tokio_command("git")?
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .status(),
    )
    .await
    .map_err(|_| format!("git {} timed out", args.join(" ")))?
    .map(|status| status.success())
    .map_err(|error| format!("run git {}: {error}", args.join(" ")))
}

pub async fn remote_contains(wiki: &Path, commit: &str) -> Result<bool, String> {
    output(wiki, &["fetch", "--no-tags", "origin"]).await?;
    // Resolve the upstream first so missing configuration is not mistaken for an absent commit.
    let upstream = output(wiki, &["rev-parse", "@{u}"]).await?;
    success(wiki, &["merge-base", "--is-ancestor", commit, &upstream]).await
}
