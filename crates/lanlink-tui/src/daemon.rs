//! Daemon control/status client (Unix domain socket or TCP loopback,
//! newline-delimited JSON).
//!
//! The TUI now speaks the unified control/status socket introduced for
//! disconnect/graceful-stop: it sends [`lanlink_proto::control::ControlRequest`]
//! and receives [`lanlink_proto::control::ControlResponse`]. The legacy
//! per-network `lanlinkd-<network_id>.sock` path is no longer used.

use lanlink_proto::control::{ControlRequest, ControlResponse, ControlResult};
use lanlink_proto::status::StatusSnapshot;
#[cfg(windows)]
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

const CONTROL_SOCKET: &str = "lanlinkd-control.sock";

/// Fetch the current status snapshot from the daemon's control socket.
#[cfg(unix)]
pub async fn fetch_snapshot(path: &Path) -> anyhow::Result<StatusSnapshot> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::UnixStream::connect(path).await?;
    let request = serde_json::to_string(&ControlRequest::Status)?;
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.shutdown().await?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    let text = std::str::from_utf8(&buf)?;
    let line = text.lines().next().unwrap_or("");
    match serde_json::from_str::<ControlResponse>(line)? {
        ControlResponse::Ok {
            result: ControlResult::Status(snapshot),
        } => Ok(snapshot),
        ControlResponse::Ok { .. } => anyhow::bail!("unexpected control response"),
        ControlResponse::Error { error } => anyhow::bail!("daemon: {error}"),
    }
}

/// Fetch one snapshot over the daemon's TCP loopback control socket (Windows).
#[cfg(windows)]
pub async fn fetch_snapshot(path: &Path) -> anyhow::Result<StatusSnapshot> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr: SocketAddr = path
        .to_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("invalid TCP control address: {}", path.display()))?;

    let mut stream = tokio::net::TcpStream::connect(addr).await?;
    let request = serde_json::to_string(&ControlRequest::Status)?;
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.shutdown().await?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    let text = std::str::from_utf8(&buf)?;
    let line = text.lines().next().unwrap_or("");
    match serde_json::from_str::<ControlResponse>(line)? {
        ControlResponse::Ok {
            result: ControlResult::Status(snapshot),
        } => Ok(snapshot),
        ControlResponse::Ok { .. } => anyhow::bail!("unexpected control response"),
        ControlResponse::Error { error } => anyhow::bail!("daemon: {error}"),
    }
}

/// Send a control command and return the daemon's response.
#[cfg(unix)]
pub async fn send_command(path: &Path, req: ControlRequest) -> anyhow::Result<ControlResponse> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::UnixStream::connect(path).await?;
    let request = serde_json::to_string(&req)?;
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.shutdown().await?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    let text = std::str::from_utf8(&buf)?;
    let line = text.lines().next().unwrap_or("");
    serde_json::from_str::<ControlResponse>(line)
        .map_err(|e| anyhow::anyhow!("invalid control response: {e}"))
}

#[cfg(windows)]
pub async fn send_command(path: &Path, req: ControlRequest) -> anyhow::Result<ControlResponse> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr: SocketAddr = path
        .to_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("invalid TCP control address: {}", path.display()))?;

    let mut stream = tokio::net::TcpStream::connect(addr).await?;
    let request = serde_json::to_string(&req)?;
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.shutdown().await?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    let text = std::str::from_utf8(&buf)?;
    let line = text.lines().next().unwrap_or("");
    serde_json::from_str::<ControlResponse>(line)
        .map_err(|e| anyhow::anyhow!("invalid control response: {e}"))
}

/// Find the listening daemon control socket when `--control-socket` is not given.
#[cfg(unix)]
pub async fn discover_socket() -> Result<Option<PathBuf>, String> {
    const DIRS: &[&str] = &["/run/lanlink", "/tmp"];
    for dir in DIRS {
        let path = PathBuf::from(dir).join(CONTROL_SOCKET);
        if probe(&path).await {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

#[cfg(unix)]
async fn probe(path: &Path) -> bool {
    tokio::net::UnixStream::connect(path).await.is_ok()
}

/// Path of the live daemon config written by the MSI / `service up`
/// (matches `CONFIG_PATH` in `crates/lanlinkd/src/service.rs`).
#[cfg(windows)]
const DAEMON_CONFIG: &str = "C:\\Program Files\\LanLink\\lanlinkd.toml";

/// Read the configured `[status] socket` from the daemon config, if the
/// config exists and parses. The control/status socket reuses that config key.
#[cfg(windows)]
fn configured_control_socket() -> Option<String> {
    #[derive(serde::Deserialize, Default)]
    struct Probe {
        #[serde(default)]
        status: StatusSection,
    }
    #[derive(serde::Deserialize, Default)]
    struct StatusSection {
        socket: Option<String>,
    }
    let text = std::fs::read_to_string(DAEMON_CONFIG).ok()?;
    let probe: Probe = toml::from_str(&text).ok()?;
    probe.status.socket
}

/// Find a listening daemon TCP control socket on Windows.
#[cfg(windows)]
pub async fn discover_socket() -> Result<Option<PathBuf>, String> {
    if let Some(configured) = configured_control_socket() {
        if let Ok(addr) = configured.parse::<SocketAddr>() {
            if tokio::net::TcpStream::connect(addr).await.is_ok() {
                return Ok(Some(PathBuf::from(addr.to_string())));
            }
        }
    }
    let addr = SocketAddr::from(([127, 0, 0, 1], 19000));
    if tokio::net::TcpStream::connect(addr).await.is_ok() {
        return Ok(Some(PathBuf::from(addr.to_string())));
    }
    Ok(None)
}

#[cfg(windows)]
#[allow(dead_code)]
async fn probe(_path: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use lanlink_proto::status::{DaemonStatus, PathKind, PeerStatus};
    use lanlink_proto::{NetworkId, NodeId, VirtualIp};

    fn canned_snapshot() -> StatusSnapshot {
        StatusSnapshot {
            status: DaemonStatus {
                version: "0.1.0-test".into(),
                pid: 1234,
                network_id: NetworkId(42),
                assigned_ip: VirtualIp("26.0.0.9".parse().unwrap()),
                tap_name: "lanlink0".into(),
                bind: "0.0.0.0:9000".parse().unwrap(),
                relay_addr: None,
                gateway_enabled: false,
                firewall_enabled: false,
                uptime_seconds: 99,
                tx_bytes: 111,
                rx_bytes: 222,
                connected: true,
            },
            peers: vec![PeerStatus {
                node_id: NodeId([7; 8]),
                virtual_ip: VirtualIp("26.0.0.10".parse().unwrap()),
                endpoint: Some("127.0.0.1:9100".parse().unwrap()),
                path: PathKind::Direct,
                latency_ms: Some(23),
                last_seen_secs_ago: Some(2),
                tx_bytes: 1,
                rx_bytes: 2,
                is_gateway: false,
                shared_subnet: String::new(),
            }],
        }
    }

    /// Client side of the NDJSON protocol against a minimal fake daemon:
    /// write a request line, half-close, parse one response line.
    #[tokio::test]
    #[cfg(unix)]
    async fn fetch_snapshot_round_trip() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let dir = std::env::temp_dir().join(format!(
            "lanlink-tui-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fake.sock");
        let _ = std::fs::remove_file(&path);

        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let snapshot = canned_snapshot();
        let canned = serde_json::to_string(&ControlResponse::Ok {
            result: ControlResult::Status(snapshot),
        })
        .unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            assert!(line.contains("\"cmd\":\"status\""), "request was {line:?}");
            let reply = reader.get_mut();
            reply.write_all(canned.as_bytes()).await.unwrap();
            reply.write_all(b"\n").await.unwrap();
        });

        let result = fetch_snapshot(&path).await.expect("fetch");
        assert_eq!(result.status.network_id.0, 42);
        assert_eq!(result.status.version, "0.1.0-test");
        assert_eq!(result.status.connected, true);
        assert_eq!(result.peers.len(), 1);
        assert_eq!(result.peers[0].latency_ms, Some(23));
        assert_eq!(result.peers[0].path, PathKind::Direct);

        server.await.unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    /// The daemon answering an error envelope surfaces as an Err.
    #[tokio::test]
    #[cfg(unix)]
    async fn fetch_snapshot_surfaces_daemon_error() {
        use tokio::io::{AsyncWriteExt, BufReader};

        let dir = std::env::temp_dir().join(format!(
            "lanlink-tui-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fake.sock");
        let _ = std::fs::remove_file(&path);

        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            use tokio::io::AsyncBufReadExt;
            reader.read_line(&mut line).await.unwrap();
            let reply = reader.get_mut();
            reply
                .write_all(b"{\"error\":\"not ready\"}\n")
                .await
                .unwrap();
        });

        let err = fetch_snapshot(&path).await.expect_err("must fail");
        assert!(err.to_string().contains("not ready"), "got: {err}");

        server.await.unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[tokio::test]
    async fn missing_socket_fails_cleanly() {
        let err = fetch_snapshot(Path::new("/nonexistent/lanlinkd-control.sock"))
            .await
            .expect_err("must fail");
        assert!(!err.to_string().is_empty());
    }
}
