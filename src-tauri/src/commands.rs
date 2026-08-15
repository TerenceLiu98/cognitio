use std::{path::PathBuf, process::Command, sync::Mutex};

use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::{
    config, credentials, diagnostics, jobs, launch_at_login, logging,
    models::{AppSettings, AppSnapshot, PreflightItem, PreflightReport},
    skill, tools, tray, wiki, workspace,
};

pub struct AppState {
    pub snapshot: Mutex<AppSnapshot>,
    pub config_dir: PathBuf,
}

impl AppState {
    pub fn load(app: &AppHandle) -> Result<Self, String> {
        let config_dir = app
            .path()
            .app_config_dir()
            .map_err(|error| format!("resolve config directory: {error}"))?;
        let settings = config::load(&config_dir)?;
        let configured = !settings.workspace_root.is_empty()
            && workspace::is_initialized(PathBuf::from(&settings.workspace_root).as_path());
        let loaded_jobs = if configured {
            jobs::recover(PathBuf::from(&settings.workspace_root).as_path())
                .iter()
                .map(Into::into)
                .collect()
        } else {
            Vec::new()
        };
        Ok(Self {
            snapshot: Mutex::new(AppSnapshot {
                configured,
                watching: configured,
                mineru_token_configured: credentials::is_mineru_token_configured(),
                settings,
                tools: tools::detect_all(),
                jobs: loaded_jobs,
                logs: logging::load(&config_dir),
            }),
            config_dir,
        })
    }
}

fn snapshot(state: &State<'_, AppState>) -> Result<AppSnapshot, String> {
    state
        .snapshot
        .lock()
        .map(|value| value.clone())
        .map_err(|_| "application state lock is poisoned".into())
}

#[tauri::command]
pub fn get_app_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    snapshot(&state)
}

#[tauri::command]
pub fn save_settings(
    settings: AppSettings,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    if settings.schema_version != 1 {
        return Err("unsupported settings schema version".into());
    }
    let home = app
        .path()
        .home_dir()
        .map_err(|error| format!("resolve home directory: {error}"))?;
    let executable =
        std::env::current_exe().map_err(|error| format!("resolve executable: {error}"))?;
    launch_at_login::configure(&home, &executable, settings.launch_at_login)?;
    config::save(&state.config_dir, &settings)?;
    state
        .snapshot
        .lock()
        .map_err(|_| "application state lock is poisoned".to_string())?
        .settings = settings;
    let current = snapshot(&state)?;
    tray::refresh(&app, &current);
    Ok(current)
}

#[tauri::command]
pub fn save_mineru_token(
    token: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    credentials::store_mineru_token(token.trim())?;
    let entry = logging::append(&state.config_dir, "info", "MinerU credential updated", None)?;
    let value = {
        let mut current = state
            .snapshot
            .lock()
            .map_err(|_| "application state lock is poisoned".to_string())?;
        current.mineru_token_configured = !token.trim().is_empty();
        current.logs.push(entry);
        current.clone()
    };
    tray::refresh(&app, &value);
    Ok(value)
}

#[tauri::command]
pub async fn choose_workspace(app: AppHandle) -> Result<Option<String>, String> {
    app.dialog()
        .file()
        .set_title("Choose LLMWiki workspace")
        .blocking_pick_folder()
        .map(|path| {
            path.into_path()
                .map_err(|error| format!("invalid workspace path: {error}"))
        })
        .transpose()
        .map(|path| path.map(|value| value.to_string_lossy().into_owned()))
}

#[tauri::command]
pub fn initialize_workspace(
    settings: AppSettings,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let preflight = run_preflight(settings.clone());
    if !preflight.ready {
        let failed = preflight
            .items
            .into_iter()
            .filter(|item| !item.ok)
            .map(|item| item.message)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!("preflight failed: {failed}"));
    }
    let root = PathBuf::from(&settings.workspace_root);
    workspace::initialize(&root)?;
    wiki::initialize(
        &root,
        &settings.git_remote,
        &settings.repository_visibility,
        &settings.hosting_provider,
    )?;
    let home = app
        .path()
        .home_dir()
        .map_err(|error| format!("resolve home directory: {error}"))?;
    skill::install_all(&home)?;
    let executable =
        std::env::current_exe().map_err(|error| format!("resolve executable: {error}"))?;
    launch_at_login::configure(&home, &executable, settings.launch_at_login)?;
    config::save(&state.config_dir, &settings)?;
    let value = {
        let mut current = state
            .snapshot
            .lock()
            .map_err(|_| "application state lock is poisoned".to_string())?;
        current.settings = settings;
        current.configured = true;
        current.watching = true;
        current.clone()
    };
    tray::refresh(&app, &value);
    Ok(value)
}

#[tauri::command]
pub fn run_preflight(settings: AppSettings) -> PreflightReport {
    let mut items = Vec::new();
    let workspace_ok = !settings.workspace_root.is_empty() && {
        let path = PathBuf::from(&settings.workspace_root);
        !path.exists()
            || (path.is_dir()
                && path
                    .read_dir()
                    .is_ok_and(|mut entries| entries.next().is_none()))
            || workspace::is_initialized(&path)
    };
    items.push(PreflightItem {
        id: "workspace".into(),
        ok: workspace_ok,
        message: "Workspace is writable and empty".into(),
        detail: None,
    });

    let capabilities = tools::detect_all();
    for id in ["git", "node"] {
        let capability = capabilities.iter().find(|tool| tool.id == id);
        items.push(PreflightItem {
            id: id.into(),
            ok: capability.is_some_and(|tool| tool.detected),
            message: format!("{id} is installed"),
            detail: capability.and_then(|tool| tool.version.clone()),
        });
    }
    let agent_ok = match settings.agent_provider {
        crate::models::AgentProvider::Auto => capabilities.iter().any(|tool| {
            ["codex", "claude", "opencode"].contains(&tool.id.as_str()) && tool.detected
        }),
        crate::models::AgentProvider::Codex => capabilities
            .iter()
            .any(|tool| tool.id == "codex" && tool.detected),
        crate::models::AgentProvider::Claude => capabilities
            .iter()
            .any(|tool| tool.id == "claude" && tool.detected),
        crate::models::AgentProvider::Opencode => capabilities
            .iter()
            .any(|tool| tool.id == "opencode" && tool.detected),
    };
    items.push(PreflightItem {
        id: "agent".into(),
        ok: agent_ok,
        message: "Selected agent is installed".into(),
        detail: None,
    });
    let mineru_ok = !matches!(settings.mineru_mode, crate::models::MineruMode::Precision)
        || credentials::is_mineru_token_configured();
    items.push(PreflightItem {
        id: "mineru".into(),
        ok: mineru_ok,
        message: "MinerU credential is configured for the selected mode".into(),
        detail: None,
    });
    let remote_ok = wiki::validate_remote(settings.git_remote.trim()).is_ok();
    items.push(PreflightItem {
        id: "remote".into(),
        ok: remote_ok,
        message: "GitHub repository is configured".into(),
        detail: None,
    });
    let identity_ok = ["user.name", "user.email"].into_iter().all(|key| {
        Command::new("git")
            .args(["config", "--global", "--get", key])
            .output()
            .is_ok_and(|output| output.status.success() && !output.stdout.is_empty())
    });
    items.push(PreflightItem {
        id: "git-identity".into(),
        ok: identity_ok,
        message: "Git author identity is configured".into(),
        detail: None,
    });
    if !settings.git_remote.contains("://") && !settings.git_remote.starts_with("git@") {
        let gh_ok = capabilities
            .iter()
            .find(|tool| tool.id == "gh")
            .is_some_and(|tool| tool.detected && tool.authenticated == Some(true));
        items.push(PreflightItem {
            id: "github-auth".into(),
            ok: gh_ok,
            message: "GitHub CLI is authenticated".into(),
            detail: None,
        });
    }
    PreflightReport {
        ready: items.iter().all(|item| item.ok),
        items,
    }
}

#[tauri::command]
pub fn set_watching(
    watching: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let value = {
        let mut current = state
            .snapshot
            .lock()
            .map_err(|_| "application state lock is poisoned".to_string())?;
        if watching && !current.configured {
            return Err("workspace is not configured".into());
        }
        current.watching = watching;
        current.clone()
    };
    tray::refresh(&app, &value);
    Ok(value)
}

#[tauri::command]
pub fn retry_job(
    job_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    update_job(
        &job_id,
        jobs::JobState::Queued,
        "Waiting for processor",
        10,
        &app,
        &state,
    )
}

#[tauri::command]
pub fn cancel_job(
    job_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    update_job(
        &job_id,
        jobs::JobState::Cancelled,
        "Cancelled",
        0,
        &app,
        &state,
    )
}

fn update_job(
    job_id: &str,
    next: jobs::JobState,
    phase: &str,
    progress: u8,
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let workspace = snapshot(state)?.settings.workspace_root;
    let record = jobs::update(
        PathBuf::from(workspace).as_path(),
        job_id,
        next,
        phase,
        progress,
    )?;
    let mut current = state
        .snapshot
        .lock()
        .map_err(|_| "application state lock is poisoned".to_string())?;
    let summary = (&record).into();
    if let Some(existing) = current.jobs.iter_mut().find(|job| job.id == job_id) {
        *existing = summary;
    } else {
        current.jobs.insert(0, summary);
    }
    let value = current.clone();
    drop(current);
    let _ = app.emit("app-snapshot", value.clone());
    tray::refresh(app, &value);
    Ok(value)
}

#[tauri::command]
pub fn open_target(target: String, state: State<'_, AppState>) -> Result<(), String> {
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
        .spawn()
        .map_err(|error| format!("open target: {error}"))?;
    Ok(())
}

#[tauri::command]
pub async fn export_diagnostics(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let Some(path) = app
        .dialog()
        .file()
        .set_title("Export LLMWiki diagnostics")
        .set_file_name("llmwiki-diagnostics.zip")
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|error| format!("invalid diagnostics path: {error}"))?;
    diagnostics::export(&path, &snapshot(&state)?, &state.config_dir)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}
