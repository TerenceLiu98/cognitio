use crate::{
    app_events::publish,
    app_state::{snapshot, AppState},
    diagnostics,
    models::{AppSettings, AppSnapshot, PreflightReport},
    tray,
};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tokio::{process::Command, sync::oneshot};

#[tauri::command]
pub async fn apply_settings(
    settings: AppSettings,
    token_action: String,
    token: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    crate::settings::apply(settings, token_action, token, &app, &state).await
}

#[tauri::command]
pub async fn initialize_workspace(
    settings: AppSettings,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    crate::setup::initialize_workspace(settings, app, &state).await
}

#[tauri::command]
pub async fn cancel_initialization(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    crate::setup::cancel_initialization(app, &state).await
}

#[tauri::command]
pub async fn run_preflight(settings: AppSettings) -> PreflightReport {
    crate::preflight::run(&settings).await
}

#[tauri::command]
pub async fn get_app_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    snapshot(&state)
}

#[tauri::command]
pub async fn choose_workspace(app: AppHandle) -> Result<Option<String>, String> {
    let (sender, receiver) = oneshot::channel();
    app.dialog()
        .file()
        .set_title("Choose Cognitio workspace")
        .pick_folder(move |path| {
            let _ = sender.send(path);
        });
    receiver
        .await
        .map_err(|_| "workspace dialog closed unexpectedly".to_string())?
        .map(|path| {
            path.into_path()
                .map_err(|error| format!("invalid workspace path: {error}"))
        })
        .transpose()
        .map(|path| path.map(|value| value.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn set_watching(
    watching: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let current = {
        let mut value = state
            .snapshot
            .lock()
            .map_err(|_| "application state lock is poisoned".to_string())?;
        if watching && !value.configured {
            return Err("workspace is not configured".into());
        }
        value.watching = watching;
        value.clone()
    };
    publish(&app);
    Ok(current)
}

#[tauri::command]
pub async fn retry_job(
    job_id: String,
    retry_mode: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let force_reparse = match retry_mode.as_str() {
        "reuse_valid" => false,
        "force_reparse_current_mode" => true,
        _ => return Err("unknown retry mode".into()),
    };
    let current = snapshot(&state)?;
    let workspace = current.settings.workspace_root.clone();
    let settings = current.settings;
    let id = job_id.clone();
    let record = state
        .jobs
        .retry(Path::new(&workspace), &id, &settings, force_reparse)
        .await?;
    crate::app_events::publish_record(&app, &record);
    snapshot(&state)
}

#[tauri::command]
pub async fn cancel_job(
    job_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let workspace = snapshot(&state)?.settings.workspace_root;
    let id = job_id.to_owned();
    let runtime = state.jobs.clone();
    let record = tokio::task::spawn_blocking(move || runtime.cancel(Path::new(&workspace), &id))
        .await
        .map_err(|error| format!("join job update: {error}"))??;
    crate::app_events::publish_record(&app, &record);
    snapshot(&state)
}

#[tauri::command]
pub async fn open_target(target: String, state: State<'_, AppState>) -> Result<(), String> {
    let current = snapshot(&state)?;
    let value = match target.as_str() {
        "inbox" => PathBuf::from(&current.settings.workspace_root)
            .join("inbox")
            .to_string_lossy()
            .into_owned(),
        "wiki" => PathBuf::from(&current.settings.workspace_root)
            .join("wiki")
            .to_string_lossy()
            .into_owned(),
        "website" if !current.settings.site_url.is_empty() => current.settings.site_url,
        _ => return Err("unknown or unavailable target".into()),
    };
    Command::new("open")
        .arg(value)
        .stdin(Stdio::null())
        .spawn()
        .map_err(|error| format!("open target: {error}"))?;
    Ok(())
}

#[tauri::command]
pub fn show_app_view(view: String, app: AppHandle) -> Result<(), String> {
    if !matches!(view.as_str(), "overview" | "setup" | "settings" | "logs") {
        return Err("unknown application view".into());
    }
    tray::show_window(&app, &view);
    if let Some(panel) = app.get_webview_window("menubar") {
        let _ = panel.hide();
    }
    Ok(())
}

#[tauri::command]
pub fn hide_menu_panel(app: AppHandle) {
    if let Some(panel) = app.get_webview_window("menubar") {
        let _ = panel.hide();
    }
}

#[tauri::command]
pub async fn export_diagnostics(
    include_detailed_logs: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let (sender, receiver) = oneshot::channel();
    app.dialog()
        .file()
        .set_title("Export Cognitio diagnostics")
        .set_file_name("cognitio-diagnostics.zip")
        .save_file(move |path| {
            let _ = sender.send(path);
        });
    let Some(path) = receiver
        .await
        .map_err(|_| "diagnostics dialog closed unexpectedly".to_string())?
    else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|error| format!("invalid diagnostics path: {error}"))?;
    let current = snapshot(&state)?;
    let config_dir = state.config_dir.clone();
    let destination = path.clone();
    tokio::task::spawn_blocking(move || {
        diagnostics::export(&destination, &current, &config_dir, include_detailed_logs)
    })
    .await
    .map_err(|error| format!("join diagnostics export: {error}"))??;
    Ok(Some(path.to_string_lossy().into_owned()))
}
