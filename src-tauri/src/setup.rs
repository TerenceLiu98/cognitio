use crate::{
    app_events::publish,
    app_state::{snapshot, AppState},
    config,
    initialization::{self, InitializationJournal},
    launch_at_login,
    models::{AppSettings, AppSnapshot, InitializationSummary},
    preflight::run as run_preflight_inner,
    skill, wiki, workspace,
};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use tokio::sync::watch;

pub async fn initialize_workspace(
    mut settings: AppSettings,
    app: AppHandle,
    state: &AppState,
) -> Result<AppSnapshot, String> {
    let _settings_guard = state.settings_operation.lock().await;
    if settings.schema_version != 3 {
        return Err("unsupported settings schema version".into());
    }
    settings.site_title = wiki::normalize_site_title(&settings.site_title)?;
    wiki::validate_remote(&settings.git_remote)?;
    if snapshot(state)?.configured {
        return Err("workspace is already configured".into());
    }
    let operation_id = uuid::Uuid::new_v4().to_string();
    let (sender, receiver) = watch::channel(false);
    {
        let mut cancellation = state
            .cancellation
            .lock()
            .map_err(|_| "initialization cancellation lock is poisoned".to_string())?;
        if cancellation.is_some() {
            return Err("workspace initialization is already running".into());
        }
        *cancellation = Some(sender);
    }
    let summary = InitializationSummary {
        operation_id: Some(operation_id),
        state: "running".into(),
        phase: Some("preflight".into()),
        progress: 5,
        message: Some("Checking prerequisites".into()),
        error: None,
        can_retry: false,
    };
    let current = match set_initialization(&app, &settings, summary).await {
        Ok(current) => current,
        Err(error) => {
            if let Ok(mut cancellation) = state.cancellation.lock() {
                *cancellation = None;
            }
            return Err(error);
        }
    };
    tauri::async_runtime::spawn(run_initialization(app.clone(), settings, receiver));
    Ok(current)
}

pub async fn cancel_initialization(
    app: AppHandle,
    state: &AppState,
) -> Result<AppSnapshot, String> {
    let sender = state
        .cancellation
        .lock()
        .map_err(|_| "initialization cancellation lock is poisoned".to_string())?
        .clone()
        .ok_or_else(|| "no initialization is running".to_string())?;
    sender
        .send(true)
        .map_err(|_| "initialization task is no longer running".to_string())?;
    let current = {
        let mut value = state
            .snapshot
            .lock()
            .map_err(|_| "application state lock is poisoned".to_string())?;
        value.initialization.message = Some("Cancelling initialization".into());
        value.clone()
    };
    publish(&app);
    Ok(current)
}

async fn run_initialization(
    app: AppHandle,
    mut settings: AppSettings,
    mut cancel: watch::Receiver<bool>,
) {
    let result = run_initialization_steps(&app, &mut settings, &mut cancel).await;
    let state = app.state::<AppState>();
    if let Ok(mut cancellation) = state.cancellation.lock() {
        *cancellation = None;
    }
    match result {
        Ok(()) => {
            let summary = InitializationSummary {
                operation_id: snapshot(&state)
                    .ok()
                    .and_then(|value| value.initialization.operation_id),
                state: "succeeded".into(),
                phase: Some("finalizing".into()),
                progress: 100,
                message: Some("Workspace is ready".into()),
                error: None,
                can_retry: false,
            };
            let config_dir = state.config_dir.clone();
            let _ = tokio::task::spawn_blocking(move || initialization::clear(&config_dir)).await;
            let current = if let Ok(mut current) = state.snapshot.lock() {
                current.settings = settings;
                current.configured = true;
                current.watching = true;
                current.initialization = summary;
                Some(current.clone())
            } else {
                None
            };
            if current.is_some() {
                publish(&app);
            }
        }
        Err(error) => {
            let cancelled = error == "initialization cancelled" || *cancel.borrow();
            let current_summary = snapshot(&state)
                .ok()
                .map(|value| value.initialization)
                .unwrap_or_default();
            let summary = InitializationSummary {
                operation_id: current_summary.operation_id,
                state: if cancelled { "cancelled" } else { "failed" }.into(),
                phase: current_summary.phase,
                progress: current_summary.progress,
                message: Some(
                    if cancelled {
                        "Initialization cancelled"
                    } else {
                        "Initialization failed"
                    }
                    .into(),
                ),
                error: (!cancelled).then_some(error),
                can_retry: true,
            };
            let _ = set_initialization(&app, &settings, summary).await;
        }
    }
}

async fn run_initialization_steps(
    app: &AppHandle,
    settings: &mut AppSettings,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(), String> {
    let report = run_preflight_inner(settings).await;
    if !report.ready {
        return Err(format!(
            "preflight failed: {}",
            report
                .items
                .into_iter()
                .filter(|item| !item.ok)
                .map(|item| item.message)
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    check_cancel(cancel)?;
    update_phase(
        app,
        settings,
        "workspace",
        15,
        "Creating workspace directories",
    )
    .await?;
    let root = PathBuf::from(&settings.workspace_root);
    let workspace_root = root.clone();
    tokio::task::spawn_blocking(move || workspace::initialize(&workspace_root))
        .await
        .map_err(|error| format!("join workspace creation: {error}"))??;

    check_cancel(cancel)?;
    let slug = wiki::repository_slug(&settings.git_remote)?;
    update_phase(
        app,
        settings,
        "template",
        30,
        "Installing bundled Quartz template",
    )
    .await?;
    wiki::install_template(&root, &slug, &settings.site_title).await?;

    check_cancel(cancel)?;
    update_phase(
        app,
        settings,
        "repository",
        50,
        "Initializing Git repository",
    )
    .await?;
    wiki::initialize_git(&root, cancel).await?;
    let slug = wiki::configure_repository(
        &root,
        &settings.git_remote,
        &settings.repository_visibility,
        cancel,
    )
    .await?;

    check_cancel(cancel)?;
    update_phase(app, settings, "pages", 68, "Enabling GitHub Pages").await?;
    wiki::configure_pages(&slug, &root, cancel).await?;
    update_phase(app, settings, "pages", 78, "Pushing the main branch").await?;
    wiki::push_main(&root, cancel).await?;
    if settings.site_url.trim().is_empty() {
        settings.site_url = wiki::pages_url(&slug);
    }

    check_cancel(cancel)?;
    update_phase(app, settings, "skills", 88, "Installing agent skills").await?;
    let home = app
        .path()
        .home_dir()
        .map_err(|error| format!("resolve home directory: {error}"))?;
    let executable =
        std::env::current_exe().map_err(|error| format!("resolve executable: {error}"))?;
    let launch = settings.launch_at_login;
    tokio::task::spawn_blocking(move || {
        skill::install_all(&home)?;
        launch_at_login::configure(&home, &executable, launch)
    })
    .await
    .map_err(|error| format!("join local integration setup: {error}"))??;

    check_cancel(cancel)?;
    update_phase(
        app,
        settings,
        "finalizing",
        96,
        "Saving workspace configuration",
    )
    .await?;
    let config_dir = app.state::<AppState>().config_dir.clone();
    let persisted = settings.clone();
    let marker_root = root.clone();
    tokio::task::spawn_blocking(move || {
        config::save(&config_dir, &persisted)?;
        workspace::mark_complete(&marker_root)
    })
    .await
    .map_err(|error| format!("join initialization finalization: {error}"))??;
    Ok(())
}

fn check_cancel(cancel: &watch::Receiver<bool>) -> Result<(), String> {
    if *cancel.borrow() {
        Err("initialization cancelled".into())
    } else {
        Ok(())
    }
}

async fn update_phase(
    app: &AppHandle,
    settings: &AppSettings,
    phase: &str,
    progress: u8,
    message: &str,
) -> Result<AppSnapshot, String> {
    let operation_id = app
        .state::<AppState>()
        .snapshot
        .lock()
        .ok()
        .and_then(|value| value.initialization.operation_id.clone());
    set_initialization(
        app,
        settings,
        InitializationSummary {
            operation_id,
            state: "running".into(),
            phase: Some(phase.into()),
            progress,
            message: Some(message.into()),
            error: None,
            can_retry: false,
        },
    )
    .await
}

async fn set_initialization(
    app: &AppHandle,
    settings: &AppSettings,
    summary: InitializationSummary,
) -> Result<AppSnapshot, String> {
    let state = app.state::<AppState>();
    let config_dir = state.config_dir.clone();
    let journal = InitializationJournal {
        workspace_root: PathBuf::from(&settings.workspace_root),
        settings: settings.clone(),
        summary: summary.clone(),
    };
    tokio::task::spawn_blocking(move || initialization::save(&config_dir, &journal))
        .await
        .map_err(|error| format!("join initialization journal save: {error}"))??;
    let current = {
        let mut value = state
            .snapshot
            .lock()
            .map_err(|_| "application state lock is poisoned".to_string())?;
        value.settings = settings.clone();
        value.initialization = summary;
        value.clone()
    };
    publish(app);
    Ok(current)
}
