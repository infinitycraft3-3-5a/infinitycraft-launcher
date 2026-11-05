use std::io::{Read, Write};
use std::path::Path;

use sha2::Digest;

/// Concatène les fichiers `parts` (dans l'ordre indiqué) dans `dest_final`,
/// renvoie la taille totale et le sha256 du résultat.
pub fn concat_parts(parts: &[String], dest_final: &Path) -> Result<(u64, String), String> {
    if let Some(parent) = dest_final.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {:?}: {e}", parent))?;
    }

    let file = std::fs::File::create(dest_final)
        .map_err(|e| format!("create {}: {e}", dest_final.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    let mut hasher = sha2::Sha256::new();
    let mut total: u64 = 0;

    for p in parts {
        let mut reader = std::io::BufReader::with_capacity(
            1 << 20,
            std::fs::File::open(p).map_err(|e| format!("ouvrir la part {p}: {e}"))?,
        );
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| format!("lecture part {p}: {e}"))?;
            if n == 0 {
                break;
            }
            writer
                .write_all(&buf[..n])
                .map_err(|e| format!("écriture {}: {e}", dest_final.display()))?;
            hasher.update(&buf[..n]);
            total += n as u64;
        }
    }

    writer
        .flush()
        .map_err(|e| format!("flush {}: {e}", dest_final.display()))?;
    drop(writer);

    let digest = hasher.finalize();
    Ok((total, hex::encode(digest)))
}

/// Lève le "Mark of the Web" (quarantine) sur un fichier ou un répertoire.
pub fn quarantine(p: &Path) {
    if !p.exists() {
        return;
    }
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(format!(
            "Unblock-File -Path '{}' -ErrorAction SilentlyContinue",
            p.to_string_lossy()
        ));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let _ = cmd.output();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn concat_two() {
        let dir = std::env::temp_dir().join("lc-concat-test");
        let _ = std::fs::create_dir_all(&dir);
        let a = dir.join("a.bin");
        let b = dir.join("b.bin");
        let out = dir.join("out.bin");

        let mut fa = std::fs::File::create(&a).unwrap();
        fa.write_all(b"Hello ").unwrap();
        let mut fb = std::fs::File::create(&b).unwrap();
        fb.write_all(b"World").unwrap();

        let (size, sha) = concat_parts(&[a.display().to_string(), b.display().to_string()], &out).unwrap();
        assert_eq!(size, 11);
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "Hello World");
        assert_eq!(sha.len(), 64);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
