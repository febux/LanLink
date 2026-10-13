//! HTTP API handlers and daemon status-socket client for `lanlink-web`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use lanlink_proto::status::{StatusRequest, StatusResponse, StatusSnapshot};
use lanlink_proto::{
    CreateNetworkRequest, CreateNetworkResponse, JoinNetworkRequest, JoinNetworkResponse, NodeId,
    PublicNetworkInfo,
};
use rand::RngCore;
use serde_json::json;
use tokio::sync::Mutex;

/// Shared server state: coordinator URL, optional status socket override, and a
/// single reused HTTP client for coordinator calls.
#[derive(Clone)]
pub struct AppState {
    pub coordinator: String,
    pub status_socket: Arc<Mutex<Option<PathBuf>>>,
    pub http: reqwest::Client,
}

impl AppState {
    /// Resolve the daemon status socket: use the override if set, otherwise
    /// auto-discover a listening `/run/lanlink/lanlinkd-*.sock` or
    /// `/tmp/lanlinkd-*.sock`.
    pub async fn resolve_socket(&self) -> Option<PathBuf> {
        let guard = self.status_socket.lock().await;
        if let Some(path) = guard.as_ref() {
            return Some(path.clone());
        }
        drop(guard);
        discover_socket().await
    }
}

/// `GET /api/health` — liveness probe.
pub async fn health() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

/// `GET /api/snapshot` — proxy a single snapshot request to the daemon's
/// status socket and return the result as JSON.
pub async fn snapshot(State(state): State<AppState>) -> impl IntoResponse {
    match state.resolve_socket().await {
        Some(path) => match fetch_snapshot(&path).await {
            Ok(snap) => (StatusCode::OK, Json(json!({"snapshot": snap}))).into_response(),
            Err(e) => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": format!("daemon status error: {e}")})),
            )
                .into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "no daemon status socket found"})),
        )
            .into_response(),
    }
}

/// `GET /api/directory` — return the coordinator's public directory.
pub async fn directory(State(state): State<AppState>) -> impl IntoResponse {
    match list_directory(&state.http, &state.coordinator).await {
        Ok(networks) => (StatusCode::OK, Json(json!({"networks": networks}))).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": format!("coordinator: {e}")})),
        )
            .into_response(),
    }
}

/// `POST /api/networks` — create a network through the coordinator.
pub async fn create_network(
    State(state): State<AppState>,
    Json(req): Json<CreateNetworkRequest>,
) -> impl IntoResponse {
    match do_create(&state.http, &state.coordinator, req).await {
        Ok(resp) => (StatusCode::OK, Json(json!({"result": resp}))).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": format!("coordinator: {e}")})),
        )
            .into_response(),
    }
}

/// `POST /api/networks/:id/join` — join a network through the coordinator.
/// The server fills in the node id and placeholder candidates; the browser only
/// sends `password` and optional `shared_subnet`.
pub async fn join_network(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<u64>,
    Json(mut req): Json<JoinNetworkRequest>,
) -> impl IntoResponse {
    let mut node_id = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut node_id);
    req.node_id = NodeId(node_id);
    req.public_key = vec![];
    req.direct_candidates = vec!["127.0.0.1:9000".parse().expect("static addr parses")];
    req.relay_candidates = vec![];

    match do_join(&state.http, &state.coordinator, id, req).await {
        Ok(resp) => (StatusCode::OK, Json(json!({"result": resp}))).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": format!("coordinator: {e}")})),
        )
            .into_response(),
    }
}

/// Fetch one snapshot over the daemon's status socket.
#[cfg(unix)]
pub async fn fetch_snapshot(path: &Path) -> anyhow::Result<StatusSnapshot> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::UnixStream::connect(path).await?;
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

#[cfg(not(unix))]
pub async fn fetch_snapshot(_path: &Path) -> anyhow::Result<StatusSnapshot> {
    anyhow::bail!("the daemon status socket is not supported on this platform")
}

/// Find a listening daemon socket when `--status-socket` was not given.
pub async fn discover_socket() -> Option<PathBuf> {
    const DIRS: &[&str] = &["/run/lanlink", "/tmp"];
    let mut candidates: Vec<PathBuf> = Vec::new();
    for dir in DIRS {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("lanlinkd-") && name.ends_with(".sock") {
                    candidates.push(entry.path());
                }
            }
        }
    }
    candidates.sort();
    for path in candidates {
        if probe(&path).await {
            return Some(path);
        }
    }
    None
}

#[cfg(unix)]
async fn probe(path: &Path) -> bool {
    tokio::net::UnixStream::connect(path).await.is_ok()
}

#[cfg(not(unix))]
async fn probe(_path: &Path) -> bool {
    false
}

/// List public networks from the coordinator directory.
pub async fn list_directory(
    client: &reqwest::Client,
    base: &str,
) -> anyhow::Result<Vec<PublicNetworkInfo>> {
    let networks: Vec<PublicNetworkInfo> = client
        .get(format!("{base}/directory"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(networks)
}

/// Execute create against the coordinator.
async fn do_create(
    client: &reqwest::Client,
    base: &str,
    req: CreateNetworkRequest,
) -> anyhow::Result<CreateNetworkResponse> {
    let resp: CreateNetworkResponse = client
        .post(format!("{base}/networks"))
        .json(&req)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(resp)
}

/// Execute join against the coordinator.
async fn do_join(
    client: &reqwest::Client,
    base: &str,
    network_id: u64,
    req: JoinNetworkRequest,
) -> anyhow::Result<JoinNetworkResponse> {
    let resp: JoinNetworkResponse = client
        .post(format!("{base}/networks/{network_id}/join"))
        .json(&req)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lanlink_proto::status::{DaemonStatus, PathKind, PeerStatus};
    use lanlink_proto::{NetworkId, VirtualIp};

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

    /// Client side of the NDJSON protocol against a minimal fake daemon.
    #[tokio::test]
    #[cfg(unix)]
    async fn fetch_snapshot_round_trip() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let dir = std::env::temp_dir().join(format!(
            "lanlink-web-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("status.sock");
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
        assert_eq!(result.peers.len(), 1);
        assert_eq!(result.peers[0].latency_ms, Some(23));

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
