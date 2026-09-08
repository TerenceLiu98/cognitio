use crate::jobs::JobState;
use crate::{
    app_events::{notify, publish_record},
    jobs,
    models::DeploymentSummary,
    tools,
};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tauri::AppHandle;

pub fn resume(app: AppHandle, workspace: PathBuf) {
    let Ok(records) = jobs::load_all(&workspace) else {
        return;
    };
    for record in records.into_iter().filter(|record| {
        record.state == JobState::Succeeded
            && matches!(
                record.deployment.status.as_str(),
                "pending" | "running" | "unknown"
            )
    }) {
        monitor(
            app.clone(),
            workspace.clone(),
            record.id,
            record.task_commit,
        );
    }
}

pub fn monitor(app: AppHandle, workspace: PathBuf, job_id: String, commit: Option<String>) {
    tauri::async_runtime::spawn(async move {
        let Some(commit) = commit else {
            update_deployment(
                &app,
                &workspace,
                &job_id,
                "unknown",
                None,
                Some("Published commit is unavailable".into()),
            )
            .await;
            return;
        };
        tokio::time::sleep(Duration::from_secs(5)).await;
        for _ in 0..60 {
            if let Ok(value) = github_run(&workspace.join("wiki"), &commit).await {
                let status = value.get("status").and_then(serde_json::Value::as_str);
                let conclusion = value.get("conclusion").and_then(serde_json::Value::as_str);
                let url = value
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                if status == Some("completed") {
                    let successful = conclusion == Some("success");
                    update_deployment(
                        &app,
                        &workspace,
                        &job_id,
                        if successful { "succeeded" } else { "failed" },
                        url,
                        (!successful).then(|| {
                            format!(
                                "GitHub Pages concluded with {}",
                                conclusion.unwrap_or("unknown")
                            )
                        }),
                    )
                    .await;
                    if !successful {
                        notify(
                            "Cognitio site deployment failed",
                            "Open the task details for the GitHub Actions run",
                        );
                    }
                    return;
                }
                update_deployment(&app, &workspace, &job_id, "running", url, None).await;
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
        update_deployment(
            &app,
            &workspace,
            &job_id,
            "unknown",
            None,
            Some("GitHub Pages status was not available within 10 minutes".into()),
        )
        .await;
    });
}

async fn github_run(wiki: &Path, commit: &str) -> Result<serde_json::Value, String> {
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        tools::tokio_command("gh")?
            .args([
                "run",
                "list",
                "--workflow",
                "deploy.yml",
                "--commit",
                commit,
                "--limit",
                "1",
                "--json",
                "status,conclusion,url",
            ])
            .current_dir(wiki)
            .stdin(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| "GitHub Actions status timed out".to_string())?
    .map_err(|error| format!("query GitHub Actions status: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().into());
    }
    let values: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("decode GitHub Actions status: {error}"))?;
    values
        .into_iter()
        .next()
        .ok_or_else(|| "GitHub Pages workflow has not started".into())
}

async fn update_deployment(
    app: &AppHandle,
    workspace: &Path,
    job_id: &str,
    status: &str,
    url: Option<String>,
    error: Option<String>,
) {
    let update_workspace = workspace.to_path_buf();
    let update_job_id = job_id.to_owned();
    let deployment = DeploymentSummary {
        status: status.into(),
        url,
        error,
        updated_at: Some(chrono::Utc::now().to_rfc3339()),
    };
    if let Ok(Ok(record)) = tokio::task::spawn_blocking(move || {
        jobs::set_deployment(&update_workspace, &update_job_id, deployment)
    })
    .await
    {
        publish_record(app, &record);
    }
}
