use std::{path::PathBuf, process::Command};

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager,
};

use crate::commands::AppState;

pub fn install(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open LLMWiki", true, None::<&str>)?;
    let inbox = MenuItem::with_id(app, "inbox", "Open Inbox", true, None::<&str>)?;
    let toggle = MenuItem::with_id(
        app,
        "toggle",
        "Pause or Resume Watching",
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit LLMWiki", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &inbox, &toggle, &separator, &quit])?;
    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("LLMWiki")
        .icon_as_template(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_window(app),
            "inbox" => {
                if let Some(root) = workspace_root(app) {
                    let _ = Command::new("open").arg(root.join("inbox")).spawn();
                }
            }
            "toggle" => {
                let state = app.state::<AppState>();
                if let Ok(mut snapshot) = state.snapshot.lock() {
                    if snapshot.configured {
                        snapshot.watching = !snapshot.watching;
                        let _ = app.emit("app-snapshot", snapshot.clone());
                    }
                };
            }
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

fn show_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn workspace_root(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.state::<AppState>()
        .snapshot
        .lock()
        .ok()
        .and_then(|snapshot| {
            snapshot
                .configured
                .then(|| PathBuf::from(&snapshot.settings.workspace_root))
        })
}
