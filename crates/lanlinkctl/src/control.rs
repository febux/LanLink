//! Local control/status client for `lanlinkctl`.
//!
//! Speaks the same newline-delimited JSON protocol as the daemon's
//! [`lanlink_proto::control`] server. Tries the Unix-domain control socket by
//! default (TCP loopback on Windows) and allows an explicit `--socket` override.

use base64::Engine;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

use lanlink_proto::control::{ControlRequest, ControlResponse};

/// Candidate control endpoints, in preference order.
fn control_candidates(override_socket: Option<String>) -> Vec<String> {
    if let Some(s) = override_socket {
        return vec![s];
    }
    if let Ok(env) = std::env::var("LANLINK_CONTROL_SOCKET") {
        return vec![env];
    }

    #[cfg(unix)]
    {
        vec![
            "/run/lanlink/lanlinkd-control.sock".into(),
            "/tmp/lanlinkd-control.sock".into(),
        ]
    }
    #[cfg(not(unix))]
    {
        vec!["127.0.0.1:19000".into()]
    }
}

#[cfg(unix)]
async fn connect_unix(candidates: &[String]) -> anyhow::Result<tokio::net::UnixStream> {
    let mut last_err = None;
    for path in candidates {
        match tokio::net::UnixStream::connect(path).await {
            Ok(s) => return Ok(s),
            Err(e) => last_err = Some((path.clone(), e)),
        }
    }
    Err(anyhow::anyhow!(
        "could not connect to lanlinkd control socket (tried {:?}): {:?}",
        candidates,
        last_err
    ))
}

#[cfg(not(unix))]
async fn connect_tcp(candidates: &[String]) -> anyhow::Result<tokio::net::TcpStream> {
    let mut last_err = None;
    for spec in candidates {
        let addr: std::net::SocketAddr = match spec.parse() {
            Ok(a) => a,
            Err(e) => {
                last_err = Some((spec.clone(), e));
                continue;
            }
        };
        match tokio::net::TcpStream::connect(addr).await {
            Ok(s) => return Ok(s),
            Err(e) => last_err = Some((spec.clone(), e)),
        }
    }
    Err(anyhow::anyhow!(
        "could not connect to lanlinkd control TCP socket (tried {:?}): {:?}",
        candidates,
        last_err
    ))
}

/// Send a single request and read the matching response line.
pub async fn send_request(
    req: ControlRequest,
    override_socket: Option<String>,
) -> anyhow::Result<ControlResponse> {
    let candidates = control_candidates(override_socket);

    #[cfg(unix)]
    {
        let mut stream = connect_unix(&candidates).await?;
        transact_unix(&mut stream, &req).await
    }
    #[cfg(not(unix))]
    {
        let mut stream = connect_tcp(&candidates).await?;
        transact_tcp(&mut stream, &req).await
    }
}

#[cfg(unix)]
async fn transact_unix(
    stream: &mut tokio::net::UnixStream,
    req: &ControlRequest,
) -> anyhow::Result<ControlResponse> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let body = serde_json::to_string(req)?;
    stream.write_all(body.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.shutdown().await?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    if line.is_empty() {
        anyhow::bail!("control socket closed without response");
    }
    serde_json::from_str(line.trim_end())
        .map_err(|e| anyhow::anyhow!("invalid control response: {e}"))
}

#[cfg(not(unix))]
async fn transact_tcp(
    stream: &mut tokio::net::TcpStream,
    req: &ControlRequest,
) -> anyhow::Result<ControlResponse> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let body = serde_json::to_string(req)?;
    stream.write_all(body.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.shutdown().await?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    if line.is_empty() {
        anyhow::bail!("control socket closed without response");
    }
    serde_json::from_str(line.trim_end())
        .map_err(|e| anyhow::anyhow!("invalid control response: {e}"))
}

/// Send a stop request, wait for the ack, then wait for the daemon process to
/// release the control socket. The CLI only returns after the socket is closed.
pub async fn stop_daemon(override_socket: Option<String>, timeout: Duration) -> anyhow::Result<()> {
    let candidates = control_candidates(override_socket);

    #[cfg(unix)]
    {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let mut stream = connect_unix(&candidates).await?;
        let req = ControlRequest::Stop;
        let body = serde_json::to_string(&req)?;
        tokio::io::AsyncWriteExt::write_all(&mut stream, body.as_bytes()).await?;
        tokio::io::AsyncWriteExt::write_all(&mut stream, b"\n").await?;
        stream.shutdown().await?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        if line.is_empty() {
            // Socket already closed: daemon is gone.
            return Ok(());
        }
        let resp: ControlResponse = serde_json::from_str(line.trim_end())
            .map_err(|e| anyhow::anyhow!("invalid stop response: {e}"))?;
        match resp {
            ControlResponse::Ok { .. } => {}
            ControlResponse::Error { error } => anyhow::bail!("daemon refused stop: {error}"),
        }

        match tokio::time::timeout(timeout, reader.read_line(&mut line)).await {
            Ok(Ok(0)) => Ok(()),
            Ok(Ok(_)) => Err(anyhow::anyhow!("daemon sent unexpected data after stop ack")),
            Ok(Err(e)) => Err(anyhow::anyhow!("control socket error while waiting for shutdown: {e}")),
            Err(_) => Err(anyhow::anyhow!(
                "timed out waiting for daemon to exit after stop ack"
            )),
        }
    }
    #[cfg(not(unix))]
    {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let mut stream = connect_tcp(&candidates).await?;
        let req = ControlRequest::Stop;
        let body = serde_json::to_string(&req)?;
        tokio::io::AsyncWriteExt::write_all(&mut stream, body.as_bytes()).await?;
        tokio::io::AsyncWriteExt::write_all(&mut stream, b"\n").await?;
        stream.shutdown().await?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        if line.is_empty() {
            return Ok(());
        }
        let resp: ControlResponse = serde_json::from_str(line.trim_end())
            .map_err(|e| anyhow::anyhow!("invalid stop response: {e}"))?;
        match resp {
            ControlResponse::Ok { .. } => {}
            ControlResponse::Error { error } => anyhow::bail!("daemon refused stop: {error}"),
        }

        match tokio::time::timeout(timeout, reader.read_line(&mut line)).await {
            Ok(Ok(0)) => Ok(()),
            Ok(Ok(_)) => Err(anyhow::anyhow!("daemon sent unexpected data after stop ack")),
            Ok(Err(e)) => Err(anyhow::anyhow!("control socket error while waiting for shutdown: {e}")),
            Err(_) => Err(anyhow::anyhow!(
                "timed out waiting for daemon to exit after stop ack"
            )),
        }
    }
}

/// Format a status snapshot for human consumption.
pub fn print_status(snap: &lanlink_proto::status::StatusSnapshot) {
    let s = &snap.status;
    println!("daemon {} (pid {})", s.version, s.pid);
    println!(
        "network: {}  assigned_ip: {}  tap: {}  bind: {}",
        s.network_id.0, s.assigned_ip.0, s.tap_name, s.bind
    );
    println!(
        "connected: {}  gateway: {}  firewall: {}  uptime: {}s",
        s.connected, s.gateway_enabled, s.firewall_enabled, s.uptime_seconds
    );
    println!(
        "relay: {}  tx: {}  rx: {}",
        s.relay_addr
            .as_ref()
            .map(|a| a.to_string())
            .unwrap_or_else(|| "none".into()),
        s.tx_bytes,
        s.rx_bytes
    );
    println!("peers: {}", snap.peers.len());
    for p in &snap.peers {
        println!(
            "  {}  {}  path={}  endpoint={}  latency={}ms  last_seen={}s  tx={}  rx={}  gateway={}",
            base64::engine::general_purpose::STANDARD.encode(p.node_id.0),
            p.virtual_ip.0,
            p.path,
            p.endpoint
                .as_ref()
                .map(|a| a.to_string())
                .unwrap_or_else(|| "-".into()),
            p.latency_ms.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
            p.last_seen_secs_ago
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".into()),
            p.tx_bytes,
            p.rx_bytes,
            p.is_gateway,
        );
    }
}
