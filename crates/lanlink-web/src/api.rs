//! HTTP API handlers and daemon status-socket client for `lanlink-web`.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use lanlink_client::{discover_socket, fetch_snapshot};
use lanlink_types::{NodeId};
use lanlink_proto::{CreateNetworkRequest, CreateNetworkResponse, JoinNetworkRequest, JoinNetworkResponse, PublicNetworkInfo};
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
    /// auto-discover a listening `/run/lanlink/lanlinkd-control.sock` or
    /// `/tmp/lanlinkd-control.sock`.
    pub async fn resolve_socket(&self) -> Result<Option<PathBuf>, String> {
        let guard = self.status_socket.lock().await;
        if let Some(path) = guard.as_ref() {
            return Ok(Some(path.clone()));
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
        Ok(Some(path)) => match fetch_snapshot(&path).await {
            Ok(snap) => (StatusCode::OK, Json(json!({"snapshot": snap}))).into_response(),
            Err(e) => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": format!("daemon status error: {e}")})),
            )
                .into_response(),
        },
        Ok(None) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "no daemon status socket found"})),
        )
            .into_response(),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": e}))).into_response(),
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

// Daemon status-socket I/O is delegated to the shared `lanlink-client` crate
// (`fetch_snapshot`, `discover_socket`).

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

    #[tokio::test]
    async fn snapshot_endpoint_returns_503_when_daemon_unreachable() {
        let state = AppState {
            coordinator: "http://localhost:8080".into(),
            status_socket: Arc::new(Mutex::new(Some(PathBuf::from(
                "/nonexistent/lanlinkd-control.sock",
            )))),
            http: reqwest::Client::new(),
        };
        let resp = snapshot(State(state)).await.into_response();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
