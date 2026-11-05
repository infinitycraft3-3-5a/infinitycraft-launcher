use serde::Serialize;
use tauri::State;

use crate::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct ServerStatus {
    pub online: bool,
    pub host: String,
    pub port: u16,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 3724;

fn resolve_host(state: &AppState) -> String {
    let s = state.settings().server_host.trim().to_string();
    if s.is_empty() { DEFAULT_HOST.to_string() } else { s }
}

fn resolve_port(state: &AppState) -> u16 {
    let p = state.settings().server_port;
    if p == 0 { DEFAULT_PORT } else { p }
}

fn tcp_probe(host: &str, port: u16) -> Result<u64, String> {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::{Duration, Instant};

    let addrs: Vec<std::net::SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("DNS: {e}"))?
        .collect();

    let addr = addrs
        .first()
        .ok_or_else(|| "aucune adresse résolue".to_string())?;

    let t0 = Instant::now();
    TcpStream::connect_timeout(addr, Duration::from_secs(2))
        .map_err(|e| format!("connect: {e}"))?;
    Ok(t0.elapsed().as_millis() as u64)
}

#[tauri::command]
pub async fn server_status(
    state: State<'_, AppState>,
) -> Result<ServerStatus, String> {
    let host = resolve_host(&state);
    let port = resolve_port(&state);

    match tcp_probe(&host, port) {
        Ok(ms) => Ok(ServerStatus {
            online: true,
            host,
            port,
            latency_ms: Some(ms),
            error: None,
        }),
        Err(e) => Ok(ServerStatus {
            online: false,
            host,
            port,
            latency_ms: None,
            error: Some(e),
        }),
    }
}
