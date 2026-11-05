use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub release: String,
    pub latest_version: u32,
    #[serde(default)]
    pub min_client_build: Option<u32>,
    pub skeleton: Skeleton,
    #[serde(default)]
    pub patches: Vec<Patch>,
}

/// Fichier individuel du squelette (chemin relatif depuis la racine d'install,
/// séparateur `/`). `size` permet de détecter un fichier manquant ou tronqué :
/// si l'un d'entre eux est absent/incorrect, le 7z complet est re-téléchargé.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkeletonFile {
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skeleton {
    #[serde(default)]
    pub sha256: String,
    pub parts: Vec<Part>,
    /// Liste des fichiers contenus dans l'archive 7z. Vide sur les anciens
    /// manifests (le lanceur retombe alors sur la détection de `wow.exe`).
    #[serde(default)]
    pub files: Vec<SkeletonFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Patch {
    pub file: String,
    #[serde(default)]
    pub version: Option<u32>,
    pub target: String,
    #[serde(default)]
    pub sha256: String,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Part {
    pub file: String,
    pub size: u64,
    #[serde(default)]
    pub sha256: String,
}

impl Patch {
    pub fn base_name(&self) -> String {
        self.file.clone()
    }
}

pub async fn fetch(url: &str, auth: Option<&str>) -> Result<VersionJson, String> {
    let client = reqwest::Client::new();
    let mut req = client
        .get(url)
        .header("Accept", "application/json");
    if let Some(tok) = auth.map(str::trim).filter(|t| !t.is_empty()) {
        req = req.header("Authorization", format!("Bearer {tok}"));
    }
    let res = req
        .send()
        .await
        .map_err(|e| format!("version.json fetch failed -> {url}: {e}"))?;
    if !res.status().is_success() {
        let code = res.status().as_u16();
        let body = res.text().await.unwrap_or_default();
        return Err(format!("version.json fetch failed ({code}): {body}"));
    }
    let body = res
        .text()
        .await
        .map_err(|e| format!("version.json body read failed: {e}"))?;
    serde_json::from_str(&body).map_err(|e| format!("version.json parse error: {e}\nbody: {body}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sample() {
        let s = r#"{
          "release": "v3.3.5-full",
          "latestVersion": 2433,
          "minClientBuild": 2433,
          "skeleton": { "sha256": "ab", "parts": [ {"file":"skeleton.7z.001","size":10,"sha256":"cd"} ] },
          "patches": [ { "file":"patch.MPQ","version":2433,"target":"Data/patch.MPQ","sha256":"ef","parts":[{"file":"patch.MPQ.001","size":5,"sha256":"11"}] } ]
        }"#;
        let v: VersionJson = serde_json::from_str(s).unwrap();
        assert_eq!(v.latest_version, 2433);
        assert_eq!(v.skeleton.parts.len(), 1);
        assert_eq!(v.patches.len(), 1);
        assert_eq!(v.patches[0].target, "Data/patch.MPQ");
    }
}
