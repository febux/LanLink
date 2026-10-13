//! Local read-only status API for `lanlinkd`, served over a Unix domain
//! socket (named pipe equivalent is stubbed on Windows until the Wintun
//! backend lands).
//!
//! The socket speaks newline-delimited JSON: one [`StatusRequest`] per line
//! in, one [`lanlink_proto::status::StatusResponse`] per line out. Multiple
//! requests per connection are allowed; a half-close ends the session. The
//! protocol is read-only and exposes no secrets.
//!
//! Failure to create the socket never aborts the daemon: the data plane is
//! the critical path, the status API is a convenience for frontends.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use lanlink_proto::status::{DaemonStatus, PathKind, PeerStatus, StatusResponse, StatusSnapshot};
use lanlink_proto::{NodeId, VirtualIp};
use tokio::sync::Mutex;

use crate::Metrics;

/// Default socket location for a joined network (root-created, mode 0660).
fn default_socket_path(network_id: u64) -> PathBuf {
    PathBuf::from(format!("/run/lanlink/lanlinkd-{network_id}.sock"))
}

/// Unprivileged fallback used when `/run/lanlink` cannot be created (dev).
fn fallback_socket_path(network_id: u64) -> PathBuf {
    PathBuf::from(format!("/tmp/lanlinkd-{network_id}.sock"))
}

/// Static info about this daemon run, set once after a successful join.
#[derive(Debug, Clone)]
pub struct DaemonInfo {
    pub network_id: u64,
    pub assigned_ip: std::net::Ipv4Addr,
    pub tap_name: String,
    pub bind: SocketAddr,
    pub gateway_enabled: bool,
    pub firewall_enabled: bool,
    pub relay_addr: Option<SocketAddr>,
    /// Every relay server the network may use (used to classify peer paths).
    pub relay_servers: Vec<SocketAddr>,
}

/// Coordinator-view metadata for a peer (virtual IP, gateway role). Kept
/// apart from the switch, which only tracks data-plane state.
#[derive(Debug, Clone, Default)]
pub struct PeerMeta {
    pub virtual_ip: Option<VirtualIp>,
    pub is_gateway: bool,
    pub shared_subnet: String,
}

/// Shared state written by the daemon tasks and read by the status server.
#[derive(Debug, Clone, Default)]
pub struct StatusRegistry {
    info: Arc<std::sync::RwLock<Option<DaemonInfo>>>,
    peer_meta: Arc<std::sync::RwLock<HashMap<NodeId, PeerMeta>>>,
}

impl StatusRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the one-time daemon info after joining a network.
    pub fn set_info(&self, info: DaemonInfo) {
        *self.info.write().unwrap() = Some(info);
    }

    /// Replace the coordinator-view peer metadata.
    pub fn update_peers(&self, peers: &[lanlink_proto::PeerInfo]) {
        let mut meta = HashMap::with_capacity(peers.len());
        for p in peers {
            meta.insert(
                p.node_id,
                PeerMeta {
                    virtual_ip: Some(p.virtual_ip),
                    is_gateway: p.is_gateway,
                    shared_subnet: p.shared_subnet.clone(),
                },
            );
        }
        *self.peer_meta.write().unwrap() = meta;
    }
}

/// Everything the status server needs to assemble a snapshot.
#[derive(Clone)]
pub struct StatusState {
    pub registry: StatusRegistry,
    pub switch: Arc<Mutex<lanlink_switch::Switch>>,
    pub metrics: Arc<Metrics>,
    pub punching: Arc<Mutex<HashSet<NodeId>>>,
    pub start: Instant,
}

impl StatusState {
    /// Assemble a full snapshot. The switch lock is held only to copy
    /// per-peer stats; it is never held across an await.
    pub async fn snapshot(&self) -> StatusSnapshot {
        let stats = self.switch.lock().await.peer_stats();
        let punching = self.punching.lock().await;
        let meta = self.peer_meta_snapshot();
        let info = self.info_snapshot();

        let mut peers: Vec<PeerStatus> = meta
            .iter()
            .map(|(id, m)| {
                let stat = stats.iter().find(|s| s.node_id == *id);
                let endpoint = stat.map(|s| s.addr);
                let path = classify_path(*id, endpoint, &punching, info.as_ref());
                PeerStatus {
                    node_id: *id,
                    virtual_ip: m
                        .virtual_ip
                        .unwrap_or(VirtualIp(std::net::Ipv4Addr::UNSPECIFIED)),
                    endpoint,
                    path,
                    latency_ms: stat.and_then(|s| s.rtt).map(|d| d.as_millis() as u64),
                    last_seen_secs_ago: stat.map(|s| s.last_seen.elapsed().as_secs()),
                    tx_bytes: stat.map(|s| s.tx_bytes).unwrap_or(0),
                    rx_bytes: stat.map(|s| s.rx_bytes).unwrap_or(0),
                    is_gateway: m.is_gateway,
                    shared_subnet: m.shared_subnet.clone(),
                }
            })
            .collect();
        peers.sort_by_key(|p| p.virtual_ip.0);

        let status = DaemonStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            network_id: lanlink_proto::NetworkId(info.as_ref().map(|i| i.network_id).unwrap_or(0)),
            assigned_ip: info
                .as_ref()
                .map(|i| VirtualIp(i.assigned_ip))
                .unwrap_or(VirtualIp(std::net::Ipv4Addr::UNSPECIFIED)),
            tap_name: info
                .as_ref()
                .map(|i| i.tap_name.clone())
                .unwrap_or_default(),
            bind: info
                .as_ref()
                .map(|i| i.bind)
                .unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], 0))),
            relay_addr: info.as_ref().and_then(|i| i.relay_addr),
            gateway_enabled: info.as_ref().map(|i| i.gateway_enabled).unwrap_or(false),
            firewall_enabled: info.as_ref().map(|i| i.firewall_enabled).unwrap_or(false),
            uptime_seconds: self.start.elapsed().as_secs(),
            tx_bytes: self.metrics.tx_bytes(),
            rx_bytes: self.metrics.rx_bytes(),
        };

        StatusSnapshot { status, peers }
    }

    fn peer_meta_snapshot(&self) -> HashMap<NodeId, PeerMeta> {
        self.registry.peer_meta.read().unwrap().clone()
    }

    fn info_snapshot(&self) -> Option<DaemonInfo> {
        self.registry.info.read().unwrap().clone()
    }
}

/// Peer path classification: punching wins over everything (a verified path
/// may already exist while a re-punch runs), then relay membership, then
/// direct; no endpoint at all means we are still waiting.
fn classify_path(
    id: NodeId,
    endpoint: Option<SocketAddr>,
    punching: &HashSet<NodeId>,
    info: Option<&DaemonInfo>,
) -> PathKind {
    if punching.contains(&id) {
        return PathKind::Punching;
    }
    match endpoint {
        Some(addr) => {
            let is_relay = info
                .map(|i| i.relay_servers.contains(&addr))
                .unwrap_or(false);
            if is_relay {
                PathKind::Relay
            } else {
                PathKind::Direct
            }
        }
        None => PathKind::Waiting,
    }
}

/// Bind the status listener, trying the configured path (or the default with
/// a `/tmp` fallback). Spawns the accept loop; failure is logged and the
/// daemon continues without a status API.
pub async fn spawn_server(configured: Option<String>, network_id: u64, state: StatusState) {
    let candidates: Vec<PathBuf> = match configured {
        Some(p) => vec![PathBuf::from(p)],
        None => vec![
            default_socket_path(network_id),
            fallback_socket_path(network_id),
        ],
    };

    tokio::spawn(async move {
        for path in candidates {
            match bind_listener(&path) {
                Ok(listener) => {
                    tracing::info!(path = %path.display(), "status socket listening");
                    run_server(listener, &path, state).await;
                    return;
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "status socket bind failed"
                    );
                }
            }
        }
        tracing::warn!("status server disabled: no usable socket path");
    });
}

#[cfg(unix)]
fn bind_listener(path: &PathBuf) -> std::io::Result<tokio::net::UnixListener> {
    use std::os::unix::fs::PermissionsExt;

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Remove a stale socket left by a crashed previous run.
    let _ = std::fs::remove_file(path);
    let listener = tokio::net::UnixListener::bind(path)?;
    // Restrict to owner + group (root:lanlink in production; the running user
    // in dev). The data exposed is operational status, not secrets, but it
    // still should not be world-readable.
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660));
    Ok(listener)
}

#[cfg(unix)]
async fn run_server(listener: tokio::net::UnixListener, path: &PathBuf, state: StatusState) {
    loop {
        tokio::select! {
            _ = crate::shutdown().wait() => {
                tracing::debug!("status server stopping");
                break;
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => {
                        let state = state.clone();
                        tokio::spawn(handle_conn(stream, state));
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "status accept failed");
                    }
                }
            }
        }
    }
    let _ = std::fs::remove_file(path);
}

#[cfg(unix)]
async fn handle_conn(stream: tokio::net::UnixStream, state: StatusState) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // client closed the connection
            Ok(_) => {}
            Err(_) => break,
        }

        let response =
            match serde_json::from_str::<lanlink_proto::status::StatusRequest>(line.trim_end()) {
                Ok(lanlink_proto::status::StatusRequest::Snapshot) => {
                    StatusResponse::Snapshot(state.snapshot().await)
                }
                Err(e) => StatusResponse::Error {
                    error: format!("bad request: {e}"),
                },
            };

        let mut out = serde_json::to_string(&response)
            .unwrap_or_else(|_| "{\"error\":\"snapshot encode failed\"}".to_string());
        out.push('\n');
        if reader.get_mut().write_all(out.as_bytes()).await.is_err() {
            break;
        }
    }
}

#[cfg(not(unix))]
struct DummyListener;

#[cfg(not(unix))]
fn bind_listener(_path: &PathBuf) -> std::io::Result<DummyListener> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "status socket is not supported on this platform yet",
    ))
}

#[cfg(not(unix))]
async fn run_server(_listener: DummyListener, _path: &PathBuf, _state: StatusState) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(id: u64, ip: &str, relay: Option<SocketAddr>) -> lanlink_proto::PeerInfo {
        lanlink_proto::PeerInfo {
            node_id: NodeId(id.to_be_bytes()),
            virtual_ip: VirtualIp(ip.parse().unwrap()),
            direct_candidates: vec![],
            relay_candidates: relay.into_iter().collect(),
            is_gateway: false,
            shared_subnet: String::new(),
        }
    }

    fn test_state() -> StatusState {
        let registry = StatusRegistry::new();
        let relay: SocketAddr = "203.0.113.9:3478".parse().unwrap();
        registry.set_info(DaemonInfo {
            network_id: 5,
            assigned_ip: "26.0.0.1".parse().unwrap(),
            tap_name: "lanlink0".into(),
            bind: "0.0.0.0:9000".parse().unwrap(),
            gateway_enabled: false,
            firewall_enabled: false,
            relay_addr: Some(relay),
            relay_servers: vec![relay],
        });
        registry.update_peers(&[
            peer(1, "26.0.0.2", None),
            peer(2, "26.0.0.3", Some(relay)),
            peer(3, "26.0.0.4", None),
        ]);

        let session = lanlink_crypto::StaticSession::new([0u8; 32]);
        let mut switch = lanlink_switch::Switch::new(NodeId(9u64.to_be_bytes()), session);
        switch.add_peer(
            NodeId(1u64.to_be_bytes()),
            "198.51.100.7:5000".parse().unwrap(),
        );
        switch.add_peer(NodeId(2u64.to_be_bytes()), relay);
        let switch = Arc::new(Mutex::new(switch));

        let mut punching = HashSet::new();
        punching.insert(NodeId(3u64.to_be_bytes()));

        StatusState {
            registry,
            switch,
            metrics: Arc::new(Metrics::default()),
            punching: Arc::new(Mutex::new(punching)),
            start: Instant::now(),
        }
    }

    #[tokio::test]
    async fn snapshot_classifies_paths_and_merges_meta() {
        let state = test_state();
        let snap = state.snapshot().await;
        assert_eq!(snap.status.network_id.0, 5);
        assert_eq!(
            snap.status.relay_addr.map(|a| a.to_string()),
            Some("203.0.113.9:3478".to_string())
        );

        // Sorted by virtual IP: 26.0.0.2, .3, .4
        assert_eq!(snap.peers.len(), 3);
        assert_eq!(snap.peers[0].path, PathKind::Direct);
        assert_eq!(snap.peers[0].virtual_ip.0.to_string(), "26.0.0.2");
        assert_eq!(snap.peers[1].path, PathKind::Relay);
        assert_eq!(snap.peers[2].path, PathKind::Punching);
        assert_eq!(snap.peers[2].endpoint, None);
    }

    #[tokio::test]
    async fn snapshot_without_info_uses_defaults() {
        let session = lanlink_crypto::StaticSession::new([0u8; 32]);
        let switch = Arc::new(Mutex::new(lanlink_switch::Switch::new(
            NodeId(1u64.to_be_bytes()),
            session,
        )));
        let state = StatusState {
            registry: StatusRegistry::new(),
            switch,
            metrics: Arc::new(Metrics::default()),
            punching: Arc::new(Mutex::new(HashSet::new())),
            start: Instant::now(),
        };
        let snap = state.snapshot().await;
        assert_eq!(snap.status.network_id.0, 0);
        assert!(snap.peers.is_empty());
    }

    /// End-to-end NDJSON round trip over a real Unix domain socket: the
    /// server handler used in production answers a raw client request.
    #[cfg(unix)]
    #[tokio::test]
    async fn status_socket_round_trip() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let dir = std::env::temp_dir().join(format!(
            "lanlink-status-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("status.sock");

        let state = test_state();
        let listener = bind_listener(&path).expect("bind test socket");
        assert!(path.exists(), "socket file created");

        let client_path = path.clone();
        let client = tokio::spawn(async move {
            let mut stream = tokio::net::UnixStream::connect(&client_path)
                .await
                .expect("connect");
            stream
                .write_all(b"{\"cmd\":\"snapshot\"}\n")
                .await
                .expect("write request");
            stream.shutdown().await.expect("half-close");
            let mut buf = String::new();
            stream.read_to_string(&mut buf).await.expect("read reply");
            buf
        });

        let (conn, _) = listener.accept().await.expect("accept");
        handle_conn(conn, state).await;

        let response = client.await.expect("client task");
        let parsed: lanlink_proto::status::StatusResponse =
            serde_json::from_str(response.trim_end()).expect("parse response");
        match parsed {
            lanlink_proto::status::StatusResponse::Snapshot(snap) => {
                assert_eq!(snap.status.network_id.0, 5);
                assert_eq!(snap.peers.len(), 3);
                assert_eq!(snap.peers[0].path, PathKind::Direct);
            }
            lanlink_proto::status::StatusResponse::Error { error } => {
                panic!("unexpected error response: {error}")
            }
        }

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    /// A malformed request line must answer with an error envelope, not kill
    /// the connection handler.
    #[cfg(unix)]
    #[tokio::test]
    async fn status_socket_bad_request_returns_error() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let dir = std::env::temp_dir().join(format!(
            "lanlink-status-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("status.sock");

        let state = test_state();
        let listener = bind_listener(&path).expect("bind test socket");

        let client_path = path.clone();
        let client = tokio::spawn(async move {
            let mut stream = tokio::net::UnixStream::connect(&client_path)
                .await
                .expect("connect");
            stream.write_all(b"not json\n").await.expect("write");
            stream.shutdown().await.expect("half-close");
            let mut buf = String::new();
            stream.read_to_string(&mut buf).await.expect("read");
            buf
        });

        let (conn, _) = listener.accept().await.expect("accept");
        handle_conn(conn, state).await;

        let response = client.await.expect("client task");
        let parsed: lanlink_proto::status::StatusResponse =
            serde_json::from_str(response.trim_end()).expect("parse response");
        assert!(
            matches!(
                &parsed,
                lanlink_proto::status::StatusResponse::Error { error } if error.contains("bad request")
            ),
            "expected bad-request error, got {parsed:?}"
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
}
