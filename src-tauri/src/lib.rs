mod agents;
mod commands;
mod config;
mod credentials;
mod diagnostics;
mod jobs;
mod launch_at_login;
mod logging;
mod mineru;
mod models;
mod skill;
mod tools;
mod tray;
mod watcher;
mod wiki;
mod worker;
mod workspace;

use commands::{
    cancel_job, choose_workspace, export_diagnostics, get_app_snapshot, initialize_workspace,
    open_target, retry_job, run_preflight, save_mineru_token, save_settings, set_watching,
    AppState,
};
use tauri::Manager;

pub fn run_cli(args: &[String]) -> Result<bool, String> {
    if args.first().map(String::as_str) != Some("parse") {
        return Ok(false);
    }
    let mut input = None;
    let mut output = None;
    let mut mode = mineru::ParseMode::Precision;
    let mut json_output = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--input" => {
                index += 1;
                input = args.get(index).map(std::path::PathBuf::from);
            }
            "--output" => {
                index += 1;
                output = args.get(index).map(std::path::PathBuf::from);
            }
            "--mode" => {
                index += 1;
                mode = mineru::ParseMode::parse(
                    args.get(index)
                        .ok_or_else(|| "--mode requires a value".to_string())?,
                )?;
            }
            "--json" => json_output = true,
            value => return Err(format!("unknown parse argument: {value}")),
        }
        index += 1;
    }
    let input = input.ok_or_else(|| "parse requires --input <PDF>".to_string())?;
    let output = output.ok_or_else(|| "parse requires --output <DIR>".to_string())?;
    let runtime =
        tokio::runtime::Runtime::new().map_err(|error| format!("start async runtime: {error}"))?;
    let markdown = runtime.block_on(mineru::parse(&input, &output, mode))?;
    if json_output {
        println!(
            "{}",
            serde_json::json!({ "ok": true, "markdown": markdown })
        );
    } else {
        println!("{}", markdown.display());
    }
    Ok(true)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = AppState::load(app.handle())?;
            let configured = state
                .snapshot
                .lock()
                .map(|snapshot| snapshot.configured)
                .unwrap_or(false);
            app.manage(state);
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            tray::install(app)?;
            watcher::start(app.handle().clone());
            worker::start(app.handle().clone());
            if should_show_main_window(configured) {
                if let Some(window) = app.get_webview_window("main") {
                    window.show()?;
                    window.set_focus()?;
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            save_settings,
            save_mineru_token,
            choose_workspace,
            initialize_workspace,
            run_preflight,
            set_watching,
            retry_job,
            cancel_job,
            open_target,
            export_diagnostics,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");
    app.run(|app, event| {
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } = event
        {
            let view = app
                .state::<AppState>()
                .snapshot
                .lock()
                .map(|snapshot| {
                    if snapshot.configured {
                        "overview"
                    } else {
                        "setup"
                    }
                })
                .unwrap_or("setup");
            tray::show_window(app, view);
        }
    });
}

fn should_show_main_window(configured: bool) -> bool {
    !configured
}

#[cfg(test)]
mod tests {
    use super::should_show_main_window;

    #[test]
    fn only_unconfigured_launches_show_the_main_window() {
        assert!(should_show_main_window(false));
        assert!(!should_show_main_window(true));
    }
}
