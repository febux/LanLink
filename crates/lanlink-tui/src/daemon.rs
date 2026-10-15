//! Daemon status-socket client (Unix domain socket or TCP loopback,
//! newline-delimited JSON).

use lanlink_proto::status::{StatusRequest, StatusResponse, StatusSnapshot};
#[cfg(windows)]
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// Fetch one snapshot over the daemon's Unix status socket.
#[cfg(unix)]
pub async fn fetch_snapshot(path: &Path) -> anyhow::Result<StatusSnapshot> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::UnixStream::connect(path).await?;
    let request = serde_json::to_string(&StatusRequest::Snapshot)?;
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    // Half-close: the daemon processes the line and answers, then sees EOF.
    stream.shutdown().await?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    let text = std::str::from_utf8(&buf)?;
    let line = text.lines().next().unwrap_or("");
    match serde_json::from_str::<StatusResponse>(line)? {
        StatusResponse::Snapshot(snapshot) => Ok(snapshot),
        StatusResponse::Error { error } => anyhow::bail!("daemon: {error}"),
    }
}

/// Fetch one snapshot over the daemon's TCP loopback status socket (Windows).
#[cfg(windows)]
pub async fn fetch_snapshot(path: &Path) -> anyhow::Result<StatusSnapshot> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr: SocketAddr = path
        .to_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("invalid TCP status address: {}", path.display()))?;

    let mut stream = tokio::net::TcpStream::connect(addr).await?;
    let request = serde_json::to_string(&StatusRequest::Snapshot)?;
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.shutdown().await?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    let text = std::str::from_utf8(&buf)?;
    let line = text.lines().next().unwrap_or("");
    match serde_json::from_str::<StatusResponse>(line)? {
        StatusResponse::Snapshot(snapshot) => Ok(snapshot),
        StatusResponse::Error { error } => anyhow::bail!("daemon: {error}"),
    }
}

/// Find a listening daemon socket when `--status-socket` was not given.
#[cfg(unix)]
pub async fn discover_socket() -> Result<Option<PathBuf>, String> {
    const DIRS: &[&str] = &["/run/lanlink", "/tmp"];
    let mut candidates: Vec<PathBuf> = Vec::new();
    for dir in DIRS {
        match std::fs::read_dir(dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if name.starts_with("lanlinkd-") && name.ends_with(".sock") {
                        candidates.push(entry.path());
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                return Err(format!(
                    "permission denied reading {dir}: add your user to the 'lanlink' group with \"sudo usermod -aG lanlink $USER\", then log out and back in (or run \"newgrp lanlink\" in this shell)"
                ));
            }
            Err(_) => {}
        }
    }
    candidates.sort();
    for path in candidates {
        if probe(&path).await {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// Path of the live daemon config written by the MSI / `service up`
/// (matches `CONFIG_PATH` in `crates/lanlinkd/src/service.rs`).
#[cfg(windows)]
const DAEMON_CONFIG: &str = "C:\\Program Files\\LanLink\\lanlinkd.toml";

/// Read the configured `[status] socket` from the daemon config, if the
/// config exists and parses.
#[cfg(windows)]
fn configured_status_socket() -> Option<String> {
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

/// Find a listening daemon TCP status socket on Windows.
#[cfg(windows)]
pub async fn discover_socket() -> Result<Option<PathBuf>, String> {
    // The daemon config is authoritative when it names a status socket.
    if let Some(configured) = configured_status_socket() {
        if let Ok(addr) = configured.parse::<SocketAddr>() {
            if tokio::net::TcpStream::connect(addr).await.is_ok() {
                return Ok(Some(PathBuf::from(addr.to_string())));
            }
        }
    }
    // Default layout: port = 9000 + network_id, ids 1..=32.
    for network_id in 1u64..=32 {
        let addr = SocketAddr::from(([127, 0, 0, 1], (9000u16 + network_id as u16)));
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return Ok(Some(PathBuf::from(addr.to_string())));
        }
    }
    Ok(None)
}

#[cfg(unix)]
async fn probe(path: &Path) -> bool {
    tokio::net::UnixStream::connect(path).await.is_ok()
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
        let canned = serde_json::to_string(&StatusResponse::Snapshot(snapshot)).unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            assert!(line.contains("snapshot"), "request was {line:?}");
            let reply = reader.get_mut();
            reply.write_all(canned.as_bytes()).await.unwrap();
            reply.write_all(b"\n").await.unwrap();
        });

        let result = fetch_snapshot(&path).await.expect("fetch");
        assert_eq!(result.status.network_id.0, 42);
        assert_eq!(result.status.version, "0.1.0-test");
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
        let err = fetch_snapshot(Path::new("/nonexistent/lanlinkd-0.sock"))
            .await
            .expect_err("must fail");
        assert!(!err.to_string().is_empty());
    }
}
