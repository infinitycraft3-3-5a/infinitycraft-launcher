use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::core::settings::Settings;
use crate::core::utils;

#[derive(Debug, Clone, Serialize)]
pub struct LaunchResult {
    pub pid: u32,
    pub realmline: String,
    pub cwd: String,
}

#[tauri::command]
pub async fn launch_game(
    app: AppHandle,
    state: State<'_, crate::AppState>,
) -> Result<LaunchResult, String> {
    let settings: Settings = state.settings();
    let install_dir = settings.install_dir();
    let wow = utils::wow_exe(&install_dir);

    if !wow.is_file() {
        return Err(format!(
            "wow.exe introuvable dans {:?} - lance \"Installer / Mettre à jour\"",
            install_dir
        ));
    }

    if settings.clear_cache {
        match utils::clear_cache(&install_dir) {
            Ok(_) => {}
            Err(e) => {
                let _ = app.emit("lc://launcher-log", format!("cache clear failed: {e}"));
            }
        }
    }

    let realmline = settings.realmline();
    let mut args: Vec<String> = Vec::new();
    if !realmline.is_empty() {
        args.push("-realmline".into());
        args.push(realmline.clone());
    }

    let cwd = install_dir.to_string_lossy().to_string();
    let mut child = std::process::Command::new(wow)
        .args(&args)
        .current_dir(&install_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("démarrage de wow.exe impossible: {e}"))?;

    let pid = child.id();
    let app2 = app.clone();

    std::thread::spawn(move || {
        let _status = child.wait();
        let _ = app2.emit("game-exited", pid);
    });

    let _ = app.emit("game-launched", pid);

    Ok(LaunchResult {
        pid,
        realmline,
        cwd,
    })
}
