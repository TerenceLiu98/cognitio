use crate::{
    app_state::AppState, job_runtime::RuntimeEvent, jobs::JobRecord, logging, models::AppSnapshot,
    tray,
};
use std::path::Path;
use std::process::Stdio;
use tauri::{AppHandle, Emitter, Manager};
use tokio::process::Command;

pub fn publish(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Ok(mut snapshot) = state.snapshot.lock() else {
        return;
    };
    snapshot.revision += 1;
    let _ = app.emit("app-snapshot", snapshot.clone());
    tray::refresh(app, &snapshot);
}

pub fn listen(app: AppHandle) {
    let mut events = app.state::<AppState>().jobs.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(RuntimeEvent::JobUpdated { workspace, record }) => {
                    let matches = app
                        .state::<AppState>()
                        .snapshot
                        .lock()
                        .is_ok_and(|snapshot| {
                            Path::new(&snapshot.settings.workspace_root) == workspace
                        });
                    if matches {
                        publish_record(&app, &record);
                    }
                }
                Ok(RuntimeEvent::Log {
                    level,
                    message,
                    job_id,
                }) => append_log(&app, &level, &message, Some(&job_id)).await,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let state = app.state::<AppState>();
                    let root = state
                        .snapshot
                        .lock()
                        .ok()
                        .map(|snapshot| snapshot.settings.workspace_root.clone());
                    if let Some(root) = root {
                        if let Ok(Ok(records)) = tokio::task::spawn_blocking(move || {
                            crate::jobs::load_all(Path::new(&root))
                        })
                        .await
                        {
                            for record in records {
                                publish_record(&app, &record);
                            }
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

pub fn publish_record(app: &AppHandle, record: &JobRecord) {
    let state = app.state::<AppState>();
    let Ok(mut snapshot) = state.snapshot.lock() else {
        return;
    };
    let summary = record.into();
    if let Some(existing) = snapshot.jobs.iter_mut().find(|job| job.id == record.id) {
        if existing.revision > record.revision {
            return;
        }
        *existing = summary;
    } else {
        snapshot.jobs.insert(0, summary);
    }
    drop(snapshot);
    publish(app);
}

pub async fn append_log(app: &AppHandle, level: &str, message: &str, job_id: Option<&str>) {
    let app = app.clone();
    let level = level.to_owned();
    let message = message.to_owned();
    let job_id = job_id.map(str::to_owned);
    let _ = tokio::task::spawn_blocking(move || {
        append_log_blocking(&app, &level, &message, job_id.as_deref());
    })
    .await;
}

// Call only from the blocking pool, including the file watcher and scheduler lookup.
pub fn append_log_blocking(app: &AppHandle, level: &str, message: &str, job_id: Option<&str>) {
    let state = app.state::<AppState>();
    if let Ok(entry) = logging::append(&state.config_dir, level, message, job_id) {
        if let Ok(mut snapshot) = state.snapshot.lock() {
            snapshot.logs.push(entry);
            trim_snapshot_logs(&mut snapshot);
        }
    }
    publish(app);
}

fn trim_snapshot_logs(snapshot: &mut AppSnapshot) {
    const MAX_LOGS: usize = 200;
    if snapshot.logs.len() > MAX_LOGS {
        snapshot.logs.drain(..snapshot.logs.len() - MAX_LOGS);
    }
}

pub fn notify(title: &str, body: &str) {
    let script =
        "on run argv\ndisplay notification (item 2 of argv) with title (item 1 of argv)\nend run";
    let title = title.to_owned();
    let body = body.to_owned();
    tauri::async_runtime::spawn(async move {
        let _ = Command::new("osascript")
            .args(["-e", script, &title, &body])
            .stdin(Stdio::null())
            .status()
            .await;
    });
}
