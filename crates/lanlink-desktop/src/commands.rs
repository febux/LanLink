#[cfg(windows)]
use std::net::SocketAddr;
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

/// Path to the `lanlinkd` binary used for service control commands.
#[cfg(windows)]
fn lanlinkd_exe() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("lanlinkd.exe")))
        .unwrap_or_else(|| PathBuf::from("lanlinkd.exe"))
}

/// Hide the subprocess console window (SPAWN flags for `CreateProcess`).
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Run `lanlinkd service <sub>` and return its captured output. On Windows
/// the service-control commands need an elevated token: when the desktop app
/// runs non-elevated, the command is re-launched through PowerShell
/// `Start-Process -Verb RunAs`, which shows the UAC prompt and captures the
/// child output through a temp log file.
#[cfg(windows)]
async fn run_lanlinkd_service(args: &[&str]) -> Result<String, String> {
    if is_elevated().await {
        return run_lanlinkd_service_direct(args).await;
    }
    run_lanlinkd_service_elevated(args).await
}

/// True when this process holds an elevated (administrator) token. `net
/// session` succeeds only for elevated processes; a dependency-free probe.
#[cfg(windows)]
async fn is_elevated() -> bool {
    match tokio::process::Command::new("net")
        .arg("session")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
    {
        Ok(out) => out.status.success(),
        Err(_) => false,
    }
}

/// Run `lanlinkd service <args>` as-is (correct when already elevated).
#[cfg(windows)]
async fn run_lanlinkd_service_direct(args: &[&str]) -> Result<String, String> {
    let exe = lanlinkd_exe();
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.arg("service").args(args).creation_flags(CREATE_NO_WINDOW);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to run {} service {:?}: {e}", exe.display(), args))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!("lanlinkd service {:?} failed: {detail}", args));
    }
    Ok(if stdout.is_empty() { stderr } else { stdout })
}

/// Run `lanlinkd service <args>` elevated via the UAC prompt (`runas` verb).
///
/// A small batch script runs the command with stdout/stderr redirected to a
/// temp log, `Start-Process -Verb RunAs -Wait` elevates it, and the log is
/// read back. For `service up` the network password is handed over through
/// `LANLINK_PASSWORD` inside the elevated script instead of the command line,
/// keeping the secret out of process listings. `%` is escaped (`%%`) as batch
/// files require; passwords containing double quotes are rejected because
/// `cmd.exe` has no way to escape them inside a quoted string.
#[cfg(windows)]
async fn run_lanlinkd_service_elevated(args: &[&str]) -> Result<String, String> {
    let exe = lanlinkd_exe();
    if !exe.is_file() {
        return Err(format!(
            "lanlinkd.exe not found next to the desktop app (looked at {})",
            exe.display()
        ));
    }

    // Move an optional `--password <pw>` out of the command line into the
    // LANLINK_PASSWORD env var (honored by `lanlinkd service up`).
    let mut password: Option<String> = None;
    let mut clean_args: Vec<&str> = Vec::with_capacity(args.len());
    let mut skip_next = false;
    for (i, arg) in args.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }
        if *arg == "--password" {
            password = args.get(i + 1).map(|s| s.to_string());
            skip_next = true;
            continue;
        }
        clean_args.push(arg);
    }
    if let Some(pw) = &password {
        if pw.contains('"') {
            return Err(
                "password must not contain double-quote characters".to_string(),
            );
        }
    }

    let tag = format!("{}-{}", std::process::id(), args.first().unwrap_or(&"cmd"));
    let bat_path = std::env::temp_dir().join(format!("lanlink-svc-{tag}.cmd"));
    let log_path = std::env::temp_dir().join(format!("lanlink-svc-{tag}.log"));
    let _ = std::fs::remove_file(&log_path);

    let mut batch = String::from("@echo off\r\n");
    // Prepare the target directory inside the elevated process so restrictive
    // parent ACLs cannot block the service config write. We grant Everyone
    // full control on C:\Program Files\LanLink (it contains only the daemon
    // config + secret). The batch also logs the user context for diagnosis.
    let config_dir = std::path::Path::new(DAEMON_CONFIG)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| r"C:\Program Files\LanLink".to_string());
    let config_dir_escaped = config_dir.replace('\\', "\\\\");
    let log_escaped = log_path.to_string_lossy().replace('\\', "\\\\").replace('%', "%%");
    batch.push_str(&format!(
        "echo [elevated] running as: %USERDOMAIN%\\%USERNAME%\r\n"
    ));
    batch.push_str(&format!("mkdir \"{config_dir_escaped}\" 2>nul\r\n"));
    batch.push_str(&format!(
        "icacls \"{config_dir_escaped}\" /grant:r *S-1-1-0:(OI)(CI)F /T /Q\r\n"
    ));
    if let Some(pw) = &password {
        batch.push_str(&format!(
            "set \"LANLINK_PASSWORD={}\"\r\n",
            pw.replace('%', "%%")
        ));
    }
    batch.push_str(&format!("\"{}\" service", exe.display()));
    for arg in &clean_args {
        batch.push_str(&format!(" \"{arg}\""));
    }
    batch.push_str(&format!(">\"{}\" 2>&1\r\n", log_escaped));
    batch.push_str("exit /b %ERRORLEVEL%\r\n");
    std::fs::write(&bat_path, batch)
        .map_err(|e| format!("failed to write elevation script: {e}"))?;

    // UAC prompt happens here; -Wait -PassThru gives us the exit code. A
    // declined prompt makes Start-Process throw, which PowerShell surfaces
    // as a non-zero exit with the error on stderr.
    let ps_script = format!(
        "$p = Start-Process -FilePath '{}' -Verb RunAs -Wait -PassThru; exit $p.ExitCode",
        bat_path.to_string_lossy().replace('\'', "''")
    );
    let output = tokio::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
        .map_err(|e| format!("failed to launch elevated helper: {e}"))?;
    let _ = std::fs::remove_file(&bat_path);

    let text = std::fs::read_to_string(&log_path).unwrap_or_default();
    let _ = std::fs::remove_file(&log_path);
    let text = text.trim().to_string();

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let code = output.status.code().unwrap_or(-1);
        let detail = if text.is_empty() {
            if stderr.is_empty() {
                "cancelled or refused by UAC".to_string()
            } else {
                stderr
            }
        } else {
            text
        };
        return Err(format!(
            "elevated 'lanlinkd service {:?}' failed (exit {code}): {}",
            args, detail
        ));
    }

    Ok(if text.is_empty() { "done".to_string() } else { text })
}

/// Tauri-managed state shared across commands.
pub struct DesktopState {
    pub coordinator: String,
    pub status_socket: Arc<Mutex<Option<PathBuf>>>,
    pub http: Client,
    /// The built-in coordinator process, if this app launched it.
    pub coordinator_child: Arc<Mutex<Option<tokio::process::Child>>>,
}

/// Data directory for app-managed state (built-in coordinator database).
fn app_data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files\LanLink"))
            .join("LanLink")
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(".local/share/lanlink"))
            .unwrap_or_else(|| PathBuf::from("/tmp/lanlink"))
    }
}

/// Path to a sibling binary shipped next to the desktop app.
fn sibling_exe(name: &str) -> PathBuf {
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(&exe)))
        .unwrap_or_else(|| PathBuf::from(exe))
}

/// Loopback `host:port` the built-in coordinator should listen on, derived
/// from the configured coordinator URL when it points at this machine.
fn builtin_coordinator_listen(url: &str) -> String {
    let rest = url
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(url);
    let authority = rest.split('/').next().unwrap_or(rest);
    match authority.rsplit_once(':') {
        Some((host, port)) if host == "localhost" || host == "127.0.0.1" => {
            format!("127.0.0.1:{port}")
        }
        _ => "127.0.0.1:8080".to_string(),
    }
}

/// Probe the coordinator HTTP API (2s timeout).
async fn coordinator_reachable(base: &str, http: &Client) -> bool {
    match http
        .get(format!("{base}/directory"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success() || resp.status() == reqwest::StatusCode::UNAUTHORIZED,
        Err(_) => false,
    }
}

/// Report whether the built-in coordinator is reachable.
#[tauri::command]
pub async fn coordinator_status(state: State<'_, DesktopState>) -> Result<bool, String> {
    Ok(coordinator_reachable(&state.coordinator, &state.http).await)
}

/// Start the bundled coordinator as a detached background process.
#[tauri::command]
pub async fn coordinator_start(state: State<'_, DesktopState>) -> Result<String, String> {
    if coordinator_reachable(&state.coordinator, &state.http).await {
        return Ok("coordinator is already running".to_string());
    }
    let exe = sibling_exe("lanlink-coordinator");
    if !exe.is_file() {
        return Err(format!(
            "lanlink-coordinator not found next to the desktop app (looked at {})",
            exe.display()
        ));
    }
    let db_dir = app_data_dir();
    std::fs::create_dir_all(&db_dir)
        .map_err(|e| format!("failed to create {}: {e}", db_dir.display()))?;
    let db = db_dir.join("coordinator.db");
    let listen = builtin_coordinator_listen(&state.coordinator);

    let mut cmd = tokio::process::Command::new(&exe);
    cmd.arg("--listen")
        .arg(&listen)
        .arg("--db")
        .arg(&db)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        // Detach so it outlives the app; no console window.
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("failed to start {}: {e}", exe.display()))?;
    *state.coordinator_child.lock().await = Some(child);

    // Wait briefly for the HTTP API to come up.
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(300)).await;
        if coordinator_reachable(&state.coordinator, &state.http).await {
            return Ok(format!("coordinator started on {listen} (db: {})", db.display()));
        }
    }
    Ok(format!(
        "coordinator process launched on {listen}; the HTTP API is not answering yet — if it stays unreachable, run it from a terminal to see the error"
    ))
}

/// Stop the built-in coordinator if this app launched it.
#[tauri::command]
pub async fn coordinator_stop(state: State<'_, DesktopState>) -> Result<String, String> {
    let child = state.coordinator_child.lock().await.take();
    if let Some(mut child) = child {
        let _ = child.kill().await;
        return Ok("coordinator stopped".to_string());
    }
    if coordinator_reachable(&state.coordinator, &state.http).await {
        return Err(
            "coordinator was not started by this app; stop it where it is running".to_string(),
        );
    }
    Ok("coordinator is not running".to_string())
}

/// Whether the daemon's live config file exists (Windows).
#[tauri::command]
pub async fn daemon_config_exists() -> Result<bool, String> {
    #[cfg(windows)]
    {
        Ok(std::path::Path::new(DAEMON_CONFIG).is_file())
    }
    #[cfg(not(windows))]
    {
        Ok(false)
    }
}

/// Read the last few error lines from the daemon service log (Windows only).
/// Returns None when there is no log file or no error line.
#[cfg(windows)]
#[tauri::command]
pub async fn daemon_log_tail() -> Result<Option<String>, String> {
    const LOG_PATH: &str = r"C:\Program Files\LanLink\lanlinkd.log";
    let text = match std::fs::read_to_string(LOG_PATH) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("failed to read {LOG_PATH}: {e}")),
    };
    let tail = text
        .lines()
        .filter(|line| line.contains("ERROR") || line.contains("error="))
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    Ok(if tail.is_empty() { None } else { Some(tail) })
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
            coordinator_child: Arc::new(Mutex::new(None)),
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

/// Path of the live daemon config written by the MSI / `service up`
/// (matches `CONFIG_PATH` in `crates/lanlinkd/src/service.rs`).
#[cfg(windows)]
const DAEMON_CONFIG: &str = "C:\\Program Files\\LanLink\\lanlinkd.toml";

/// Read the configured `[status] socket` from the daemon config, if the
/// config exists and parses. Absent/invalid config = None (fall back to the
/// default port scan).
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

/// Where the discovery below looked, for actionable error messages.
#[cfg(windows)]
fn status_probe_hint() -> String {
    match configured_status_socket() {
        Some(s) => format!(
            "tried {s} (the [status] socket in {DAEMON_CONFIG}) and the default ports 127.0.0.1:9001-9032"
        ),
        None => format!(
            "tried the default ports 127.0.0.1:9001-9032; to use another port set [status] socket in {DAEMON_CONFIG}"
        ),
    }
}

/// Error shown when no status endpoint answers.
#[cfg(windows)]
fn no_socket_message() -> String {
    format!(
        "no daemon status socket found ({}) — is the LanLinkDaemon service running?",
        status_probe_hint()
    )
}

#[cfg(unix)]
fn no_socket_message() -> String {
    "no daemon status socket found".to_string()
}

/// Read the daemon status snapshot via the local Unix socket (Linux/macOS) or
/// TCP loopback (Windows).
#[tauri::command]
pub async fn get_snapshot(state: State<'_, DesktopState>) -> Result<StatusSnapshot, String> {
    let path = match state.resolve_socket().await {
        Ok(Some(path)) => path,
        Ok(None) => return Err(no_socket_message()),
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

/// Install the LanLinkDaemon Windows service.
#[cfg(windows)]
#[tauri::command]
pub async fn service_install() -> Result<String, String> {
    run_lanlinkd_service(&["install"]).await
}

/// Uninstall the LanLinkDaemon Windows service.
#[cfg(windows)]
#[tauri::command]
pub async fn service_uninstall() -> Result<String, String> {
    run_lanlinkd_service(&["uninstall"]).await
}

/// Start the LanLinkDaemon Windows service.
#[cfg(windows)]
#[tauri::command]
pub async fn service_start() -> Result<String, String> {
    run_lanlinkd_service(&["start", ""]).await
}

/// Stop the LanLinkDaemon Windows service.
#[cfg(windows)]
#[tauri::command]
pub async fn service_stop() -> Result<String, String> {
    run_lanlinkd_service(&["stop", ""]).await
}

/// Query the LanLinkDaemon Windows service status. This is a read-only SCM
/// query (`SERVICE_QUERY_STATUS`), allowed for standard users, so it never
/// triggers a UAC prompt — the desktop UI calls it automatically to diagnose
/// the first-run state.
#[cfg(windows)]
#[tauri::command]
pub async fn service_status() -> Result<String, String> {
    run_lanlinkd_service_direct(&["status"]).await
}

/// Configure a network and start it via the Windows service. This writes
/// `C:\Program Files\LanLink\lanlinkd.toml` and the password secret, then calls
/// `lanlinkd service up`.
#[cfg(windows)]
#[tauri::command]
pub async fn service_up(
    name: String,
    password: String,
    coordinator: Option<String>,
    public: Option<bool>,
) -> Result<String, String> {
    // `lanlinkd service up` uses a positional `name` argument (clap expects the
    // network name without `--name`). Flags still need their values split.
    let mut args = vec!["up", &name];
    let coordinator_str;
    let password_str;
    if let Some(c) = coordinator {
        coordinator_str = c;
        args.push("--coordinator");
        args.push(&coordinator_str);
    }
    password_str = password;
    args.push("--password");
    args.push(&password_str);
    if public == Some(true) {
        args.push("--public");
    }
    run_lanlinkd_service(&args).await
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
    #[allow(non_snake_case)] isPublic: bool,
) -> Result<CreateNetworkResponse, String> {
    let is_public = isPublic;
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
    #[allow(non_snake_case)] networkId: u64,
    password: String,
    #[allow(non_snake_case)] sharedSubnet: Option<String>,
) -> Result<JoinNetworkResponse, String> {
    let network_id = networkId;
    let shared_subnet = sharedSubnet;
    let mut node_id = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut node_id);
    let req = JoinNetworkRequest {
        password,
        node_id: NodeId(node_id),
        public_key: vec![],
        direct_candidates: vec!["127.0.0.1:9000".parse().expect("static addr parses")],
        relay_candidates: vec![],
        tcp_candidates: vec![],
        tcp_relay_candidates: vec![],
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
    #[cfg(windows)]
    {
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
}

#[cfg(unix)]
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

#[cfg(windows)]
async fn discover_socket() -> Result<Option<PathBuf>, String> {
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
