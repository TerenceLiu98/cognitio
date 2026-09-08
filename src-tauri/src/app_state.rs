use crate::models::{AppSettings, AppSnapshot, InitializationSummary};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Manager};
use tokio::sync::watch;

pub struct AppState {
    pub settings_operation: tokio::sync::Mutex<()>,
    pub jobs: Arc<crate::job_runtime::JobRuntime>,
    pub snapshot: Mutex<AppSnapshot>,
    pub cancellation: Mutex<Option<watch::Sender<bool>>>,
    pub config_dir: PathBuf,
}

impl AppState {
    pub fn new(app: &AppHandle) -> Result<Self, String> {
        let config_dir = app
            .path()
            .app_config_dir()
            .map_err(|error| format!("resolve config directory: {error}"))?;
        Ok(Self {
            settings_operation: tokio::sync::Mutex::new(()),
            jobs: Arc::new(crate::job_runtime::JobRuntime::default()),
            snapshot: Mutex::new(AppSnapshot {
                revision: 0,
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
            config_dir,
        })
    }
}

pub fn snapshot(state: &AppState) -> Result<AppSnapshot, String> {
    state
        .snapshot
        .lock()
        .map(|value| value.clone())
        .map_err(|_| "application state lock is poisoned".into())
}
