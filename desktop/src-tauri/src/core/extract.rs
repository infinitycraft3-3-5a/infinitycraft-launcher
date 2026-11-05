use std::path::{Path, PathBuf};

/// Binaire 7za embarqué directement dans l'exécutable : le launcher reste 100% standalone.
static EMBEDDED_7ZA: &[u8] = include_bytes!("../../resources/7za.exe");

/// Dossier cache dédié au launcher.
fn cache_dir() -> Result<PathBuf, String> {
    let local = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("infinitycraft-launcher"));
    let d = local.join("infinitycraft-launcher");
    std::fs::create_dir_all(&d).map_err(|e| format!("création {:?} : {e}", d))?;
    Ok(d)
}

/// Dépose la copie embarquée de 7za sur disque (une fois, si absente/incompatible).
fn deploy_embedded_7za() -> Result<PathBuf, String> {
    let dest = cache_dir()?.join("7za.exe");
    let size_ok = std::fs::metadata(&dest)
        .map(|m| m.len() as usize)
        .unwrap_or(0)
        == EMBEDDED_7ZA.len();
    if size_ok {
        return Ok(dest);
    }
    std::fs::write(&dest, EMBEDDED_7ZA).map_err(|e| format!("écriture de 7za : {e}"))?;
    // Lève un éventuel "Mark of the Web" si le fichier existait déjà.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("powershell.exe");
        cmd.arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-Command")
            .arg(format!(
                "Unblock-File -Path '{}' -ErrorAction SilentlyContinue",
                dest.display()
            ));
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let _ = cmd.output();
    }
    Ok(dest)
}

/// Résout l'emplacement de 7za.exe : dossier cache, à côté de l'exe, puis PATH.
/// En dernier recours, déploie la copie embarquée dans le binaire.
pub fn find_7za(app_dir: Option<&Path>) -> Result<PathBuf, String> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(d) = cache_dir() {
        candidates.push(d.join("7za.exe"));
    }
    if let Some(d) = app_dir {
        candidates.push(d.join("7za.exe"));
        candidates.push(d.join("7z.exe"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(ed) = exe.parent() {
            candidates.push(ed.join("7za.exe"));
            candidates.push(ed.join("7z.exe"));
        }
    }
    for c in &candidates {
        if c.is_file() {
            return Ok(c.clone());
        }
    }

    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            for name in ["7za.exe", "7z.exe"] {
                let c = dir.join(name);
                if c.is_file() {
                    return Ok(c);
                }
            }
        }
    }

    // Rien sur le système : on déploie la copie embarquée.
    deploy_embedded_7za()
}

/// Extrait la première partie `part0` d'une archive 7z (multi-volume supporte).
/// 7za charge automatiquement les parties suivantes (.7z.002, .7z.003, …)
/// situées à côté de `part0`.
pub fn extract_7z(part0: &Path, out_dir: &Path, app_dir: Option<&Path>) -> Result<(), String> {
    let bin = find_7za(app_dir)?;

    std::fs::create_dir_all(out_dir).map_err(|e| format!("mkdir {:?}: {e}", out_dir))?;

    let out_arg = format!("-o{}", out_dir.to_string_lossy());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let mut cmd = std::process::Command::new(&bin);
        cmd.creation_flags(CREATE_NO_WINDOW);
        let output = cmd
            .arg("x")
            .arg(&out_arg)
            .arg(part0)
            .arg("-y")
            .arg("-r")
            .arg("-bsp1")
            .output()
            .map_err(|e| format!("lancement de 7za impossible: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(format!(
                "extraction 7za a echoue (code {}): {} {}",
                output.status,
                stdout.trim(),
                stderr.trim()
            ));
        }
        return Ok(());
    }
    #[cfg(not(windows))]
    {
        let output = std::process::Command::new(&bin)
            .arg("x")
            .arg(&out_arg)
            .arg(part0)
            .arg("-y")
            .arg("-r")
            .arg("-bsp1")
            .output()
            .map_err(|e| format!("lancement de 7za impossible: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(format!(
                "extraction 7za a echoue (code {}): {} {}",
                output.status,
                stdout.trim(),
                stderr.trim()
            ));
        }
        Ok(())
    }
}
