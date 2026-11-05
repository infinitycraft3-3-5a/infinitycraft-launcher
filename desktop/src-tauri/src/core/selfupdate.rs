use serde::Deserialize;
use std::path::PathBuf;

use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

/// URL du manifest de mise à jour du lanceur (EXE brut + sha256).
const MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/infinitycraft3-3-5a/infinitycraft-launcher/main/updater.json";

#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    version: String,
    url: String,
    #[serde(default)]
    sha256: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SelfupdateInfo {
    pub available: bool,
    pub version: String,
    pub url: String,
    #[serde(default)]
    pub sha256: String,
}

fn local_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn parse_ver(s: &str) -> (u64, u64, u64) {
    let mut parts = [0u64; 3];
    for (i, seg) in s.trim().split('.').enumerate() {
        if i >= 3 {
            break;
        }
        parts[i] = seg.trim().parse().unwrap_or(0);
    }
    (parts[0], parts[1], parts[2])
}

/// true si `remote` est strictement plus grand que `local` (semver X.Y.Z).
fn is_newer(remote: &str, local: &str) -> bool {
    let r = parse_ver(remote);
    let l = parse_ver(local);
    r > l
}

/// Récupère le manifest distant et dit s'il y a une mise à jour à jouer.
#[tauri::command]
pub async fn launcher_selfupdate_check() -> Result<SelfupdateInfo, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let res = client
        .get(MANIFEST_URL)
        .header("User-Agent", "InfinityCraft-Launcher")
        .send()
        .await
        .map_err(|e| format!("manifest fetch failed: {e}"))?;

    if !res.status().is_success() {
        let s = res.status();
        return Err(format!("manifest fetch failed: HTTP {s}"));
    }

    let body: String = res
        .text()
        .await
        .map_err(|e| format!("manifest read failed: {e}"))?;

    let m: Manifest = serde_json::from_str::<Manifest>(&body)
        .map_err(|e| format!("manifest parse failed: {e}\nbody: {body}"))?;

    let avail = !m.sha256.trim().is_empty() && is_newer(&m.version, &local_version());
    Ok(SelfupdateInfo {
        available: avail,
        version: m.version,
        url: m.url,
        sha256: m.sha256,
    })
}

/// Télécharge le nouvel EXE à côté du lanceur courant, vérifie son sha256,
/// lance un helper PowerShell (décaché) qui attend la mort du process,
/// remplace le binaire en place (InfinityCraft_Launcher_v<ver>.exe), relance,
/// puis termine le process courant.
#[tauri::command]
pub async fn launcher_selfupdate_run(info: SelfupdateInfo) -> Result<(), String> {
    let url = info.url.trim().to_string();
    if url.is_empty() {
        return Err("URL de mise à jour vide".into());
    }
    let expected_sha = info.sha256.trim().to_ascii_lowercase();
    let version = info.version.trim().to_string();
    if version.is_empty() {
        return Err("version vide dans le manifest".into());
    }

    // 1. Où tourne le lanceur ?
    let exe = std::env::current_exe().map_err(|e| format!("current_exe failed: {e}"))?;
    let dir = exe
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "dir du lanceur inconnu".to_string())?;
    let old_path = exe.clone();

    // Nettoyage des résidus d'une éventuelle mise à jour interrompue
    // (fichiers ic_selfupdate_*.ps1 et ic_selfupdate_new.*).
    let _ = tokio::task::spawn_blocking({
        let d = dir.clone();
        move || {
            use std::fs::read_dir;
            for entry in read_dir(&d).into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("ic_selfupdate_") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    })
    .await;

    // 2. On télécharge dans le même dossier (fichier .part dédié).
    let part_path: PathBuf = dir.join(format!("ic_selfupdate_new.{}", std::process::id()));
    let _ = tokio::fs::remove_file(&part_path).await;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;

    let res = client
        .get(&url)
        .header("User-Agent", "InfinityCraft-Launcher")
        .send()
        .await
        .map_err(|e| format!("download failed: {e}"))?;
    let status = res.status();
    if !status.is_success() {
        let _ = tokio::fs::remove_file(&part_path).await;
        return Err(format!("download failed: HTTP {status}"));
    }

    let mut out = tokio::fs::File::create(&part_path)
        .await
        .map_err(|e| format!("open part failed: {e}"))?;
    let mut stream = res.bytes_stream();
    let mut received: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let c = chunk.map_err(|e| format!("stream read: {e}"))?;
        out.write_all(&c)
            .await
            .map_err(|e| format!("write part: {e}"))?;
        received += c.len() as u64;
    }
    out.flush()
        .await
        .map_err(|e| format!("flush part: {e}"))?;
    drop(out);

    if received == 0 {
        let _ = tokio::fs::remove_file(&part_path).await;
        return Err("téléchargement vide".into());
    }

    // 3. Vérification SHA-256 (si un sha256 est fourni dans le manifest).
    if !expected_sha.is_empty() {
        let actual = tokio::task::spawn_blocking({
            let p = part_path.clone();
            move || super::hash::sha256_file_sync(&p)
        })
        .await
        .map_err(|e| format!("sha256 task: {e}"))??;
        if !actual.eq_ignore_ascii_case(&expected_sha) {
            let _ = tokio::fs::remove_file(&part_path).await;
            return Err(format!(
                "checksum SHA-256 invalide\nattendu: {expected_sha}\nreçu:   {actual}"
            ));
        }
    }

    // 4. Lancer le helper puis se terminer.
    do_replace(&dir, &old_path, &part_path, &version).map_err(|e| {
        let _ = std::fs::remove_file(&part_path);
        e
    })
}

fn do_replace(
    dir: &std::path::Path,
    old_path: &std::path::Path,
    part: &std::path::Path,
    version: &str,
) -> Result<(), String> {
    let final_path = dir.join(format!("InfinityCraft_Launcher_v{version}.exe"));
    let ps1 = dir.join(format!("ic_selfupdate_{}.ps1", std::process::id()));
    let pid = std::process::id();

    let esc = |p: &std::path::Path| p.to_string_lossy().replace('\'', "''");

    let script = format!(
        "$ErrorActionPreference = 'Stop'\n\
         $old   = '{}'\n\
         $new   = '{}'\n\
         $final = '{}'\n\
         $ps1   = '{}'\n\
         $target_pid = {};\n\
          $done  = $false;\n\
          try {{\n\
            while (Get-Process -Id $target_pid -ErrorAction SilentlyContinue) {{ Start-Sleep -Milliseconds 250 }}\n\
           Start-Sleep -Milliseconds 800\n\
           if (Test-Path -LiteralPath $final) {{ Remove-Item -LiteralPath $final -Force -ErrorAction SilentlyContinue }}\n\
           Move-Item -LiteralPath $new -Destination $final -Force\n\
           Remove-Item -LiteralPath $old  -Force -ErrorAction SilentlyContinue\n\
           $done = $true\n\
           Start-Process -FilePath $final\n\
         }} finally {{\n\
           if (Test-Path -LiteralPath $new) {{ Remove-Item -LiteralPath $new -Force -ErrorAction SilentlyContinue }}\n\
           Remove-Item -LiteralPath $ps1  -Force -ErrorAction SilentlyContinue\n\
         }}\n",
        esc(old_path),
        esc(part),
        esc(&final_path),
        esc(&ps1),
        pid
    );

    std::fs::create_dir_all(dir).map_err(|e| format!("mkdir dir failed: {e}"))?;
    std::fs::write(&ps1, script).map_err(|e| format!("write ps1 failed: {e}"))?;

    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-WindowStyle",
        "Hidden",
        "-File",
    ]);
    cmd.arg(ps1.to_string_lossy().as_ref());
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.spawn().map_err(|e| format!("launch helper failed: {e}"))?;

    // Le helper attend la mort du process, remplace le binaire, relance.
    std::process::exit(0);
}
