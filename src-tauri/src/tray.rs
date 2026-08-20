use std::path::PathBuf;

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager, Wry,
};

use crate::{
    commands::AppState,
    models::{AppSnapshot, Locale},
    wiki,
};

struct TrayState {
    status: MenuItem<Wry>,
    job: MenuItem<Wry>,
    inbox: MenuItem<Wry>,
    wiki: MenuItem<Wry>,
    repository: MenuItem<Wry>,
    toggle: MenuItem<Wry>,
    settings: MenuItem<Wry>,
    logs: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

#[derive(Debug, Eq, PartialEq)]
struct TrayPresentation {
    status: String,
    job: String,
    inbox: &'static str,
    wiki: &'static str,
    repository: &'static str,
    toggle: &'static str,
    settings: &'static str,
    logs: &'static str,
    quit: &'static str,
    configured: bool,
    repository_available: bool,
}

pub fn install(app: &tauri::App) -> tauri::Result<()> {
    let snapshot = app.state::<AppState>().snapshot.lock().map_or_else(
        |_| AppSnapshot::empty_for_tray(),
        |snapshot| snapshot.clone(),
    );
    let presentation = presentation(&snapshot);
    let status = MenuItem::with_id(app, "status", &presentation.status, false, None::<&str>)?;
    let job = MenuItem::with_id(app, "job", &presentation.job, false, None::<&str>)?;
    let inbox = MenuItem::with_id(
        app,
        "inbox",
        presentation.inbox,
        presentation.configured,
        None::<&str>,
    )?;
    let wiki_item = MenuItem::with_id(
        app,
        "wiki",
        presentation.wiki,
        presentation.configured,
        None::<&str>,
    )?;
    let repository = MenuItem::with_id(
        app,
        "repository",
        presentation.repository,
        presentation.repository_available,
        None::<&str>,
    )?;
    let toggle = MenuItem::with_id(
        app,
        "toggle",
        presentation.toggle,
        presentation.configured,
        None::<&str>,
    )?;
    let settings = MenuItem::with_id(app, "settings", presentation.settings, true, None::<&str>)?;
    let logs = MenuItem::with_id(app, "logs", presentation.logs, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", presentation.quit, true, None::<&str>)?;
    let first_separator = PredefinedMenuItem::separator(app)?;
    let second_separator = PredefinedMenuItem::separator(app)?;
    let third_separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &job,
            &first_separator,
            &inbox,
            &wiki_item,
            &repository,
            &second_separator,
            &toggle,
            &settings,
            &logs,
            &third_separator,
            &quit,
        ],
    )?;
    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip("Cognitio")
        .icon_as_template(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "inbox" => open_workspace_path(app, "inbox"),
            "wiki" => open_workspace_path(app, "wiki"),
            "repository" => open_repository(app),
            "toggle" => toggle_watching(app),
            "settings" => show_window(app, settings_view(app)),
            "logs" => show_window(app, "logs"),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    app.manage(TrayState {
        status,
        job,
        inbox,
        wiki: wiki_item,
        repository,
        toggle,
        settings,
        logs,
        quit,
    });
    Ok(())
}

pub fn refresh(app: &tauri::AppHandle, snapshot: &AppSnapshot) {
    let Some(items) = app.try_state::<TrayState>() else {
        return;
    };
    let presentation = presentation(snapshot);
    let _ = items.status.set_text(&presentation.status);
    let _ = items.job.set_text(&presentation.job);
    let _ = items.inbox.set_text(presentation.inbox);
    let _ = items.inbox.set_enabled(presentation.configured);
    let _ = items.wiki.set_text(presentation.wiki);
    let _ = items.wiki.set_enabled(presentation.configured);
    let _ = items.repository.set_text(presentation.repository);
    let _ = items
        .repository
        .set_enabled(presentation.repository_available);
    let _ = items.toggle.set_text(presentation.toggle);
    let _ = items.toggle.set_enabled(presentation.configured);
    let _ = items.settings.set_text(presentation.settings);
    let _ = items.logs.set_text(presentation.logs);
    let _ = items.quit.set_text(presentation.quit);
}

pub fn show_window(app: &tauri::AppHandle, view: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        let _ = app.emit_to("main", "navigate-view", view);
    }
}

fn presentation(snapshot: &AppSnapshot) -> TrayPresentation {
    let zh = matches!(snapshot.settings.locale, Locale::Zh);
    let inbox_path = PathBuf::from(&snapshot.settings.workspace_root).join("inbox");
    let status = if !snapshot.configured {
        if zh {
            "○ 需要完成设置".into()
        } else {
            "○ Setup required".into()
        }
    } else if snapshot.watching {
        if zh {
            format!("● 正在监听 {}", inbox_path.display())
        } else {
            format!("● Watching {}", inbox_path.display())
        }
    } else if zh {
        format!("Ⅱ 已暂停 {}", inbox_path.display())
    } else {
        format!("Ⅱ Paused {}", inbox_path.display())
    };
    let active_job = snapshot
        .jobs
        .iter()
        .find(|job| matches!(job.state.as_str(), "preflight" | "running" | "verifying"))
        .or_else(|| snapshot.jobs.iter().find(|job| job.state == "queued"));
    let job = active_job.map_or_else(
        || {
            if zh {
                "无活动任务".into()
            } else {
                "No active tasks".into()
            }
        },
        |job| {
            if zh {
                format!("正在处理：{} · {}", job.filename, job.phase)
            } else {
                format!("Processing: {} · {}", job.filename, job.phase)
            }
        },
    );
    TrayPresentation {
        status,
        job,
        inbox: if zh { "打开收件箱" } else { "Open Inbox" },
        wiki: if zh { "打开 Wiki" } else { "Open Wiki" },
        repository: if zh {
            "打开代码仓库"
        } else {
            "Open Repository"
        },
        toggle: if snapshot.watching {
            if zh {
                "暂停监听"
            } else {
                "Pause Watching"
            }
        } else if zh {
            "恢复监听"
        } else {
            "Resume Watching"
        },
        settings: if !snapshot.configured {
            if zh {
                "完成设置…"
            } else {
                "Setup…"
            }
        } else if zh {
            "设置…"
        } else {
            "Settings…"
        },
        logs: if zh { "查看日志" } else { "View Logs" },
        quit: if zh {
            "退出 Cognitio"
        } else {
            "Quit Cognitio"
        },
        configured: snapshot.configured,
        repository_available: repository_url(&snapshot.settings.git_remote).is_some(),
    }
}

fn toggle_watching(app: &tauri::AppHandle) {
    let current = {
        let state = app.state::<AppState>();
        let Ok(mut snapshot) = state.snapshot.lock() else {
            return;
        };
        if !snapshot.configured {
            return;
        }
        snapshot.watching = !snapshot.watching;
        snapshot.clone()
    };
    let _ = app.emit("app-snapshot", current.clone());
    refresh(app, &current);
}

fn open_workspace_path(app: &tauri::AppHandle, directory: &str) {
    let root = app
        .state::<AppState>()
        .snapshot
        .lock()
        .ok()
        .and_then(|snapshot| {
            snapshot
                .configured
                .then(|| PathBuf::from(&snapshot.settings.workspace_root))
        });
    if let Some(root) = root {
        let target = root.join(directory);
        tauri::async_runtime::spawn(async move {
            let _ = tokio::process::Command::new("open")
                .arg(target)
                .status()
                .await;
        });
    }
}

fn open_repository(app: &tauri::AppHandle) {
    let url = app
        .state::<AppState>()
        .snapshot
        .lock()
        .ok()
        .and_then(|snapshot| repository_url(&snapshot.settings.git_remote));
    if let Some(url) = url {
        tauri::async_runtime::spawn(async move {
            let _ = tokio::process::Command::new("open").arg(url).status().await;
        });
    }
}

fn settings_view(app: &tauri::AppHandle) -> &'static str {
    app.state::<AppState>()
        .snapshot
        .lock()
        .map(|snapshot| {
            if snapshot.configured {
                "settings"
            } else {
                "setup"
            }
        })
        .unwrap_or("setup")
}

fn repository_url(remote: &str) -> Option<String> {
    let remote = remote.trim();
    wiki::validate_remote(remote).ok()?;
    let path = remote
        .strip_prefix("https://github.com/")
        .or_else(|| remote.strip_prefix("git@github.com:"))
        .unwrap_or(remote);
    let path = path.strip_suffix(".git").unwrap_or(path);
    Some(format!("https://github.com/{path}"))
}

impl AppSnapshot {
    fn empty_for_tray() -> Self {
        Self {
            ready: false,
            configured: false,
            watching: false,
            mineru_token_configured: false,
            settings: Default::default(),
            tools: Vec::new(),
            jobs: Vec::new(),
            logs: Vec::new(),
            initialization: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{AppSettings, JobSummary};

    use super::*;

    #[test]
    fn builds_localized_dynamic_menu_text() {
        let mut snapshot = AppSnapshot::empty_for_tray();
        assert_eq!(presentation(&snapshot).settings, "Setup…");
        snapshot.configured = true;
        snapshot.watching = true;
        snapshot.settings = AppSettings {
            locale: Locale::Zh,
            workspace_root: "/tmp/论文".into(),
            git_remote: "owner/repo".into(),
            ..AppSettings::default()
        };
        snapshot.jobs.push(JobSummary {
            id: "job".into(),
            filename: "paper.pdf".into(),
            state: "running".into(),
            phase: "Codex".into(),
            progress: 50,
            agent: Some("codex".into()),
            created_at: String::new(),
            updated_at: String::new(),
            error: None,
        });
        snapshot.jobs.insert(
            0,
            JobSummary {
                id: "queued".into(),
                filename: "queued.pdf".into(),
                state: "queued".into(),
                phase: "Waiting".into(),
                progress: 10,
                agent: None,
                created_at: String::new(),
                updated_at: String::new(),
                error: None,
            },
        );

        let value = presentation(&snapshot);

        assert!(value.status.contains("正在监听"));
        assert!(value.job.contains("paper.pdf"));
        assert_eq!(value.toggle, "暂停监听");
        assert!(value.repository_available);

        snapshot.watching = false;
        let paused = presentation(&snapshot);
        assert!(paused.status.contains("已暂停"));
        assert_eq!(paused.toggle, "恢复监听");
    }

    #[test]
    fn converts_only_supported_github_remotes() {
        assert_eq!(
            repository_url("owner/repo"),
            Some("https://github.com/owner/repo".into())
        );
        assert_eq!(
            repository_url("https://github.com/owner/repo.git"),
            Some("https://github.com/owner/repo".into())
        );
        assert_eq!(
            repository_url("git@github.com:owner/repo.git"),
            Some("https://github.com/owner/repo".into())
        );
        assert_eq!(repository_url("https://example.com/owner/repo"), None);
    }
}
