pub mod commands;
pub mod core;

use tauri::Manager;

pub struct AppState {
    pub settings: std::sync::Arc<std::sync::Mutex<core::settings::Settings>>,
    pub transfer: std::sync::Arc<core::transfer::TransferState>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            settings: std::sync::Arc::new(std::sync::Mutex::new(
                core::settings::Settings::load(),
            )),
            transfer: std::sync::Arc::new(core::transfer::TransferState::new()),
        }
    }

    pub fn settings(&self) -> core::settings::Settings {
        self.settings.lock().expect("settings lock poisoned").clone()
    }
}

#[tauri::command]
fn pause_download(state: tauri::State<'_, AppState>) {
    state.transfer.set_paused(true);
}

#[tauri::command]
fn resume_download(state: tauri::State<'_, AppState>) {
    state.transfer.set_paused(false);
}

pub use core::selfupdate::{launcher_selfupdate_check, launcher_selfupdate_run};

fn log_line(tag: &str, msg: &str) {
    use std::io::Write;
    let dir = std::env::var("LOCALAPPDATA")
        .map(|p| std::path::PathBuf::from(p).join("InfinityCraft"))
        .unwrap_or_else(|_| std::path::PathBuf::from("temp"));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("debug.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{:>12}] [{}] {}", ts, tag, msg);
    }
}

fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.clone()))
            .unwrap_or_else(|| "unknown panic payload".to_string());
        let loc = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "?".to_string());
        log_line("PANIC", &format!("{} @ {}", msg, loc));
        default(info);
    }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_panic_hook();
    log_line("MAIN", "process start");
    let res = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {
            log_line("SINGLE-INST", "second instance detected, ignoring");
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            core::settings::get_settings,
            core::settings::save_settings,
            core::settings::save_install_dir,
            commands::install::check_state,
            commands::install::install_or_update,
            commands::install::repair_client,
            pause_download,
            resume_download,
            launcher_selfupdate_check,
            launcher_selfupdate_run,
            commands::launch::launch_game,
            commands::server::server_status,
            commands::announce::fetch_announcement,
            commands::announce::open_announcement
        ])
        .on_window_event(|_win, event| match event {
            tauri::WindowEvent::CloseRequested { .. } => {
                log_line("WIN", "close requested");
            }
            tauri::WindowEvent::Focused(f) => {
                log_line("WIN", &format!("focused={}", f));
            }
            tauri::WindowEvent::Destroyed => {
                log_line("WIN", "destroyed");
            }
            _ => {}
        })
        .setup(|app| {
            let w = app.get_webview_window("main");
            log_line(
                "SETUP",
                &format!("webview window present: {}", w.is_some()),
            );
            if let Some(w) = w {
                let _ = w.show();
                let _ = w.set_focus();
                log_line("SETUP", "window shown + focused");
                Ok(())
            } else {
                log_line("SETUP", "NO WEBVIEW WINDOW (expected at setup time)");
                Ok(())
            }
        })
        .run(tauri::generate_context!());
    match res {
        Ok(()) => {
            log_line("MAIN", "tauri run() returned Ok");
        }
        Err(e) => {
            log_line("MAIN-ERR", &format!("{}", e));
        }
    }
}
