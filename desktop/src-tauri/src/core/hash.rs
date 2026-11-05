use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

use tokio::task;

use sha2::Digest;

pub fn sha256_file_sync(path: &std::path::Path) -> Result<String, String> {
    let mut f =
        File::open(path).map_err(|e| format!("open {} failed: {e}", path.display()))?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("read {} failed: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub async fn sha256_file(path: &std::path::Path) -> Result<String, String> {
    let path: PathBuf = path.to_path_buf();
    task::spawn_blocking(move || sha256_file_sync(&path))
        .await
        .map_err(|e| format!("sha256 task spawn failed: {e}"))?
}

pub async fn verify(path: &std::path::Path, expected: &str) -> Result<(bool, String), String> {
    let actual = sha256_file(path).await?;
    Ok((actual.eq_ignore_ascii_case(expected.trim()), actual))
}
