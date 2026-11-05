use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EntryRec {
    pub sha256: String,
    pub size: u64,
    #[serde(default)]
    pub at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    pub install_dir: PathBuf,
    #[serde(default)]
    pub last_sync: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub build: Option<u32>,
    #[serde(default)]
    pub skeleton_version: Option<u32>,
    #[serde(default)]
    pub skeleton_sha256: Option<String>,
    #[serde(default)]
    pub mpqs: std::collections::BTreeMap<String, EntryRec>,
}

pub fn state_path(install_dir: &Path) -> PathBuf {
    install_dir.join("launcher_state.json")
}

pub struct Store {
    pub path: PathBuf,
    pub data: State,
}

impl Store {
    pub fn new(install_dir: &Path) -> Self {
        Self {
            path: state_path(install_dir),
            data: State {
                install_dir: install_dir.to_path_buf(),
                ..Default::default()
            },
        }
    }

    pub fn load(&mut self) {
        if let Ok(s) = std::fs::read_to_string(&self.path) {
            if let Ok(parsed) = serde_json::from_str::<State>(&s) {
                self.data = parsed;
            }
        }
    }

    pub fn save(&mut self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir {:?} failed: {e}", parent))?;
        }
        let mut d = self.data.clone();
        d.last_sync = Some(now_iso());
        let json = serde_json::to_string_pretty(&d)
            .map_err(|e| format!("state serialize failed: {e}"))?;
        std::fs::write(&self.path, json).map_err(|e| format!("state write failed: {e}"))
    }

    pub fn record_mpq(&mut self, rel: &str, sha256: &str, size: u64) {
        self.data.mpqs.insert(
            rel.replace('\\', "/"),
            EntryRec {
                sha256: sha256.to_string(),
                size,
                at: Some(now_iso()),
            },
        );
    }

    pub fn lookup_mpq(&self, rel: &str) -> Option<String> {
        self.data
            .mpqs
            .get(&rel.replace('\\', "/"))
            .map(|e| e.sha256.clone())
    }

    pub fn set_skeleton(&mut self, version: u32, sha256: &str) {
        self.data.skeleton_version = Some(version);
        self.data.skeleton_sha256 = Some(sha256.to_string());
    }
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = std::env::temp_dir().join("lc-state-test");
        let _ = std::fs::create_dir_all(&dir);
        let s = State {
            install_dir: dir.clone(),
            skeleton_version: Some(2433),
            skeleton_sha256: Some("ab".into()),
            mpqs: std::collections::BTreeMap::from([(
                "Data/patch.MPQ".to_string(),
                EntryRec {
                    sha256: "cd".into(),
                    size: 1,
                    at: None,
                },
            )]),
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let parsed: State = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.mpqs.len(), 1);
        assert_eq!(parsed.skeleton_version, Some(2433));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
