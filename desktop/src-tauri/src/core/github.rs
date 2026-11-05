use serde::Deserialize;

use super::version::VersionJson;

#[derive(Debug, Clone, Deserialize)]
pub struct AssetInfo {
    pub id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiRelease {
    #[allow(dead_code)]
    id: u64,
    assets: Vec<AssetInfo>,
}

fn urlenc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Résout la release `tag` du repo `owner/repo` via l'API GitHub et renvoie
/// la liste des assets (nom, id, taille).
pub async fn fetch_release(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    tag: &str,
    token: Option<&str>,
) -> Result<Vec<AssetInfo>, String> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases/tags/{}",
        urlenc(owner),
        urlenc(repo),
        urlenc(tag)
    );
    let mut req = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "InfinityCraft-Launcher");
    if let Some(tok) = token.map(str::trim).filter(|t| !t.is_empty()) {
        req = req.header("Authorization", format!("Bearer {tok}"));
    }
    let res = req
        .send()
        .await
        .map_err(|e| format!("API release fetch failed -> {url}: {e}"))?;
    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(format!("release {tag} introuvable ou inaccessibles ({status}): {body}"));
    }
    let rel: ApiRelease = res
        .json()
        .await
        .map_err(|e| format!("parse JSON release: {e}"))?;
    Ok(rel.assets)
}

/// Lit un fichier du repo via l'API GitHub (contenu brut).
pub async fn fetch_contents(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    ref_name: &str,
    path: &str,
    token: Option<&str>,
) -> Result<String, String> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/contents/{}?ref={}",
        urlenc(owner),
        urlenc(repo),
        urlenc(path),
        urlenc(ref_name)
    );
    let mut req = client
        .get(&url)
        .header("Accept", "application/vnd.github.raw+json")
        .header("User-Agent", "InfinityCraft-Launcher");
    if let Some(tok) = token.map(str::trim).filter(|t| !t.is_empty()) {
        req = req.header("Authorization", format!("Bearer {tok}"));
    }
    let res = req
        .send()
        .await
        .map_err(|e| format!("API contents fetch failed -> {url}: {e}"))?;
    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(format!("{path}@{ref_name} introuvable ou non accessible ({status}): {body}"));
    }
    res.text()
        .await
        .map_err(|e| format!("lecture du contenu {path} impossible : {e}"))
}

/// Lit version.json au racine du repo et le retourne parsé.
pub async fn fetch_version_json(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    ref_name: &str,
    token: Option<&str>,
) -> Result<VersionJson, String> {
    let body = fetch_contents(client, owner, repo, ref_name, "version.json", token).await?;
    serde_json::from_str(&body).map_err(|e| format!("version.json parse error: {e}\nbody: {body}"))
}

/// URL API de téléchargement d'un asset (302 → CDN).
pub fn asset_url(owner: &str, repo: &str, asset_id: u64) -> String {
    format!(
        "https://api.github.com/repos/{}/{}/releases/assets/{asset_id}",
        urlenc(owner),
        urlenc(repo)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlenc_basics() {
        assert_eq!(urlenc("my-repo"), "my-repo");
        assert_eq!(urlenc("a/b"), "a%2Fb");
        assert_eq!(urlenc("v1.0"), "v1.0");
    }
}
