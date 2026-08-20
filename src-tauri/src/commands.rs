use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::Duration,
};

use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tokio::{
    process::Command,
    sync::{oneshot, watch},
};

use crate::{
    config, credentials, diagnostics, initialization,
    initialization::InitializationJournal,
    jobs, launch_at_login, logging,
    models::{AppSettings, AppSnapshot, InitializationSummary, PreflightItem, PreflightReport},
    skill, tools, tray, wiki, workspace,
};

pub struct AppState {
    pub snapshot: Mutex<AppSnapshot>,
    pub cancellation: Mutex<Option<watch::Sender<bool>>>,
    pub repository_operation: tokio::sync::Mutex<()>,
    pub config_dir: PathBuf,
}

impl AppState {
    pub fn new(app: &AppHandle) -> Result<Self, String> {
        let config_dir = app
            .path()
            .app_config_dir()
            .map_err(|error| format!("resolve config directory: {error}"))?;
        Ok(Self {
            snapshot: Mutex::new(AppSnapshot {
                ready: false,
                configured: false,
                watching: false,
                mineru_token_configured: false,
                settings: AppSettings::default(),
                tools: Vec::new(),
                jobs: Vec::new(),
                logs: Vec::new(),
                initialization: InitializationSummary::default(),
            }),
            cancellation: Mutex::new(None),
            repository_operation: tokio::sync::Mutex::new(()),
            config_dir,
        })
    }
}

pub fn bootstrap(app: AppHandle) {
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
            let loaded_jobs = if configured {
                jobs::recover(Path::new(&settings.workspace_root))
                    .iter()
                    .map(Into::into)
                    .collect()
            } else {
                Vec::new()
            };
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
                loaded_jobs,
                credentials::is_mineru_token_configured(),
                tools::detect_all(),
                logging::load(&config_dir),
                initialization,
            ))
        })
        .await;
        match loaded {
            Ok(Ok((settings, configured, jobs, token, tools, logs, initialization))) => {
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
                publish_snapshot(&app, current.clone());
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
    let current = {
        let Ok(mut snapshot) = state.snapshot.lock() else {
            return;
        };
        snapshot.ready = true;
        snapshot.initialization.state = "failed".into();
        snapshot.initialization.error = Some(error.into());
        snapshot.initialization.can_retry = false;
        snapshot.clone()
    };
    publish_snapshot(app, current);
    tray::show_window(app, "setup");
}

fn snapshot(state: &State<'_, AppState>) -> Result<AppSnapshot, String> {
    state
        .snapshot
        .lock()
        .map(|value| value.clone())
        .map_err(|_| "application state lock is poisoned".into())
}

fn publish_snapshot(app: &AppHandle, value: AppSnapshot) {
    let _ = app.emit("app-snapshot", value.clone());
    tray::refresh(app, &value);
}

#[tauri::command]
pub async fn get_app_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    snapshot(&state)
}

#[tauri::command]
pub async fn save_settings(
    mut settings: AppSettings,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    if settings.schema_version != 3 {
        return Err("unsupported settings schema version".into());
    }
    settings.site_title = wiki::normalize_site_title(&settings.site_title)?;
    let before = snapshot(&state)?;
    if before.configured && before.settings.site_title != settings.site_title {
        let _repository_guard = state.repository_operation.lock().await;
        let current = snapshot(&state)?;
        let busy = current.jobs.iter().any(|job| {
            matches!(
                job.state.as_str(),
                "detected" | "stabilizing" | "queued" | "preflight" | "running" | "verifying"
            )
        });
        if busy {
            return Err("wait for the active paper job before changing the site title".into());
        }
        wiki::update_site_title(
            Path::new(&current.settings.workspace_root),
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
    tokio::task::spawn_blocking(move || {
        launch_at_login::configure(&home, &executable, persisted.launch_at_login)?;
        config::save(&config_dir, &persisted)
    })
    .await
    .map_err(|error| format!("join settings save: {error}"))??;
    let current = {
        let mut current = state
            .snapshot
            .lock()
            .map_err(|_| "application state lock is poisoned".to_string())?;
        current.settings = settings;
        current.clone()
    };
    publish_snapshot(&app, current.clone());
    Ok(current)
}

#[tauri::command]
pub async fn save_mineru_token(
    token: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let trimmed = token.trim().to_owned();
    let config_dir = state.config_dir.clone();
    let entry = tokio::task::spawn_blocking(move || {
        credentials::store_mineru_token(&trimmed)?;
        logging::append(&config_dir, "info", "MinerU credential updated", None)
    })
    .await
    .map_err(|error| format!("join credential save: {error}"))??;
    let current = {
        let mut current = state
            .snapshot
            .lock()
            .map_err(|_| "application state lock is poisoned".to_string())?;
        current.mineru_token_configured = !token.trim().is_empty();
        current.logs.push(entry);
        current.clone()
    };
    publish_snapshot(&app, current.clone());
    Ok(current)
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
pub async fn initialize_workspace(
    mut settings: AppSettings,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    if settings.schema_version != 3 {
        return Err("unsupported settings schema version".into());
    }
    settings.site_title = wiki::normalize_site_title(&settings.site_title)?;
    wiki::validate_remote(&settings.git_remote)?;
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

#[tauri::command]
pub async fn cancel_initialization(
    app: AppHandle,
    state: State<'_, AppState>,
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
    publish_snapshot(&app, current.clone());
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
            if let Some(current) = current {
                publish_snapshot(&app, current);
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
    publish_snapshot(app, current.clone());
    Ok(current)
}

#[tauri::command]
pub async fn run_preflight(settings: AppSettings) -> PreflightReport {
    run_preflight_inner(&settings).await
}

async fn run_preflight_inner(settings: &AppSettings) -> PreflightReport {
    let workspace_root = settings.workspace_root.clone();
    let mineru_mode = settings.mineru_mode.clone();
    let detected = tokio::task::spawn_blocking(move || {
        let path = PathBuf::from(&workspace_root);
        let workspace_ok =
            !workspace_root.is_empty() && (!path.exists() || workspace::can_initialize(&path));
        (
            workspace_ok,
            tools::detect_all(),
            !matches!(mineru_mode, crate::models::MineruMode::Precision)
                || credentials::is_mineru_token_configured(),
        )
    })
    .await
    .unwrap_or((false, Vec::new(), false));
    let (workspace_ok, capabilities, mineru_ok) = detected;
    let mut items = vec![PreflightItem {
        id: "workspace".into(),
        ok: workspace_ok,
        message: "Workspace is writable and empty or resumable".into(),
        detail: None,
    }];
    let git = capabilities.iter().find(|tool| tool.id == "git");
    items.push(PreflightItem {
        id: "git".into(),
        ok: git.is_some_and(|tool| tool.detected),
        message: "git is installed".into(),
        detail: git.and_then(|tool| tool.version.clone()),
    });
    let gh = capabilities.iter().find(|tool| tool.id == "gh");
    items.push(PreflightItem {
        id: "github-auth".into(),
        ok: gh.is_some_and(|tool| tool.detected && tool.authenticated == Some(true)),
        message: "GitHub CLI is authenticated".into(),
        detail: gh.and_then(|tool| tool.version.clone()),
    });
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
    items.push(PreflightItem {
        id: "mineru".into(),
        ok: mineru_ok,
        message: "MinerU credential is configured for the selected mode".into(),
        detail: None,
    });
    items.push(PreflightItem {
        id: "remote".into(),
        ok: wiki::validate_remote(settings.git_remote.trim()).is_ok(),
        message: "GitHub repository is configured".into(),
        detail: None,
    });
    let mut identity_ok = true;
    for key in ["user.name", "user.email"] {
        identity_ok &= command_has_output("git", &["config", "--global", "--get", key]).await;
    }
    items.push(PreflightItem {
        id: "git-identity".into(),
        ok: identity_ok,
        message: "Git author identity is configured".into(),
        detail: None,
    });
    PreflightReport {
        ready: items.iter().all(|item| item.ok),
        items,
    }
}

async fn command_has_output(executable: &str, args: &[&str]) -> bool {
    tokio::time::timeout(
        Duration::from_secs(15),
        Command::new(executable)
            .args(args)
            .stdin(Stdio::null())
            .output(),
    )
    .await
    .is_ok_and(|output| {
        output.is_ok_and(|output| output.status.success() && !output.stdout.is_empty())
    })
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
    publish_snapshot(&app, current.clone());
    Ok(current)
}

#[tauri::command]
pub async fn retry_job(
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
    .await
}

#[tauri::command]
pub async fn cancel_job(
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
    .await
}

async fn update_job(
    job_id: &str,
    next: jobs::JobState,
    phase: &str,
    progress: u8,
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let workspace = snapshot(state)?.settings.workspace_root;
    let id = job_id.to_owned();
    let phase = phase.to_owned();
    let record = tokio::task::spawn_blocking(move || {
        jobs::update(Path::new(&workspace), &id, next, &phase, progress)
    })
    .await
    .map_err(|error| format!("join job update: {error}"))??;
    let current = {
        let mut value = state
            .snapshot
            .lock()
            .map_err(|_| "application state lock is poisoned".to_string())?;
        let summary = (&record).into();
        if let Some(existing) = value.jobs.iter_mut().find(|job| job.id == job_id) {
            *existing = summary;
        } else {
            value.jobs.insert(0, summary);
        }
        value.clone()
    };
    publish_snapshot(app, current.clone());
    Ok(current)
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
pub async fn export_diagnostics(
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
    tokio::task::spawn_blocking(move || diagnostics::export(&destination, &current, &config_dir))
        .await
        .map_err(|error| format!("join diagnostics export: {error}"))??;
    Ok(Some(path.to_string_lossy().into_owned()))
}
