use crate::{
    app_events::publish, app_state::AppState, config, credentials, initialization, logging, skill,
    tools, tray, workspace,
};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let config_dir = state.config_dir.clone();
        let home = match app.path().home_dir() {
            Ok(home) => home,
            Err(error) => {
                publish_bootstrap_error(&app, &format!("resolve home directory: {error}"));
                return;
            }
        };
        let loaded = tokio::task::spawn_blocking(move || {
            let mut settings = config::load(&config_dir)?;
            let mut initialization_journal = initialization::load(&config_dir)?;
            if settings.workspace_root.is_empty() {
                if let Some(journal) = &initialization_journal {
                    settings = journal.settings.clone();
                }
            }
            let configured = !settings.workspace_root.is_empty()
                && workspace::is_initialized(Path::new(&settings.workspace_root));
            if configured && initialization_journal.is_some() {
                initialization::clear(&config_dir)?;
                initialization_journal = None;
            }
            let initialization = initialization_journal
                .map(|journal| journal.summary)
                .unwrap_or_default();
            if configured {
                if let Err(error) = skill::install_all(&home) {
                    let _ = logging::append(
                        &config_dir,
                        "warn",
                        &format!("Could not update managed agent Skill: {error}"),
                        None,
                    );
                }
            }
            Ok::<_, String>((
                settings,
                configured,
                credentials::is_mineru_token_configured(),
                tools::detect_all(),
                logging::load(&config_dir),
                initialization,
            ))
        })
        .await;
        match loaded {
            Ok(Ok((settings, configured, token, tools, logs, initialization))) => {
                let jobs = if configured {
                    match crate::recovery::recover_all(Path::new(&settings.workspace_root)).await {
                        Ok(records) => records.iter().map(Into::into).collect(),
                        Err(error) => {
                            publish_bootstrap_error(&app, &error);
                            return;
                        }
                    }
                } else {
                    Vec::new()
                };

                let current = {
                    let Ok(mut snapshot) = state.snapshot.lock() else {
                        return;
                    };
                    snapshot.ready = true;
                    snapshot.configured = configured;
                    snapshot.watching = configured;
                    snapshot.settings = settings;
                    snapshot.jobs = jobs;
                    snapshot.mineru_token_configured = token;
                    snapshot.tools = tools;
                    snapshot.logs = logs;
                    snapshot.initialization = initialization;
                    snapshot.clone()
                };
                publish(&app);
                if current.configured {
                    crate::deployment::resume(
                        app.clone(),
                        PathBuf::from(&current.settings.workspace_root),
                    );
                }
                if !current.configured {
                    tray::show_window(&app, "setup");
                }
            }
            Ok(Err(error)) => publish_bootstrap_error(&app, &error),
            Err(error) => publish_bootstrap_error(&app, &format!("join startup loading: {error}")),
        }
    });
}

fn publish_bootstrap_error(app: &AppHandle, error: &str) {
    let state = app.state::<AppState>();
    {
        let Ok(mut snapshot) = state.snapshot.lock() else {
            return;
        };
        snapshot.ready = true;
        snapshot.initialization.state = "failed".into();
        snapshot.initialization.error = Some(error.into());
        snapshot.initialization.can_retry = false;
    }
    publish(app);
    tray::show_window(app, "setup");
}
