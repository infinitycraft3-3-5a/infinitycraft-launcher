use std::path::{Path, PathBuf};

pub fn path_exists(p: &Path) -> bool {
    p.is_file()
}

pub fn cache_dir(install_dir: &Path) -> PathBuf {
    install_dir.join("Cache")
}

pub fn cache_wdb(install_dir: &Path) -> PathBuf {
    install_dir.join("Cache").join("WDB")
}

pub fn clear_cache(install_dir: &Path) -> Result<u64, String> {
    let wdb = cache_wdb(install_dir);
    let tmp = install_dir.join("Cache").join("WDB");
    let mut removed: u64 = 0;
    for d in [&wdb, &tmp] {
        if d.is_dir() {
            removed += walk_bytes(d)?;
            let _ = std::fs::remove_dir_all(d);
        }
    }
    Ok(removed)
}

fn walk_bytes(d: &Path) -> Result<u64, String> {
    let mut total: u64 = 0;
    let read_dir = std::fs::read_dir(d).map_err(|e| format!("read {:?}: {e}", d))?;
    for e in read_dir.flatten() {
        let p = e.path();
        if p.is_dir() {
            total += walk_bytes(&p)?;
        } else {
            if let Ok(md) = p.metadata() {
                total += md.len();
            }
            let _ = std::fs::remove_file(&p);
        }
    }
    Ok(total)
}

pub fn wow_exe(install_dir: &Path) -> PathBuf {
    install_dir.join("wow.exe")
}
