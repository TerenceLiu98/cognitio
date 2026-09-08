use crate::{
    app_events::publish,
    app_state::{snapshot, AppState},
    config, credentials, jobs, launch_at_login,
    models::{AppSettings, AppSnapshot},
    wiki,
};
use std::path::Path;
use tauri::{AppHandle, Manager};

async fn persist_settings(
    mut settings: AppSettings,
    app: &AppHandle,
    state: &AppState,
) -> Result<AppSnapshot, String> {
    if settings.schema_version != 3 {
        return Err("unsupported settings schema version".into());
    }
    settings.site_title = wiki::normalize_site_title(&settings.site_title)?;
    let before = snapshot(state)?;
    if before.configured && before.settings.workspace_root != settings.workspace_root {
        return Err("the workspace is fixed after initialization".into());
    }
    if before.configured && before.settings.git_remote != settings.git_remote {
        return Err(
            "the GitHub repository is fixed after initialization; use a new workspace to change it"
                .into(),
        );
    }
    if before.configured && before.settings.site_title != settings.site_title {
        let _repository_guard = state.jobs.repository.lock().await;
        let workspace = std::path::PathBuf::from(&before.settings.workspace_root);
        let busy = tokio::task::spawn_blocking(move || {
            jobs::load_all(&workspace).map(|records| records.iter().any(blocks_site_title_update))
        })
        .await
        .map_err(|error| format!("join site title task check: {error}"))??;
        if busy {
            return Err("wait for the active paper job before changing the site title".into());
        }
        wiki::update_site_title(
            Path::new(&before.settings.workspace_root),
            &settings.site_title,
        )
        .await?;
    }
    let home = app
        .path()
        .home_dir()
        .map_err(|error| format!("resolve home directory: {error}"))?;
    let executable =
        std::env::current_exe().map_err(|error| format!("resolve executable: {error}"))?;
    let config_dir = state.config_dir.clone();
    let persisted = settings.clone();
    let previous_launch_at_login = before.settings.launch_at_login;
    tokio::task::spawn_blocking(move || {
        launch_at_login::configure(&home, &executable, persisted.launch_at_login)?;
        if let Err(error) = config::save(&config_dir, &persisted) {
            let _ = launch_at_login::configure(&home, &executable, previous_launch_at_login);
            return Err(error);
        }
        Ok(())
    })
    .await
    .map_err(|error| format!("join settings save: {error}"))??;
    {
        let mut current = state
            .snapshot
            .lock()
            .map_err(|_| "application state lock is poisoned".to_string())?;
        current.settings = settings;
    }
    publish(app);
    snapshot(state)
}

pub async fn apply(
    settings: AppSettings,
    token_action: String,
    token: String,
    app: &AppHandle,
    state: &AppState,
) -> Result<AppSnapshot, String> {
    let _settings_guard = state.settings_operation.lock().await;
    if snapshot(state)?.initialization.state == "running" {
        return Err("wait for workspace initialization before saving settings".into());
    }
    let previous_token = tokio::task::spawn_blocking(credentials::mineru_token)
        .await
        .map_err(|error| format!("join credential read: {error}"))??;
    let next_token = match token_action.as_str() {
        "unchanged" => previous_token.clone(),
        "set" => {
            let value = token.trim();
            if value.is_empty() {
                return Err("a non-empty MinerU token is required for the set operation".into());
            }
            Some(value.to_owned())
        }
        "clear" => None,
        _ => return Err("unknown MinerU token operation".into()),
    };
    if settings.mineru_mode == crate::models::MineruMode::Precision && next_token.is_none() {
        return Err("Precision mode requires a MinerU token in Keychain".into());
    }
    if token_action != "unchanged" {
        let credential = next_token.clone().unwrap_or_default();
        tokio::task::spawn_blocking(move || credentials::store_mineru_token(&credential))
            .await
            .map_err(|error| format!("join credential save: {error}"))??;
    }
    match persist_settings(settings, app, state).await {
        Ok(mut current) => {
            current.mineru_token_configured = next_token.is_some();
            {
                let mut snapshot = state
                    .snapshot
                    .lock()
                    .map_err(|_| "application state lock is poisoned".to_string())?;
                snapshot.mineru_token_configured = current.mineru_token_configured;
            }
            publish(app);
            snapshot(state)
        }
        Err(error) => {
            if token_action != "unchanged" {
                let rollback = previous_token.unwrap_or_default();
                let _ =
                    tokio::task::spawn_blocking(move || credentials::store_mineru_token(&rollback))
                        .await;
            }
            Err(error)
        }
    }
}

fn blocks_site_title_update(job: &jobs::JobRecord) -> bool {
    job.active_run.is_some()
        || !matches!(
            job.state,
            jobs::JobState::Succeeded | jobs::JobState::Failed | jobs::JobState::Cancelled
        )
        || (job.task_commit.is_some() && !job.remote_confirmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_title_waits_for_unfinished_task_publication() {
        let root = std::env::temp_dir().join(format!("cognitio-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("inbox")).unwrap();
        let source = root.join("inbox/paper.pdf");
        std::fs::write(&source, b"PDF").unwrap();
        let mut job = jobs::create(&root, &source, &AppSettings::default()).unwrap();
        assert!(blocks_site_title_update(&job));
        job.state = jobs::JobState::Blocked;
        job.task_commit = Some("unpublished".into());
        assert!(blocks_site_title_update(&job));
        job.state = jobs::JobState::Failed;
        assert!(blocks_site_title_update(&job));
        job.state = jobs::JobState::Succeeded;
        job.remote_confirmed = true;
        assert!(!blocks_site_title_update(&job));
        job.active_run = Some("finishing".into());
        assert!(blocks_site_title_update(&job));
        std::fs::remove_dir_all(root).unwrap();
    }
}
