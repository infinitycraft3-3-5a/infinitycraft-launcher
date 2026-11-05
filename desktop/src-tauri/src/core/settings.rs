use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "def_install")]
    pub install_dir: String,
    #[serde(default = "def_true")]
    pub clear_cache: bool,
    #[serde(default = "def_true")]
    pub auto_update: bool,
    #[serde(default = "def_lang")]
    pub language: String,
    #[serde(default = "def_owner")]
    pub owner: String,
    #[serde(default = "def_repo")]
    pub repo: String,
    #[serde(default)]
    pub tag_hint: String,
    #[serde(default = "def_realmline")]
    pub realmline: String,
    #[serde(default)]
    pub version_json_url: String,
    #[serde(default)]
    pub github_token: String,
}

fn def_true() -> bool {
    true
}
fn def_lang() -> String {
    "frFR".into()
}
fn def_owner() -> String {
    "infinitycraft3-3-5a".into()
}
fn def_repo() -> String {
    "infinitycraft-client".into()
}
fn def_realmline() -> String {
    "InfinityCraft".into()
}
fn def_install() -> String {
    "C:\\Program Files\\InfinityCraft 3.3.5a".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            install_dir: def_install(),
            clear_cache: true,
            auto_update: true,
            language: "frFR".into(),
            owner: def_owner(),
            repo: def_repo(),
            tag_hint: String::new(),
            realmline: def_realmline(),
            version_json_url: String::new(),
            github_token: String::new(),
        }
    }
}

impl Settings {
    pub fn github_token(&self) -> String {
        self.github_token.trim().to_string()
    }
}

impl Settings {
    pub fn owner(&self) -> String {
        let s = self.owner.trim();
        if s.is_empty() {
            def_owner()
        } else {
            s.to_string()
        }
    }

    pub fn repo(&self) -> String {
        let s = self.repo.trim();
        if s.is_empty() {
            def_repo()
        } else {
            s.to_string()
        }
    }

    pub fn version_json_url(&self) -> String {
        self.version_json_url.trim().to_string()
    }

    pub fn base_url(&self) -> String {
        format!("https://github.com/{}/{}", self.owner(), self.repo())
    }

    pub fn asset_url(&self, tag: &str, file: &str) -> String {
        let tag = tag.trim();
        let file = file.trim_start_matches('/');
        format!("{}/releases/download/{tag}/{file}", self.base_url())
    }
}

fn settings_path() -> std::path::PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    base.join("infinitycraft-launcher").join("settings.json")
}

impl Settings {
    pub fn load() -> Self {
        let p = settings_path();
        if let Ok(s) = std::fs::read_to_string(p) {
            if let Ok(parsed) = serde_json::from_str::<Settings>(&s) {
                return parsed;
            }
        }
        Self::default()
    }

    pub fn save(&self) -> Result<(), String> {
        let p = settings_path();
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir {:?} failed: {e}", parent))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("settings serialize failed: {e}"))?;
        std::fs::write(p, json).map_err(|e| format!("settings write failed: {e}"))
    }

    pub fn install_dir(&self) -> std::path::PathBuf {
        let d = self.install_dir.trim();
        if d.is_empty() {
            def_install().into()
        } else {
            Path::new(d).to_path_buf()
        }
    }

    pub fn realmline(&self) -> String {
        let s = self.realmline.trim();
        if s.is_empty() {
            def_realmline()
        } else {
            s.to_string()
        }
    }
}

#[tauri::command]
pub fn get_settings(state: tauri::State<crate::AppState>) -> Result<Settings, String> {
    Ok(state.settings())
}

#[tauri::command]
pub fn save_install_dir(
    state: tauri::State<crate::AppState>,
    dir: String,
) -> Result<Settings, String> {
    let mut cur = state.settings();
    let d = dir.trim();
    if !d.is_empty() {
        cur.install_dir = d.to_string();
    }
    let out = cur.clone();
    cur.save()?;
    let mut g = state.settings.lock().expect("settings lock poisoned");
    *g = cur;
    Ok(out)
}

#[tauri::command]
pub fn save_settings(
    state: tauri::State<crate::AppState>,
    args: Settings,
) -> Result<Settings, String> {
    let out = args.clone();
    args.save()?;
    let mut g = state.settings.lock().expect("settings lock poisoned");
    *g = args;
    Ok(out)
}
