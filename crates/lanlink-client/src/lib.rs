//! Shared daemon socket client for LanLink frontends.
//!
//! Provides auto-discovery of the daemon control/status socket and a
//! newline-delimited JSON (NDJSON) exchange over Unix domain sockets (Unix)
//! or TCP loopback (Windows).

use std::path::{Path, PathBuf};

use anyhow::Context;
use lanlink_proto::control::{ControlRequest, ControlResponse, ControlResult};
use lanlink_proto::status::StatusSnapshot;

/// Name of the Unix domain control/status socket created by the daemon.
pub const CONTROL_SOCKET: &str = "lanlinkd-control.sock";

/// Default directories where the daemon may place its control socket.
#[cfg(unix)]
const DIRS: &[&str] = &["/run/lanlink", "/tmp"];

/// Path of the live daemon config written by the installer / `service up`
/// (matches `CONFIG_PATH` in `crates/lanlinkd/src/service.rs`).
pub const DAEMON_CONFIG: &str = "C:\\Program Files\\LanLink\\lanlinkd.toml";

/// Read the configured `[status] socket` from the daemon config, if the
/// config exists and parses. The control/status socket reuses that config key.
#[cfg(windows)]
pub fn configured_status_socket() -> Option<String> {
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

#[cfg(not(windows))]
pub fn configured_status_socket() -> Option<String> {
    None
}

/// Find a listening daemon control/status socket when no override is given.
#[cfg(unix)]
pub async fn discover_socket() -> Result<Option<PathBuf>, String> {
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

/// Find a listening daemon TCP control/status socket on Windows.
#[cfg(windows)]
pub async fn discover_socket() -> Result<Option<PathBuf>, String> {
    if let Some(configured) = configured_status_socket() {
        if let Ok(addr) = configured.parse::<std::net::SocketAddr>() {
            if tokio::net::TcpStream::connect(addr).await.is_ok() {
                return Ok(Some(PathBuf::from(addr.to_string())));
            }
        }
    }
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 19000));
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

/// Send a control request to the daemon and return its response.
///
/// The socket is spoken as NDJSON: one JSON request line, half-close, one
/// JSON response line. On Unix `path` is a filesystem path to a Unix domain
/// socket; on Windows it is the string form of a TCP `SocketAddr`.
pub async fn send_command(path: &Path, req: ControlRequest) -> anyhow::Result<ControlResponse> {
    let body = serde_json::to_string(&req).context("serialize control request")?;
    let text = ndjson_exchange(path, &body).await?;
    let line = first_response_line(&text)?;
    serde_json::from_str::<ControlResponse>(line)
        .map_err(|e| anyhow::anyhow!("invalid control response: {e}"))
}

/// Fetch the current status snapshot from the daemon.
///
/// This sends [`ControlRequest::Status`] over the control socket and extracts
/// the snapshot from the response. The same socket accepts read-only status
/// requests, so this works against every supported frontend target.
pub async fn fetch_snapshot(path: &Path) -> anyhow::Result<StatusSnapshot> {
    match send_command(path, ControlRequest::Status).await? {
        ControlResponse::Ok {
            result: ControlResult::Status(snapshot),
        } => Ok(snapshot),
        ControlResponse::Ok { .. } => anyhow::bail!("unexpected control response"),
        ControlResponse::Error { error } => anyhow::bail!("daemon: {error}"),
    }
}

async fn ndjson_exchange(path: &Path, request_line: &str) -> anyhow::Result<String> {
    #[cfg(unix)]
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::UnixStream::connect(path)
            .await
            .with_context(|| format!("connect to daemon control socket {}", path.display()))?;
        stream
            .write_all(request_line.as_bytes())
            .await
            .context("write request")?;
        stream.write_all(b"\n").await.context("write newline")?;
        stream.shutdown().await.context("shutdown write half")?;

        let mut buf = Vec::new();
        stream
            .read_to_end(&mut buf)
            .await
            .context("read response")?;
        String::from_utf8(buf).map_err(|e| anyhow::anyhow!("invalid UTF-8 from daemon: {e}"))
    }
    #[cfg(windows)]
    {
        use std::net::SocketAddr;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let addr: SocketAddr = path
            .to_str()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| anyhow::anyhow!("invalid TCP control address: {}", path.display()))?;
        let mut stream = tokio::net::TcpStream::connect(addr)
            .await
            .with_context(|| format!("connect to daemon TCP control socket {addr}"))?;
        stream
            .write_all(request_line.as_bytes())
            .await
            .context("write request")?;
        stream.write_all(b"\n").await.context("write newline")?;
        stream.shutdown().await.context("shutdown write half")?;

        let mut buf = Vec::new();
        stream
            .read_to_end(&mut buf)
            .await
            .context("read response")?;
        String::from_utf8(buf).map_err(|e| anyhow::anyhow!("invalid UTF-8 from daemon: {e}"))
    }
}

fn first_response_line(text: &str) -> anyhow::Result<&str> {
    text.lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty response from daemon control socket"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lanlink_proto::status::{DaemonStatus, PathKind, PeerStatus};
    use lanlink_types::{NodeId, VirtualIp};
use lanlink_proto::{NetworkId};

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
    async fn send_command_round_trip() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let dir = std::env::temp_dir().join(format!(
            "lanlink-client-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control.sock");
        let _ = std::fs::remove_file(&path);

        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let canned = serde_json::to_string(&ControlResponse::Ok {
            result: ControlResult::Ack,
        })
        .unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            assert!(
                line.contains("\"cmd\":\"disconnect\""),
                "request was {line:?}"
            );
            let reply = reader.get_mut();
            reply.write_all(canned.as_bytes()).await.unwrap();
            reply.write_all(b"\n").await.unwrap();
        });

        let result = send_command(&path, ControlRequest::Disconnect)
            .await
            .expect("send_command");
        assert!(matches!(result, ControlResponse::Ok { .. }));

        server.await.unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn fetch_snapshot_round_trip() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let dir = std::env::temp_dir().join(format!(
            "lanlink-client-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control.sock");
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
        assert!(result.status.connected);
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
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let dir = std::env::temp_dir().join(format!(
            "lanlink-client-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control.sock");
        let _ = std::fs::remove_file(&path);

        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let reply = reader.get_mut();
            reply
                .write_all(b"{\"status\":\"error\",\"data\":{\"error\":\"not ready\"}}\n")
                .await
                .unwrap();
        });

        let err = fetch_snapshot(&path).await.expect_err("must fail");
        assert!(err.to_string().contains("not ready"), "got: {err}");

        server.await.unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    /// An empty response from the daemon must surface as a clear error instead
    /// of being masked as a JSON parse failure.
    #[tokio::test]
    #[cfg(unix)]
    async fn empty_response_surfaces_explicit_error() {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let dir = std::env::temp_dir().join(format!(
            "lanlink-client-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control.sock");
        let _ = std::fs::remove_file(&path);

        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            // Write nothing back (zero bytes).
            let _ = line;
        });

        let err = fetch_snapshot(&path).await.expect_err("must fail");
        assert!(
            err.to_string()
                .contains("empty response from daemon control socket"),
            "got: {err}"
        );

        server.await.unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    /// When no daemon is listening, discovery returns `None` instead of an error.
    #[tokio::test]
    #[cfg(unix)]
    async fn discover_socket_returns_none_when_no_daemon() {
        // Make sure the well-known paths are not occupied by a listening socket.
        for dir in ["/run/lanlink", "/tmp"] {
            let path = PathBuf::from(dir).join(CONTROL_SOCKET);
            let _ = std::fs::remove_file(&path);
        }
        let result = discover_socket().await;
        assert_eq!(result.unwrap(), None);
    }

    /// A listening socket at one of the well-known paths is discovered.
    #[tokio::test]
    #[cfg(unix)]
    async fn discover_socket_finds_listening_socket() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let dir = std::env::temp_dir().join(format!(
            "lanlink-client-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lanlinkd-control.sock");
        let _ = std::fs::remove_file(&path);

        let listener = tokio::net::UnixListener::bind(&path).unwrap();

        // Create a symlink in /tmp pointing to our temp socket so the shared
        // helper's fixed search path finds it, then clean it up afterwards.
        let tmp_link = PathBuf::from("/tmp").join(CONTROL_SOCKET);
        let _ = std::fs::remove_file(&tmp_link);
        std::os::unix::fs::symlink(&path, &tmp_link).unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1];
            let _ = stream.read(&mut buf).await;
            let _ = stream.write_all(b"x").await;
        });

        let discovered = discover_socket()
            .await
            .expect("discovery should not error")
            .expect("should find the listening socket");
        assert_eq!(discovered, tmp_link);

        server.abort();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&tmp_link);
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
