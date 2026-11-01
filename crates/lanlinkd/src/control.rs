//! Persistent local control/status socket for the daemon.
//!
//! The same socket serves both control commands and read-only status snapshots
//! so frontends have one stable endpoint to discover. It outlives individual
//! network sessions: after a disconnect the daemon keeps listening here and
//! reports `connected: false`.

#[cfg(not(unix))]
use std::net::SocketAddr;
#[cfg(unix)]
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lanlink_proto::control::{ControlRequest, ControlResponse};
use lanlink_proto::status::{StatusRequest, StatusResponse};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, watch};

use crate::status::StatusState;

/// Default Unix socket path for the daemon control/status API.
#[cfg(unix)]
pub fn default_socket_path() -> PathBuf {
    PathBuf::from("/run/lanlink/lanlinkd-control.sock")
}

/// Unprivileged fallback used when `/run/lanlink` cannot be created.
#[cfg(unix)]
pub fn fallback_socket_path() -> PathBuf {
    PathBuf::from("/tmp/lanlinkd-control.sock")
}

/// Default TCP loopback address for Windows.
#[cfg(windows)]
pub fn default_tcp_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 19000))
}

/// A control command forwarded from a client to the daemon runtime.
pub struct ControlEnvelope {
    pub req: ControlRequest,
    pub resp: oneshot::Sender<ControlResponse>,
}

/// Handle used by the runtime to update the state exposed through the socket.
pub type StateTx = watch::Sender<StatusState>;

/// Start the persistent control/status listener and spawn its accept loop.
#[cfg(unix)]
pub async fn spawn_server(
    configured: Option<String>,
    state_rx: watch::Receiver<StatusState>,
    cmd_tx: mpsc::Sender<ControlEnvelope>,
) {
    let candidates: Vec<PathBuf> = match configured {
        Some(p) => vec![PathBuf::from(p)],
        None => vec![default_socket_path(), fallback_socket_path()],
    };

    tokio::spawn(async move {
        for path in candidates {
            match bind_listener(&path) {
                Ok(listener) => {
                    tracing::info!(path = %path.display(), "control socket listening");
                    run_server(listener, &path, state_rx, cmd_tx).await;
                    return;
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "control socket bind failed"
                    );
                }
            }
        }
        tracing::warn!("control server disabled: no usable socket path");
    });
}

#[cfg(unix)]
fn bind_listener(path: &Path) -> std::io::Result<tokio::net::UnixListener> {
    use std::os::unix::fs::PermissionsExt;

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(path);
    let listener = tokio::net::UnixListener::bind(path)?;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666));
    Ok(listener)
}

#[cfg(unix)]
async fn run_server(
    listener: tokio::net::UnixListener,
    path: &Path,
    state_rx: watch::Receiver<StatusState>,
    cmd_tx: mpsc::Sender<ControlEnvelope>,
) {
    loop {
        tokio::select! {
            _ = crate::shutdown().wait() => {
                tracing::debug!("control server stopping");
                break;
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => {
                        let state_rx = state_rx.clone();
                        let cmd_tx = cmd_tx.clone();
                        tokio::spawn(handle_stream(stream, state_rx, cmd_tx));
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "control accept failed");
                    }
                }
            }
        }
    }
    let _ = std::fs::remove_file(path);
}

/// Start the persistent TCP loopback listener on Windows.
#[cfg(windows)]
pub async fn spawn_server(
    configured: Option<String>,
    state_rx: watch::Receiver<StatusState>,
    cmd_tx: mpsc::Sender<ControlEnvelope>,
) {
    let addrs = parse_status_addr(configured);
    tokio::spawn(async move {
        for addr in addrs {
            match tokio::net::TcpListener::bind(addr).await {
                Ok(listener) => {
                    tracing::info!(%addr, "control TCP listener bound");
                    run_tcp_server(listener, addr, state_rx, cmd_tx).await;
                    return;
                }
                Err(e) => {
                    tracing::warn!(%addr, error = %e, "control TCP bind failed");
                }
            }
        }
        tracing::warn!("control server disabled: no usable TCP address");
    });
}

#[cfg(windows)]
fn parse_status_addr(configured: Option<String>) -> Vec<SocketAddr> {
    if let Some(value) = configured {
        if let Ok(addr) = value.parse::<SocketAddr>() {
            return vec![addr];
        }
        if let Ok(port) = value.parse::<u16>() {
            return vec![SocketAddr::from(([127, 0, 0, 1], port))];
        }
    }
    vec![default_tcp_addr()]
}

#[cfg(windows)]
async fn run_tcp_server(
    listener: tokio::net::TcpListener,
    addr: SocketAddr,
    state_rx: watch::Receiver<StatusState>,
    cmd_tx: mpsc::Sender<ControlEnvelope>,
) {
    loop {
        tokio::select! {
            _ = crate::shutdown().wait() => {
                tracing::debug!("control TCP server stopping");
                break;
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, peer)) => {
                        if !is_loopback(peer.ip()) {
                            tracing::debug!(%peer, "rejecting non-loopback control connection");
                            continue;
                        }
                        let state_rx = state_rx.clone();
                        let cmd_tx = cmd_tx.clone();
                        tokio::spawn(handle_stream(stream, state_rx, cmd_tx));
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "control TCP accept failed");
                    }
                }
            }
        }
    }
    let _ = addr;
}

#[cfg(windows)]
fn is_loopback(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => ip.is_loopback(),
        std::net::IpAddr::V6(ip) => ip.is_loopback(),
    }
}

async fn handle_stream<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    state_rx: watch::Receiver<StatusState>,
    cmd_tx: mpsc::Sender<ControlEnvelope>,
) {
    let (reader_half, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader_half);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        let out = match dispatch_line(line.trim_end(), &state_rx, &cmd_tx).await {
            Some(bytes) => bytes,
            None => continue,
        };
        if writer.write_all(&out).await.is_err() {
            break;
        }
    }
}

async fn dispatch_line(
    line: &str,
    state_rx: &watch::Receiver<StatusState>,
    cmd_tx: &mpsc::Sender<ControlEnvelope>,
) -> Option<Vec<u8>> {
    // First try to parse a control command.
    if let Ok(req) = serde_json::from_str::<ControlRequest>(line) {
        let (tx, rx) = oneshot::channel();
        if cmd_tx
            .send(ControlEnvelope { req, resp: tx })
            .await
            .is_err()
        {
            return Some(error_bytes("daemon control channel closed"));
        }
        let resp = match rx.await {
            Ok(r) => r,
            Err(_) => ControlResponse::Error {
                error: "control response dropped".into(),
            },
        };
        return Some(json_bytes(&resp, "control response encode failed"));
    }

    // Fall back to the read-only status snapshot.
    if let Ok(StatusRequest::Snapshot) = serde_json::from_str::<StatusRequest>(line) {
        let state = state_rx.borrow().clone();
        let snap = state.snapshot().await;
        return Some(json_bytes(
            &StatusResponse::Snapshot(snap),
            "snapshot encode failed",
        ));
    }

    Some(error_bytes(
        "bad request: not a control command or status snapshot",
    ))
}

fn error_bytes(msg: &str) -> Vec<u8> {
    let resp = StatusResponse::Error { error: msg.into() };
    json_bytes(&resp, "error encode failed")
}

fn json_bytes<T: serde::Serialize>(value: &T, fallback: &str) -> Vec<u8> {
    let mut text =
        serde_json::to_string(value).unwrap_or_else(|_| format!("{{\"error\":\"{}\"}}", fallback));
    text.push('\n');
    text.into_bytes()
}

/// Build an empty/disconnected status state for the daemon runtime.
pub fn empty_state() -> StatusState {
    crate::status::StatusState {
        registry: crate::status::StatusRegistry::new(),
        switch: Arc::new(tokio::sync::Mutex::new(lanlink_switch::Switch::new(
            lanlink_types::NodeId([0u8; 8]),
            lanlink_crypto::StaticSession::new([0u8; 32]),
        ))),
        metrics: Arc::new(crate::Metrics::default()),
        punching: Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new())),
        start: std::time::Instant::now(),
    }
}
