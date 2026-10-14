use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use lanlink_proto::status::{StatusRequest, StatusResponse, StatusSnapshot};
use lanlink_proto::{
    CreateNetworkRequest, CreateNetworkResponse, JoinNetworkRequest, JoinNetworkResponse, NodeId,
    PublicNetworkInfo,
};
use rand::RngCore;
use reqwest::Client;
use tauri::{AppHandle, State};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Tauri-managed state shared across commands.
pub struct DesktopState {
    pub coordinator: String,
    pub status_socket: Arc<Mutex<Option<PathBuf>>>,
    pub http: Client,
}

impl DesktopState {
    pub fn new(coordinator: String, status_socket: Option<PathBuf>) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client");
        Self {
            coordinator,
            status_socket: Arc::new(Mutex::new(status_socket)),
            http,
        }
    }

    pub async fn resolve_socket(&self) -> Result<Option<PathBuf>, String> {
        let guard = self.status_socket.lock().await;
        if let Some(path) = guard.as_ref() {
            debug!(path = %path.display(), "using explicit status socket");
            return Ok(Some(path.clone()));
        }
        drop(guard);
        match discover_socket().await {
            Ok(Some(path)) => {
                info!(path = %path.display(), "auto-discovered daemon status socket");
                Ok(Some(path))
            }
            Ok(None) => {
                warn!("no daemon status socket found in /run/lanlink or /tmp");
                Ok(None)
            }
            Err(e) => {
                warn!(error = %e, "daemon status socket discovery failed");
                Err(e)
            }
        }
    }
}

/// Read the daemon status snapshot via the local Unix socket.
#[tauri::command]
pub async fn get_snapshot(state: State<'_, DesktopState>) -> Result<StatusSnapshot, String> {
    let path = match state.resolve_socket().await {
        Ok(Some(path)) => path,
        Ok(None) => return Err("no daemon status socket found in /run/lanlink or /tmp".to_string()),
        Err(e) => return Err(e),
    };
    debug!(path = %path.display(), "fetching daemon snapshot");
    fetch_snapshot(&path).await.map_err(|e| {
        warn!(path = %path.display(), error = %e, "failed to fetch daemon snapshot");
        format!("daemon status error on {}: {e}", path.display())
    })
}

/// Expose which status socket the desktop app is using, so the UI can show it
/// in the about/status panel.
#[tauri::command]
pub async fn resolve_status_socket(
    state: State<'_, DesktopState>,
) -> Result<Option<String>, String> {
    match state.resolve_socket().await {
        Ok(opt) => Ok(opt.map(|p| p.display().to_string())),
        Err(e) => Err(e),
    }
}

/// List public networks from the coordinator directory.
#[tauri::command]
pub async fn list_directory(
    state: State<'_, DesktopState>,
) -> Result<Vec<PublicNetworkInfo>, String> {
    let url = state.coordinator.clone();
    let client = state.http.clone();
    tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Handle::current();
        runtime.block_on(async move {
            let networks: Vec<PublicNetworkInfo> = client
                .get(format!("{url}/directory"))
                .send()
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            Ok(networks)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Create a network through the coordinator.
#[tauri::command]
pub async fn create_network(
    state: State<'_, DesktopState>,
    name: String,
    password: String,
    is_public: bool,
) -> Result<CreateNetworkResponse, String> {
    let req = CreateNetworkRequest {
        name,
        password,
        is_public,
    };
    do_create(&state.http, &state.coordinator, req)
        .await
        .map_err(|e| e.to_string())
}

/// Join a network through the coordinator. The desktop app fills in the node
/// id and placeholder candidates; the UI only sends the password and an
/// optional shared subnet.
#[tauri::command]
pub async fn join_network(
    state: State<'_, DesktopState>,
    network_id: u64,
    password: String,
    shared_subnet: Option<String>,
) -> Result<JoinNetworkResponse, String> {
    let mut node_id = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut node_id);
    let req = JoinNetworkRequest {
        password,
        node_id: NodeId(node_id),
        public_key: vec![],
        direct_candidates: vec!["127.0.0.1:9000".parse().expect("static addr parses")],
        relay_candidates: vec![],
        shared_subnet: shared_subnet.unwrap_or_default(),
    };
    do_join(&state.http, &state.coordinator, network_id, req)
        .await
        .map_err(|e| e.to_string())
}

/// Open a browser to the LanLink URL (used for help or advanced web UI).
#[tauri::command]
pub async fn open_browser(_app: AppHandle, url: String) -> Result<(), String> {
    open::that(url).map_err(|e| e.to_string())
}

async fn fetch_snapshot(path: &Path) -> anyhow::Result<StatusSnapshot> {
    #[cfg(unix)]
    {
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
    {
        let _ = path;
        anyhow::bail!("the daemon status socket is not supported on this platform")
    }
}

async fn discover_socket() -> Result<Option<PathBuf>, String> {
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

#[cfg(unix)]
async fn probe(path: &Path) -> bool {
    tokio::net::UnixStream::connect(path).await.is_ok()
}

#[cfg(not(unix))]
async fn probe(_path: &Path) -> bool {
    false
}

async fn do_create(
    client: &Client,
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

async fn do_join(
    client: &Client,
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
