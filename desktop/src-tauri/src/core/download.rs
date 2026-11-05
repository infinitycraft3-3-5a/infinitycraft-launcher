use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::core::transfer::TransferState;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProgressEvent {
    pub file: String,
    pub file_index: usize,
    pub file_count: usize,
    /// Progress global de la session (0..100), basé sur les octets.
    pub percent: u8,
    pub bytes_file: u64,
    pub bytes_file_total: u64,
    /// Octets globaux accomplis (toutes parties confondues) + reçu courant.
    pub bytes_global: u64,
    /// Total d'octets à télécharger pour la session.
    pub bytes_total: u64,
    pub speed_bps: u64,
    pub done: bool,
    pub paused: bool,
    pub phase: String,
}

pub type ProgressSender = Arc<dyn Fn(ProgressEvent) + Send + Sync>;

fn pct_of(global: u64, total: u64, done: bool) -> u8 {
    if done {
        return 100;
    }
    if total == 0 {
        return 100;
    }
    ((global as f64) / (total as f64) * 100_f64).clamp(0.0, 99.5) as u8
}

/// Télécharge `url` vers `dest` (fichier `.part`), avec reprise via HTTP Range.
/// Si `dest` existe déjà et n'atteint pas `expected_bytes`, continue depuis la fin
/// (Range: bytes=N-). Gère 206 (partial), 200 (rewrite), 416 (reset).
/// `global_base` : octets déjà accomplis dans la session avant cette partie.
/// `global_total` : octets à télécharger au total pour la session.
/// `state` : état partagé (pause + vitesse).
pub async fn download_with_resume(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    expected_bytes: u64,
    tx: Option<&ProgressSender>,
    phase: &str,
    file_index: usize,
    file_count: usize,
    auth: Option<&str>,
    global_base: u64,
    global_total: u64,
    state: &TransferState,
) -> Result<u64, String> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("mkdir {:?} failed: {e}", parent))?;
    }

    let existing = match tokio::fs::metadata(dest).await.map(|m| m.len()) {
        Ok(l) => l,
        Err(_) => 0,
    };

    let start_at = if existing > 0 && existing < expected_bytes {
        existing
    } else if existing == expected_bytes {
        return Ok(existing);
    } else {
        0
    };

    let mut req = client
        .get(url)
        .header("Accept", "application/octet-stream")
        .header("User-Agent", "InfinityCraft-Launcher");
    if let Some(tok) = auth.map(str::trim).filter(|t| !t.is_empty()) {
        req = req.header("Authorization", format!("Bearer {tok}"));
    }
    if start_at > 0 {
        req = req.header("Range", format!("bytes={}-", start_at));
    }

    let res = req
        .send()
        .await
        .map_err(|e| format!("download {url}: {e}"))?;

    let status = res.status();
    let (range_ok, full_len) = if status.as_u16() == 206 {
        (true, expected_bytes)
    } else if status.is_success() {
        (false, expected_bytes)
    } else if status.as_u16() == 416 {
        let _ = tokio::fs::remove_file(dest).await;
        (false, expected_bytes)
    } else {
        return Err(format!("download {url} failed: HTTP {}", status));
    };

    let mut f = if range_ok {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(dest)
            .await
            .map_err(|e| format!("open {} (append) failed: {e}", dest.display()))?
    } else {
        let _ = tokio::fs::remove_file(dest).await;
        tokio::fs::File::create(dest)
            .await
            .map_err(|e| format!("open {} failed: {e}", dest.display()))?
    };

    let mut received = start_at;
    let mut stream = res.bytes_stream();

    struct Ctx<'a> {
        tx: Option<&'a ProgressSender>,
        url: String,
        phase: String,
        file_index: usize,
        file_count: usize,
        full_len: u64,
        global_base: u64,
        global_total: u64,
        state: &'a TransferState,
    }

    let ctx = Ctx {
        tx,
        url: url.to_string(),
        phase: phase.to_string(),
        file_index,
        file_count,
        full_len,
        global_base,
        global_total,
        state,
    };

    let emit = |received: u64, done: bool, ctx: &Ctx| {
        let paused = ctx.state.is_paused();
        ctx.state.record(ctx.global_base + received);
        let speed = ctx.state.speed_bps();
        if let Some(tx) = ctx.tx {
            tx(ProgressEvent {
                file: ctx.url.clone(),
                file_index: ctx.file_index,
                file_count: ctx.file_count,
                percent: pct_of(ctx.global_base + received, ctx.global_total, done),
                bytes_file: received,
                bytes_file_total: ctx.full_len,
                bytes_global: ctx.global_base + received,
                bytes_total: ctx.global_total,
                speed_bps: speed,
                done,
                paused,
                phase: ctx.phase.clone(),
            });
        }
    };

    if ctx.state.is_paused() {
        emit(start_at, false, &ctx);
    }

    let mut last_t = Instant::now();

    loop {
        if ctx.state.is_paused() {
            emit(received, false, &ctx);
            while ctx.state.is_paused() {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            last_t = Instant::now();
        }

        let Some(chunk) = stream.next().await else { break };
        let chunk = chunk.map_err(|e| format!("stream read: {e}"))?;
        f.write_all(&chunk)
            .await
            .map_err(|e| format!("write {}: {e}", dest.display()))?;
        received += chunk.len() as u64;

        let now = Instant::now();
        if now.duration_since(last_t).as_millis() >= 200 {
            last_t = now;
            emit(received, false, &ctx);
        }
    }

    f.flush()
        .await
        .map_err(|e| format!("flush {}: {e}", dest.display()))?;
    drop(f);

    if received != expected_bytes {
        return Err(format!(
            "download incomplete for {url}: {received} / {expected_bytes} octets"
        ));
    }
    emit(received, true, &ctx);
    Ok(received)
}
