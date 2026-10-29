mod config;
mod control;
mod sdnotify;
mod service;
mod status;
mod tcp;
mod up;

use clap::{Parser, Subcommand};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// Global shutdown signal. Triggered by SIGINT/SIGTERM (unix), Ctrl-C
/// (Windows console) or the Windows service control handler.
pub struct ShutdownSignal {
    tx: tokio::sync::watch::Sender<bool>,
}

impl ShutdownSignal {
    fn new() -> Self {
        let (tx, _rx) = tokio::sync::watch::channel(false);
        Self { tx }
    }

    /// Request shutdown (safe to call from synchronous code).
    pub fn trigger(&self) {
        let _ = self.tx.send(true);
    }

    /// Resolve once shutdown has been requested.
    pub async fn wait(&self) {
        let mut rx = self.tx.subscribe();
        if *rx.borrow() {
            return;
        }
        let _ = rx.changed().await;
    }
}

static SHUTDOWN: std::sync::OnceLock<ShutdownSignal> = std::sync::OnceLock::new();

pub fn shutdown() -> &'static ShutdownSignal {
    SHUTDOWN.get_or_init(ShutdownSignal::new)
}

/// Simple anonymized traffic counters used for opt-in telemetry.
#[derive(Debug, Default)]
pub struct Metrics {
    tx_bytes: AtomicU64,
    rx_bytes: AtomicU64,
}

impl Metrics {
    pub fn add_tx(&self, n: usize) {
        self.tx_bytes.fetch_add(n as u64, Ordering::Relaxed);
    }
    pub fn add_rx(&self, n: usize) {
        self.rx_bytes.fetch_add(n as u64, Ordering::Relaxed);
    }
    pub fn tx_bytes(&self) -> u64 {
        self.tx_bytes.load(Ordering::Relaxed)
    }
    pub fn rx_bytes(&self) -> u64 {
        self.rx_bytes.load(Ordering::Relaxed)
    }
}

/// Spawn a task that triggers the global shutdown signal on Ctrl-C and, on
/// unix, SIGTERM/SIGHUP.
fn spawn_os_signal_handlers() {
    #[cfg(unix)]
    {
        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm =
                signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
            let mut sighup =
                signal(SignalKind::hangup()).expect("failed to install SIGHUP handler");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = sigterm.recv() => {}
                _ = sighup.recv() => {}
            }
            tracing::info!("shutdown signal received");
            shutdown().trigger();
        });
    }
    #[cfg(not(unix))]
    {
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                tracing::info!("shutdown signal received");
                shutdown().trigger();
            }
        });
    }
}

#[derive(Parser, Debug)]
#[command(name = "lanlinkd", about = "LanLink virtual-LAN daemon", version)]
struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run a two-node static peer (M1/M2 milestone). No coordinator required.
    StaticPeer {
        /// TAP/virtual NIC name.
        #[arg(long, default_value = "lanlink0")]
        tap: String,

        /// Local IPv4 address inside the virtual LAN.
        #[arg(long, default_value = "26.0.0.1")]
        local_ip: std::net::Ipv4Addr,

        /// Prefix length for the virtual LAN.
        #[arg(long, default_value = "8")]
        prefix: u8,

        /// UDP socket to listen on.
        #[arg(long, default_value = "0.0.0.0:9000")]
        bind: SocketAddr,

        /// Remote peer UDP address.
        #[arg(long)]
        peer_addr: SocketAddr,

        /// Local node id (u64, converted to big-endian bytes).
        #[arg(long, default_value = "1")]
        node_id: u64,

        /// Remote peer node id (u64, converted to big-endian bytes).
        #[arg(long, default_value = "2")]
        peer_node_id: u64,

        /// Network password (used to derive the symmetric key).
        #[arg(long)]
        password: String,

        /// Network id used as salt for key derivation.
        #[arg(long, default_value = "1")]
        network_id: u64,

        /// Virtual NIC MTU.
        #[arg(long, default_value = "1400")]
        mtu: usize,
    },

    /// Join a network via the coordinator (M3/M4 milestone) and forward traffic.
    Run {
        /// TOML config file (all other flags are optional overrides).
        #[arg(long)]
        config: Option<String>,

        /// Coordinator base URL, e.g. http://localhost:8080
        #[arg(long)]
        coordinator: Option<String>,

        /// Network id to join.
        #[arg(long)]
        network_id: Option<u64>,

        /// Network password (prefer a config file + password_file).
        #[arg(long)]
        password: Option<String>,

        /// UDP socket to listen on (also advertised as direct candidate).
        #[arg(long)]
        bind: Option<SocketAddr>,

        /// TAP/virtual NIC name.
        #[arg(long)]
        tap: Option<String>,

        /// Virtual NIC MTU.
        #[arg(long)]
        mtu: Option<usize>,

        /// Comma-separated list of STUN servers (host:port). Defaults to public servers.
        #[arg(long, value_delimiter = ',')]
        stun_servers: Option<Vec<String>>,

        /// Enable gateway mode: share the physical LAN subnet with the virtual network.
        #[arg(long)]
        gateway: bool,

        /// Physical subnet to share when gateway mode is set (e.g. 192.168.1.0/24).
        #[arg(long)]
        shared_subnet: Option<String>,

        /// Physical interface to MASQUERADE to when gateway mode is set.
        #[arg(long)]
        phys_interface: Option<String>,

        /// Status socket path for frontends (default /run/lanlink/lanlinkd-<id>.sock,
        /// with a /tmp fallback when /run is not writable).
        #[arg(long)]
        status_socket: Option<String>,
    },

    /// One-command startup: resolve (or create) a network by name and run
    /// the daemon. No config file or numeric network id required.
    Up {
        /// Coordinator base URL, e.g. http://localhost:8080
        #[arg(long, default_value = "http://localhost:8080")]
        coordinator: String,

        /// Network name. If the network does not exist it is created; if it
        /// exists the password must match.
        #[arg(short, long)]
        name: String,

        /// Network password. Prefer the LANLINK_PASSWORD env var to avoid
        /// exposing secrets in `ps` output.
        #[arg(short, long)]
        password: Option<String>,

        /// Create the network as public/gaming if it does not exist.
        #[arg(long)]
        public: bool,

        /// UDP socket to listen on (also advertised as direct candidate).
        #[arg(long)]
        bind: Option<SocketAddr>,

        /// TAP/virtual NIC name.
        #[arg(long)]
        tap: Option<String>,

        /// Virtual NIC MTU.
        #[arg(long)]
        mtu: Option<usize>,

        /// Status socket path for frontends (default
        /// /run/lanlink/lanlinkd-<network_id>.sock with /tmp fallback).
        #[arg(long)]
        status_socket: Option<String>,
    },

    /// Install / control the system service (systemd, Windows SCM).
    #[command(subcommand)]
    Service(service::ServiceCmd),
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    // The Windows service entry point installs its own file-based subscriber;
    // the `service run` command is invoked without this `main`. For every
    // other invocation (CLI, cargo run, ...) keep the default fmt subscriber.
    #[cfg(not(target_os = "windows"))]
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    #[cfg(target_os = "windows")]
    {
        if !matches!(args.cmd, Cmd::Service(service::ServiceCmd::Run)) {
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                .init();
        }
    }

    let result = match args.cmd {
        Cmd::StaticPeer {
            tap,
            local_ip,
            prefix,
            bind,
            peer_addr,
            node_id,
            peer_node_id,
            password,
            network_id,
            mtu,
        } => {
            spawn_os_signal_handlers();
            run_static_peer(
                tap,
                local_ip,
                prefix,
                bind,
                peer_addr,
                node_id,
                peer_node_id,
                password,
                network_id,
                mtu,
            )
            .await
        }
        Cmd::Run {
            config,
            coordinator,
            network_id,
            password,
            bind,
            tap,
            mtu,
            stun_servers,
            gateway,
            shared_subnet,
            phys_interface,
            status_socket,
        } => {
            spawn_os_signal_handlers();
            let cli = config::CliOverrides {
                coordinator,
                network_id,
                password,
                bind,
                tap,
                mtu,
                stun_servers,
                gateway: gateway.then_some(true),
                shared_subnet,
                phys_interface,
                status_socket,
            };
            let cfg = match config {
                Some(path) => config::load(&path, &cli),
                None => config::resolve(&config::FileConfig::default(), &cli),
            };
            match cfg {
                Ok(cfg) => {
                    let control_socket = cfg.status_socket.clone();
                    run_daemon_runtime(cfg, control_socket).await
                }
                Err(e) => Err(anyhow::anyhow!("configuration error: {e}")),
            }
        }
        Cmd::Up {
            coordinator,
            name,
            password,
            public,
            bind,
            tap,
            mtu,
            status_socket,
        } => {
            spawn_os_signal_handlers();
            match password.or_else(|| std::env::var("LANLINK_PASSWORD").ok()) {
                Some(password) => {
                    match up::resolve_config(
                        coordinator,
                        name,
                        password,
                        public,
                        bind,
                        tap,
                        mtu,
                        status_socket.clone(),
                    )
                    .await
                    {
                        Ok(cfg) => run_daemon_runtime(cfg, status_socket).await,
                        Err(e) => Err(e),
                    }
                }
                None => Err(anyhow::anyhow!(
                    "--password or LANLINK_PASSWORD env var is required"
                )),
            }
        }
        Cmd::Service(sub) => service::dispatch(sub).await,
    };

    if let Err(e) = result {
        tracing::error!(error = %e, "lanlinkd failed");
        std::process::exit(1);
    }
}

/// Load a config file and run the persistent daemon runtime (used by `service run`).
pub async fn run_from_config_path(path: &str) -> anyhow::Result<()> {
    spawn_os_signal_handlers();
    let cfg = config::load(path, &config::CliOverrides::default())
        .map_err(|e| anyhow::anyhow!("configuration error: {e}"))?;
    let control_socket = cfg.status_socket.clone();
    run_daemon_runtime(cfg, control_socket).await
}

/// Persistent daemon runtime: one control/status socket, optional network session,
/// supports disconnect/reconnect/stop without a process restart.
async fn run_daemon_runtime(
    initial_cfg: config::RunConfig,
    control_socket: Option<String>,
) -> anyhow::Result<()> {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<control::ControlEnvelope>(8);
    let empty_state = control::empty_state();
    let (state_tx, state_rx) = tokio::sync::watch::channel(empty_state);

    control::spawn_server(control_socket, state_rx, cmd_tx).await;

    let mut session_cancel: Option<CancellationToken> = None;
    let mut session_handle: Option<tokio::task::JoinHandle<()>> = None;

    // Auto-connect using the CLI/config-provided network on startup.
    start_session(initial_cfg, &mut session_cancel, &mut session_handle, &state_tx);

    loop {
        tokio::select! {
            _ = shutdown().wait() => {
                tracing::info!("shutdown requested; stopping active session");
                stop_session(session_cancel, session_handle).await;
                break;
            }
            envelope = cmd_rx.recv() => {
                match envelope {
                    Some(env) => {
                        handle_control_envelope(
                            env,
                            &mut session_cancel,
                            &mut session_handle,
                            &state_tx,
                        )
                        .await;
                    }
                    None => break,
                }
            }
        }
    }

    tracing::info!("daemon runtime exiting");
    Ok(())
}

fn start_session(
    cfg: config::RunConfig,
    session_cancel: &mut Option<CancellationToken>,
    session_handle: &mut Option<tokio::task::JoinHandle<()>>,
    state_tx: &control::StateTx,
) {
    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    let state_tx = state_tx.clone();
    let task = tokio::spawn(async move {
        if let Err(e) = run_coordinated(cfg, state_tx, task_cancel).await {
            tracing::error!(error = %e, "network session ended");
        }
    });
    *session_cancel = Some(cancel);
    *session_handle = Some(task);
}

async fn stop_session(
    session_cancel: Option<CancellationToken>,
    session_handle: Option<tokio::task::JoinHandle<()>>,
) {
    if let Some(c) = session_cancel {
        c.cancel();
    }
    if let Some(h) = session_handle {
        let _ = h.await;
    }
}

async fn handle_control_envelope(
    envelope: control::ControlEnvelope,
    session_cancel: &mut Option<CancellationToken>,
    session_handle: &mut Option<tokio::task::JoinHandle<()>>,
    state_tx: &control::StateTx,
) {
    use lanlink_proto::control::ControlResponse;

    match envelope.req {
        lanlink_proto::control::ControlRequest::Status => {
            let state = state_tx.borrow().clone();
            let snap = state.snapshot().await;
            let _ = envelope
                .resp
                .send(ControlResponse::Ok { result: lanlink_proto::control::ControlResult::Status(snap) });
        }
        lanlink_proto::control::ControlRequest::Connect {
            coordinator,
            network_id,
            password,
            bind,
            tap,
            mtu,
            stun_servers,
            gateway,
            shared_subnet,
            phys_interface,
        } => {
            if session_handle.is_some() {
                let _ = envelope.resp.send(ControlResponse::Error {
                    error: "already connected".into(),
                });
                return;
            }
            let cfg = match build_run_config(
                coordinator,
                network_id,
                password,
                bind,
                tap,
                mtu,
                stun_servers,
                gateway,
                shared_subnet,
                phys_interface,
            ) {
                Ok(c) => c,
                Err(e) => {
                    let _ = envelope.resp.send(ControlResponse::Error {
                        error: format!("invalid connect config: {e}"),
                    });
                    return;
                }
            };
            start_session(cfg, session_cancel, session_handle, state_tx);
            let _ = envelope.resp.send(ControlResponse::Ok {
                result: lanlink_proto::control::ControlResult::Ack,
            });
        }
        lanlink_proto::control::ControlRequest::Disconnect => {
            if session_handle.is_none() {
                let _ = envelope.resp.send(ControlResponse::Ok {
                    result: lanlink_proto::control::ControlResult::Ack,
                });
                return;
            }
            let cancel = session_cancel.take().unwrap();
            let handle = session_handle.take().unwrap();
            cancel.cancel();
            let _ = handle.await;
            let _ = envelope.resp.send(ControlResponse::Ok {
                result: lanlink_proto::control::ControlResult::Ack,
            });
        }
        lanlink_proto::control::ControlRequest::Stop => {
            let _ = envelope.resp.send(ControlResponse::Ok {
                result: lanlink_proto::control::ControlResult::Ack,
            });
            shutdown().trigger();
            let cancel = session_cancel.take();
            let handle = session_handle.take();
            if let Some(c) = cancel {
                c.cancel();
            }
            if let Some(h) = handle {
                let _ = h.await;
            }
        }
    }
}

fn build_run_config(
    coordinator: String,
    network_id: u64,
    password: String,
    bind: std::net::SocketAddr,
    tap: String,
    mtu: usize,
    stun_servers: Vec<String>,
    gateway: bool,
    shared_subnet: String,
    phys_interface: String,
) -> anyhow::Result<config::RunConfig> {
    let file = config::FileConfig::default();
    let cli = config::CliOverrides {
        coordinator: Some(coordinator),
        network_id: Some(network_id),
        password: Some(password),
        bind: Some(bind),
        tap: Some(tap),
        mtu: Some(mtu),
        stun_servers: Some(stun_servers),
        gateway: Some(gateway),
        shared_subnet: Some(shared_subnet).filter(|s| !s.is_empty()),
        phys_interface: Some(phys_interface).filter(|s| !s.is_empty()),
        status_socket: None,
    };
    config::resolve(&file, &cli)
        .map_err(|e| anyhow::anyhow!("configuration error: {e}"))
}

#[allow(clippy::too_many_arguments)]
async fn run_static_peer(
    tap_name: String,
    local_ip: std::net::Ipv4Addr,
    prefix: u8,
    bind: SocketAddr,
    peer_addr: SocketAddr,
    node_id: u64,
    peer_node_id: u64,
    password: String,
    network_id: u64,
    mtu: usize,
) -> anyhow::Result<()> {
    let netmask = prefix_to_netmask(prefix)?;

    let config = lanlink_tap::NicConfig {
        name: tap_name.clone(),
        mtu,
        ipv4: local_ip,
        netmask,
    };

    let nic = lanlink_tap::create_nic(config)?;
    let nic = Arc::new(Mutex::new(nic));

    let psk = lanlink_crypto::derive_network_psk(&password, network_id);
    let local = node_id_from_u64(node_id);
    let peer = node_id_from_u64(peer_node_id);
    let mut switch = lanlink_switch::Switch::new(local, lanlink_crypto::StaticSession::new(psk));
    switch.add_peer(peer, peer_addr);
    let switch = Arc::new(Mutex::new(switch));

    let socket = Arc::new(bind_udp_socket(bind).await?);
    tracing::info!(
        tap = %tap_name,
        %local_ip,
        %prefix,
        local_node = %hex(&local.0),
        peer_node = %hex(&peer.0),
        %bind,
        %peer_addr,
        "static peer running; press Ctrl-C to stop"
    );

    start_packet_pump(nic, switch, socket, mtu).await
}

#[allow(clippy::too_many_arguments)]
async fn run_coordinated(
    cfg: config::RunConfig,
    state_tx: control::StateTx,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let config::RunConfig {
        coordinator,
        network_id,
        password,
        bind,
        tap: tap_name,
        mtu,
        stun_servers,
        gateway,
        shared_subnet,
        phys_interface,
        update,
        telemetry,
        status_socket: _,
    } = cfg;

    let client = reqwest::Client::new();

    // Load or create the persistent Ed25519 identity for this installation.
    // The node id is derived from the public key, so it is stable across runs.
    let identity = load_or_create_identity()?;
    let local = identity.node_id();
    tracing::info!(node_id = %hex(&local.0), "loaded identity");

    let shared_subnet = shared_subnet.unwrap_or_default();

    // Track every background task spawned by this session. After the session
    // is cancelled we wait for all of them to finish so that sockets and TAP
    // devices are released before a reconnect re-binds the same ports.
    let tracker = TaskTracker::new();

    // Bind the TCP listener early so we can advertise its address in the join
    // request. The UDP and TCP sockets can share the same port on Linux.
    let (tcp_in_tx, tcp_in_rx) = tokio::sync::mpsc::unbounded_channel::<(SocketAddr, Vec<u8>)>();
    let (tcp_transport, tcp_addr) = tcp::TcpTransport::bind(bind, tcp_in_tx, &tracker, cancel.clone()).await?;
    let tcp_candidates = lanlink_punch::gather_host_candidates(tcp_addr.port());

    let initial_candidates = lanlink_punch::gather_host_candidates(bind.port());
    let join_req = lanlink_proto::JoinNetworkRequest {
        password: password.clone(),
        node_id: local,
        public_key: identity.public_key().to_vec(),
        direct_candidates: initial_candidates.iter().map(|c| c.addr).collect(),
        relay_candidates: Vec::new(), // updated after we bind to the network relay
        tcp_candidates: tcp_candidates.iter().map(|c| c.addr).collect(),
        tcp_relay_candidates: Vec::new(),
        shared_subnet: shared_subnet.clone(),
    };

    let url = format!("{}/networks/{}/join", coordinator, network_id);
    let resp: lanlink_proto::JoinNetworkResponse = client
        .post(&url)
        .json(&join_req)
        .send()
        .await?
        .error_for_status()
        .map_err(|e| anyhow::anyhow!("coordinator join failed: {e}"))?
        .json()
        .await?;

    tracing::info!(
        network_id = resp.network_id.0,
        assigned_ip = %resp.assigned_ip.0,
        peer_count = resp.peers.len(),
        relay_servers = ?resp.relay_servers,
        "joined network via coordinator"
    );

    let prefix = 24; // coordinator assigns /24 subnets in M3
    let netmask = prefix_to_netmask(prefix)?;
    let nic_config = lanlink_tap::NicConfig {
        name: tap_name.clone(),
        mtu,
        ipv4: resp.assigned_ip.0,
        netmask,
    };

    let nic = lanlink_tap::create_nic(nic_config)?;
    let nic = Arc::new(Mutex::new(nic));

    // Apply per-network inbound firewall when the coordinator marks the
    // network public/gaming. `apply` is idempotent (replaces any previous
    // table), so this also recovers cleanly after a crash.
    let mut firewall_profile = None;
    if resp.is_public {
        let fw = lanlink_firewall::FirewallProfile::public(tap_name.clone());
        match fw.apply().await {
            Ok(()) => {
                tracing::info!(interface = %tap_name, "applied public-network firewall");
                firewall_profile = Some(fw);
            }
            Err(e) => tracing::warn!(error = %e, "failed to apply public-network firewall"),
        }
    }

    // Gateway mode: enable forwarding + NAT and intercept ARP requests for the
    // shared subnet.
    let gateway_manager = if gateway {
        let phys_interface = phys_interface.unwrap_or_default();
        let gw = Arc::new(lanlink_gateway::GatewayManager::new(
            lanlink_gateway::GatewayConfig {
                enabled: true,
                shared_subnet: shared_subnet.clone(),
                virtual_ip: resp.assigned_ip.0,
                virtual_subnet: lanlink_gateway::subnet_cidr(resp.assigned_ip.0, netmask),
                interface: tap_name.clone(),
                phys_interface,
            },
        ));
        match gw.enable().await {
            Ok(()) => tracing::info!(shared_subnet = %shared_subnet, "gateway mode enabled"),
            Err(e) => tracing::warn!(error = %e, "failed to enable gateway mode"),
        }
        Some(gw)
    } else {
        None
    };

    let psk = lanlink_crypto::derive_network_psk(&password, network_id);
    let session = lanlink_crypto::StaticSession::new(psk);
    let mut switch = lanlink_switch::Switch::new(local, session.clone());
    switch.set_identity(identity.clone());
    switch.sync_peers(&resp.peers);
    let switch = Arc::new(Mutex::new(switch));

    // Install routes for any gateway peers advertised by the coordinator.
    // `flush_stale` first removes routes a crashed previous run may have left
    // on this interface.
    let route_manager = Arc::new(lanlink_gateway::RouteManager::new(tap_name.clone()));
    if let Err(e) = route_manager.flush_stale().await {
        tracing::debug!(error = %e, "route flush at startup failed");
    }
    let initial_routes = lanlink_gateway::gateway_routes(&resp.peers);
    if let Err(e) = route_manager.sync_routes(&initial_routes).await {
        tracing::warn!(error = %e, "failed to sync gateway routes");
    }

    let metrics = Arc::new(Metrics::default());
    let start_time = Instant::now();

    let socket = Arc::new(bind_udp_socket(bind).await?);

    // Bind to a network relay server (M4/M5 fallback). The relay is content-blind:
    // it only learns our public UDP endpoint and node id, then forwards encrypted
    // packets addressed to us. Traffic remains E2E-encrypted.
    let local_relay = bind_to_relay(&socket, local, &resp.relay_servers).await;
    if let Some(addr) = local_relay {
        tracing::info!(%addr, "bound to UDP relay server");
        let socket_kp = socket.clone();
        let relay_cancel = cancel.clone();
        tracker.spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(25));
            loop {
                tokio::select! {
                    _ = relay_cancel.cancelled() => break,
                    _ = interval.tick() => {
                        let bind = lanlink_relay::encode_bind(local);
                        if let Err(e) = socket_kp.send_to(&bind, addr).await {
                            tracing::debug!(%addr, error = %e, "UDP relay keepalive failed");
                        }
                    }
                }
            }
        });
    }

    // Bind to a TCP relay server if one is configured. The relay is
    // content-blind: it only learns our node id, then forwards encrypted
    // packets addressed to us.
    let local_tcp_relay = resp.tcp_relay_servers.first().copied();
    if let Some(addr) = local_tcp_relay {
        tracing::info!(%addr, "bound to TCP relay server");
        tcp_transport.connect(addr);
        let bind = lanlink_relay::encode_bind(local);
        tcp_transport.send(addr, bind);
        let tcp_kp = tcp_transport.clone();
        let tcp_relay_cancel = cancel.clone();
        tracker.spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(25));
            loop {
                tokio::select! {
                    _ = tcp_relay_cancel.cancelled() => break,
                    _ = interval.tick() => {
                        let bind = lanlink_relay::encode_bind(local);
                        tcp_kp.send(addr, bind);
                    }
                }
            }
        });
    }

    let stun_addrs = parse_stun_servers(&stun_servers)?;
    let puncher = Arc::new(lanlink_punch::Puncher::new(
        local,
        socket.clone(),
        session,
        stun_addrs,
    ));

    let punching: Arc<Mutex<HashSet<lanlink_proto::NodeId>>> = Arc::new(Mutex::new(HashSet::new()));

    // Publish the live session state to the persistent control/status socket.
    let registry = status::StatusRegistry::new();
    registry.set_info(status::DaemonInfo {
        network_id,
        assigned_ip: resp.assigned_ip.0,
        tap_name: tap_name.clone(),
        bind,
        gateway_enabled: gateway,
        firewall_enabled: resp.is_public,
        relay_addr: local_relay,
        relay_servers: resp.relay_servers.clone(),
    });
    registry.update_peers(&resp.peers);
    let session_state = status::StatusState {
        registry: registry.clone(),
        switch: switch.clone(),
        metrics: metrics.clone(),
        punching: punching.clone(),
        start: start_time,
    };
    let _ = state_tx.send_replace(session_state);

    // Kick off NAT traversal for the initial peer list.
    punch_peers(
        coordinator.clone(),
        network_id,
        local,
        client.clone(),
        puncher.clone(),
        switch.clone(),
        punching.clone(),
        tcp_transport.clone(),
        resp.peers,
        &tracker,
        cancel.clone(),
    )
    .await;

    // Periodic candidate refresh + re-punch of any new peers.
    let refresh_coordinator = coordinator.clone();
    let refresh_client = client.clone();
    let refresh_puncher = puncher.clone();
    let refresh_switch = switch.clone();
    let refresh_punching = punching.clone();
    let refresh_route_manager = route_manager.clone();
    let refresh_registry = registry.clone();
    let local_relay_for_refresh = local_relay;
    let local_tcp_relay_for_refresh = local_tcp_relay;
    let refresh_tcp = tcp_transport.clone();
    let refresh_tracker = tracker.clone();
    let refresh_cancel = cancel.clone();
    tracker.spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = refresh_cancel.cancelled() => break,
                _ = interval.tick() => {
                    if let Ok(peers) = refresh_candidates_and_punch(
                        &refresh_coordinator,
                        network_id,
                        local,
                        &refresh_client,
                        &refresh_puncher,
                        &refresh_switch,
                        &refresh_punching,
                        local_relay_for_refresh,
                        tcp_addr,
                        local_tcp_relay_for_refresh,
                        refresh_tcp.clone(),
                        &refresh_tracker,
                        refresh_cancel.clone(),
                    )
                    .await
                    {
                        refresh_registry.update_peers(&peers);
                        let routes = lanlink_gateway::gateway_routes(&peers);
                        if let Err(e) = refresh_route_manager.sync_routes(&routes).await {
                            tracing::warn!(error = %e, "failed to sync gateway routes");
                        }
                    } else {
                        tracing::warn!("candidate refresh failed");
                    }
                }
            }
        }
    });

    tracing::info!(%bind, "coordinated peer running");

    // Opt-in telemetry: send an anonymized report to the coordinator at a
    // low frequency. Disabled by default; no node id, network id, or IP is
    // included.
    if telemetry.enabled {
        let endpoint = telemetry.endpoint.clone();
        let interval = telemetry.interval;
        let relay_servers = resp.relay_servers.clone();
        let switch_for_telemetry = switch.clone();
        let metrics_for_telemetry = metrics.clone();
        let firewall_enabled = resp.is_public;
        let telemetry_cancel = cancel.clone();
        tracker.spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("failed to build telemetry client");
            let mut ticker = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = telemetry_cancel.cancelled() => break,
                    _ = ticker.tick() => {
                        let report = build_telemetry_report(
                            &start_time,
                            &metrics_for_telemetry,
                            &switch_for_telemetry,
                            gateway,
                            firewall_enabled,
                            &relay_servers,
                        );
                        match client.post(&endpoint).json(&report).send().await {
                            Ok(r) => {
                                if r.status().is_success() {
                                    tracing::debug!("telemetry report submitted");
                                } else {
                                    tracing::debug!(status = %r.status(), "telemetry submission rejected");
                                }
                            }
                            Err(e) => tracing::debug!(error = %e, "telemetry submission failed"),
                        }
                    }
                }
            }
        });
    }

    // Opt-in update channel: periodically fetch the release manifest and log
    // when a newer version is published. Disabled by default (no phone-home);
    // the daemon never downloads or executes anything by itself.
    if update.enabled {
        let manifest_url = update.manifest.clone();
        let interval = update.interval;
        let update_cancel = cancel.clone();
        tracker.spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("failed to build update-check client");
            let mut notified: Option<String> = None;
            let mut ticker = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = update_cancel.cancelled() => break,
                    _ = ticker.tick() => {
                        match client
                            .get(&manifest_url)
                            .send()
                            .await
                            .and_then(|r| r.error_for_status())
                        {
                            Ok(resp) => match resp.json::<lanlink_proto::update::UpdateManifest>().await {
                                Ok(m) => {
                                    if let lanlink_proto::update::UpdateCheck::Available {
                                        latest, ..
                                    } =
                                        lanlink_proto::update::check_manifest(&m, env!("CARGO_PKG_VERSION"))
                                    {
                                        if notified.as_deref() != Some(latest.to_string().as_str()) {
                                            tracing::info!(
                                                running = env!("CARGO_PKG_VERSION"),
                                                latest = %latest,
                                                "update available on the release channel"
                                            );
                                            notified = Some(latest.to_string());
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::debug!(error = %e, "update manifest parse failed")
                                }
                            },
                            Err(e) => {
                                tracing::debug!(error = %e, "update check failed")
                            }
                        }
                    }
                }
            }
        });
    }

    // Tell the service manager we are up, then run until shutdown or disconnect.
    sdnotify::notify_ready();
    let watchdog = sdnotify::spawn_watchdog();
    let pump_result = start_coordinated_pump(
        nic.clone(),
        switch,
        socket,
        tcp_transport,
        tcp_in_rx,
        puncher,
        punching,
        gateway_manager.clone(),
        metrics,
        mtu,
        cancel.clone(),
    )
    .await;

    // Wait for all background tasks (TCP transport, relay keepalives, refresh,
    // telemetry, update, punching) to finish so their sockets are released
    // before the next reconnect re-binds the same ports.
    tracker.close();
    tracker.wait().await;

    // Best-effort leave notification before cleanup.
    send_leave_notification(&client,
        &coordinator,
        network_id,
        local,
        &password,
    )
    .await;

    // ---- Graceful shutdown/disconnect cleanup ---------------------------
    // Undo everything that persists beyond the process (firewall table,
    // MASQUERADE rule, pushed routes, TAP device) so no state leaks across
    // restarts and a later reconnect is clean.
    sdnotify::notify_stopping();
    if let Some(watchdog) = watchdog {
        watchdog.abort();
    }
    if let Some(fw) = &firewall_profile {
        if let Err(e) = fw.clear().await {
            tracing::warn!(error = %e, "failed to clear firewall on shutdown");
        }
    }
    if let Some(gw) = &gateway_manager {
        if let Err(e) = gw.disable().await {
            tracing::warn!(error = %e, "failed to disable gateway mode on shutdown");
        }
    }
    if let Err(e) = route_manager.sync_routes(&[]).await {
        tracing::warn!(error = %e, "failed to remove routes on shutdown");
    }
    // Drop the TAP device explicitly so the interface/IP are released before
    // the daemon reports itself as disconnected. `destroy_nic` is best-effort.
    drop(nic);
    if let Err(e) = lanlink_tap::destroy_nic(&tap_name) {
        tracing::debug!(error = %e, tap = %tap_name, "TAP destroy failed or unnecessary");
    }

    // Publish the empty/disconnected state to the control/status socket.
    let _ = state_tx.send_replace(control::empty_state());
    tracing::info!("session cleanup complete");

    pump_result
}

async fn send_leave_notification(
    client: &reqwest::Client,
    coordinator: &str,
    network_id: u64,
    node_id: lanlink_proto::NodeId,
    password: &str,
) {
    let req = lanlink_proto::LeaveNetworkRequest {
        password: password.to_string(),
        node_id,
    };
    let url = format!("{}/networks/{}/leave", coordinator, network_id);
    match client.post(&url).json(&req).send().await {
        Ok(resp) if resp.status().is_success() => {
            tracing::info!("notified coordinator of network leave");
        }
        Ok(resp) => {
            tracing::debug!(status = %resp.status(), "coordinator leave notification rejected");
        }
        Err(e) => {
            tracing::debug!(error = %e, "coordinator leave notification failed");
        }
    }
}

/// Attempt to bind the local node to one of the network relay servers.
/// Returns the address of the first relay that accepted a bind packet, or
/// `None` if no relay server was configured or reachable.
async fn bind_to_relay(
    socket: &tokio::net::UdpSocket,
    local: lanlink_proto::NodeId,
    relay_servers: &[SocketAddr],
) -> Option<SocketAddr> {
    for addr in relay_servers {
        let bind = lanlink_relay::encode_bind(local);
        if let Err(e) = socket.send_to(&bind, *addr).await {
            tracing::debug!(%addr, error = %e, "failed to send relay bind");
            continue;
        }
        // Give the relay a moment to record the binding; a response is not
        // required, so a short wait is enough to prefer a reachable server.
        tokio::time::sleep(Duration::from_millis(200)).await;
        return Some(*addr);
    }
    None
}

fn parse_stun_servers(inputs: &[String]) -> anyhow::Result<Vec<SocketAddr>> {
    if inputs.is_empty() {
        let mut addrs = Vec::new();
        for s in lanlink_punch::DEFAULT_STUN_SERVERS {
            match s.parse() {
                Ok(a) => addrs.push(a),
                Err(e) => {
                    tracing::warn!(server = s, error = %e, "failed to parse default STUN server")
                }
            }
        }
        return Ok(addrs);
    }

    let mut addrs = Vec::new();
    for s in inputs {
        addrs.push(s.parse()?);
    }
    Ok(addrs)
}

/// Submit local candidates to the coordinator, fetch the latest peer list, and
/// spawn punch tasks for any peers we are not already punching. Returns the
/// refreshed peer list.
#[allow(clippy::too_many_arguments)]
async fn refresh_candidates_and_punch(
    coordinator: &str,
    network_id: u64,
    local_node: lanlink_proto::NodeId,
    client: &reqwest::Client,
    puncher: &Arc<lanlink_punch::Puncher>,
    switch: &Arc<Mutex<lanlink_switch::Switch>>,
    punching: &Arc<Mutex<HashSet<lanlink_proto::NodeId>>>,
    local_relay: Option<SocketAddr>,
    local_tcp_addr: SocketAddr,
    local_tcp_relay: Option<SocketAddr>,
    tcp_transport: tcp::TcpTransport,
    tracker: &TaskTracker,
    cancel: CancellationToken,
) -> anyhow::Result<Vec<lanlink_proto::PeerInfo>> {
    let mut candidates = puncher.gather_candidates().await;
    if let Some(relay) = local_relay {
        candidates.push(lanlink_punch::Candidate {
            addr: relay,
            kind: lanlink_punch::CandidateKind::Relay,
        });
    }
    tracing::debug!(count = candidates.len(), "gathered local candidates");

    let tcp_candidates = lanlink_punch::gather_host_candidates(local_tcp_addr.port());
    let update = lanlink_proto::PresenceUpdate {
        node_id: local_node,
        direct_candidates: candidates
            .iter()
            .filter(|c| !matches!(c.kind, lanlink_punch::CandidateKind::Relay))
            .map(|c| c.addr)
            .collect(),
        relay_candidates: candidates
            .iter()
            .filter(|c| matches!(c.kind, lanlink_punch::CandidateKind::Relay))
            .map(|c| c.addr)
            .collect(),
        tcp_candidates: tcp_candidates.into_iter().map(|c| c.addr).collect(),
        tcp_relay_candidates: local_tcp_relay.into_iter().collect(),
    };

    let url = format!("{}/networks/{}/candidates", coordinator, network_id);
    let peers: Vec<lanlink_proto::PeerInfo> = client
        .post(&url)
        .json(&update)
        .send()
        .await?
        .error_for_status()
        .map_err(|e| anyhow::anyhow!("candidate update failed: {e}"))?
        .json()
        .await?;

    switch.lock().await.sync_peers(&peers);
    punch_peers(
        coordinator.to_string(),
        network_id,
        local_node,
        client.clone(),
        puncher.clone(),
        switch.clone(),
        punching.clone(),
        tcp_transport,
        peers.clone(),
        tracker,
        cancel,
    )
    .await;
    Ok(peers)
}

/// Spawn a punch task for every peer that is not already being punched.
#[allow(clippy::too_many_arguments)]
async fn punch_peers(
    coordinator: String,
    network_id: u64,
    local_node: lanlink_proto::NodeId,
    client: reqwest::Client,
    puncher: Arc<lanlink_punch::Puncher>,
    switch: Arc<Mutex<lanlink_switch::Switch>>,
    punching: Arc<Mutex<HashSet<lanlink_proto::NodeId>>>,
    tcp_transport: tcp::TcpTransport,
    peers: Vec<lanlink_proto::PeerInfo>,
    tracker: &TaskTracker,
    cancel: CancellationToken,
) {
    for peer in peers {
        if peer.node_id == local_node {
            continue;
        }

        let mut set = punching.lock().await;
        if set.contains(&peer.node_id) {
            continue;
        }
        set.insert(peer.node_id);
        drop(set);

        let puncher = puncher.clone();
        let switch = switch.clone();
        let punching = punching.clone();
        let coordinator = coordinator.clone();
        let client = client.clone();
        let tcp_transport = tcp_transport.clone();
        let peer_id = peer.node_id;
        let direct_candidates = peer.direct_candidates.clone();
        let relay_candidates = peer.relay_candidates.clone();
        let tcp_candidates = peer.tcp_candidates.clone();
        let tcp_relay_candidates = peer.tcp_relay_candidates.clone();
        let punch_cancel = cancel.clone();

        tracker.spawn(async move {
            tokio::select! {
                _ = punch_cancel.cancelled() => {}
                _ = async {
                    tracing::info!(?peer_id, "starting hole punch");
                    let candidates_with_kind: Vec<lanlink_punch::Candidate> = direct_candidates
                        .into_iter()
                        .map(|addr| lanlink_punch::Candidate {
                            addr,
                            kind: lanlink_punch::CandidateKind::Host,
                        })
                        .collect();

                    match puncher.punch_peer(peer_id, candidates_with_kind).await {
                        Ok(path) => {
                            tracing::info!(?peer_id, addr = %path.addr, "direct path verified");
                            switch.lock().await.set_peer_addr(peer_id, path.addr);
                        }
                        Err(e) => {
                            tracing::warn!(?peer_id, error = %e, "direct punch failed");
                            // Refresh candidates from the coordinator once more in case
                            // the peer updated its reflexive address while we were punching.
                            if let Err(e2) = refresh_single_peer_candidates(
                                &coordinator,
                                network_id,
                                local_node,
                                &client,
                                &puncher,
                                &switch,
                                peer_id,
                            )
                            .await
                            {
                                tracing::debug!(?peer_id, error = %e2, "single peer candidate refresh failed");
                            }

                            // Fallback ordering: direct paths (UDP then TCP) are
                            // preferred over relay paths (UDP then TCP). Traffic stays
                            // E2E-encrypted on every path.
                            if let Some(tcp_addr) = tcp_candidates.first() {
                                tracing::info!(?peer_id, %tcp_addr, "falling back to TCP direct path");
                                tcp_transport.connect(*tcp_addr);
                                {
                                    let mut sw = switch.lock().await;
                                    sw.set_peer_addr(peer_id, *tcp_addr);
                                    sw.set_peer_transport(peer_id, lanlink_switch::Transport::TcpDirect);
                                }
                            } else if let Some(relay_addr) = relay_candidates.first() {
                                tracing::info!(?peer_id, %relay_addr, "falling back to UDP relay path");
                                switch.lock().await.set_peer_addr(peer_id, *relay_addr);
                            } else if let Some(tcp_relay) = tcp_relay_candidates.first() {
                                tracing::info!(?peer_id, %tcp_relay, "falling back to TCP relay path");
                                tcp_transport.connect(*tcp_relay);
                                let bind = lanlink_relay::encode_bind(local_node);
                                tcp_transport.send(*tcp_relay, bind);
                                {
                                    let mut sw = switch.lock().await;
                                    sw.set_peer_addr(peer_id, *tcp_relay);
                                    sw.set_peer_transport(peer_id, lanlink_switch::Transport::TcpRelay);
                                }
                            }
                        }
                    }

                    punching.lock().await.remove(&peer_id);
                } => {}
            }
        });
    }
}

/// Re-fetch the peer list and, if the target peer has new candidates, punch it
/// one more time.
async fn refresh_single_peer_candidates(
    coordinator: &str,
    network_id: u64,
    local_node: lanlink_proto::NodeId,
    client: &reqwest::Client,
    puncher: &Arc<lanlink_punch::Puncher>,
    switch: &Arc<Mutex<lanlink_switch::Switch>>,
    target: lanlink_proto::NodeId,
) -> anyhow::Result<()> {
    let candidates = puncher.gather_candidates().await;
    let update = lanlink_proto::PresenceUpdate {
        node_id: local_node,
        direct_candidates: candidates.clone().into_iter().map(|c| c.addr).collect(),
        relay_candidates: candidates
            .iter()
            .filter(|c| matches!(c.kind, lanlink_punch::CandidateKind::Relay))
            .map(|c| c.addr)
            .collect(),
        tcp_candidates: Vec::new(),
        tcp_relay_candidates: Vec::new(),
    };

    let url = format!("{}/networks/{}/candidates", coordinator, network_id);
    let peers: Vec<lanlink_proto::PeerInfo> = client
        .post(&url)
        .json(&update)
        .send()
        .await?
        .error_for_status()
        .map_err(|e| anyhow::anyhow!("candidate refresh failed: {e}"))?
        .json()
        .await?;

    if let Some(peer) = peers.into_iter().find(|p| p.node_id == target) {
        let candidates_with_kind = peer
            .direct_candidates
            .into_iter()
            .map(|addr| lanlink_punch::Candidate {
                addr,
                kind: lanlink_punch::CandidateKind::Host,
            })
            .collect();
        if let Ok(path) = puncher.punch_peer(target, candidates_with_kind).await {
            switch.lock().await.set_peer_addr(target, path.addr);
        }
    }

    Ok(())
}

/// Coordinated-mode main loop. Wires TAP, switch, UDP socket, and puncher
/// together, routes control packets to the puncher, and handles verified
/// address updates.
#[allow(clippy::too_many_arguments)]
async fn start_coordinated_pump(
    nic: Arc<Mutex<Box<dyn lanlink_tap::VirtualNic>>>,
    switch: Arc<Mutex<lanlink_switch::Switch>>,
    socket: Arc<tokio::net::UdpSocket>,
    tcp_transport: tcp::TcpTransport,
    tcp_incoming: tokio::sync::mpsc::UnboundedReceiver<(SocketAddr, Vec<u8>)>,
    puncher: Arc<lanlink_punch::Puncher>,
    _punching: Arc<Mutex<HashSet<lanlink_proto::NodeId>>>,
    gateway_manager: Option<Arc<lanlink_gateway::GatewayManager>>,
    metrics: Arc<Metrics>,
    mtu: usize,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let mut set = JoinSet::new();

    // TAP -> network
    let nic_tx = nic.clone();
    let switch_tx = switch.clone();
    let socket_tx = socket.clone();
    let tcp_tx = tcp_transport.clone();
    let metrics_tx = metrics.clone();
    set.spawn(async move {
        let mut buf = vec![0u8; mtu + 128];
        loop {
            let n = match nic_tx.lock().await.read_frame(&mut buf).await {
                Ok(0) => {
                    tracing::warn!("TAP returned 0 bytes");
                    continue;
                }
                Ok(n) => n,
                Err(e) => {
                    tracing::error!(error = %e, "TAP read failed");
                    break;
                }
            };
            let frame = &buf[..n];
            let packets = match switch_tx.lock().await.on_tap_frame(frame) {
                Ok(p) => p,
                Err(e) => {
                    tracing::debug!(error = %e, "switch dropped outgoing frame");
                    continue;
                }
            };
            let mut sent_bytes = 0usize;
            for out in packets {
                sent_bytes += out.packet.len();
                match out.transport {
                    lanlink_switch::Transport::Udp => {
                        if let Err(e) = socket_tx.send_to(&out.packet, out.addr).await {
                            tracing::debug!(error = %e, addr = %out.addr, "UDP send failed");
                        }
                    }
                    lanlink_switch::Transport::TcpDirect | lanlink_switch::Transport::TcpRelay => {
                        tcp_tx.send(out.addr, out.packet);
                    }
                }
            }
            metrics_tx.add_tx(sent_bytes);
            tracing::trace!(bytes = n, "sent encrypted frame(s)");
        }
    });

    // UDP -> switch/puncher
    let nic_rx = nic.clone();
    let switch_rx = switch.clone();
    let socket_rx = socket.clone();
    let puncher_rx = puncher.clone();
    let gateway_rx = gateway_manager.clone();
    let metrics_rx = metrics.clone();
    set.spawn(async move {
        let mut buf = vec![0u8; mtu + 256];
        loop {
            let (n, from) = match socket_rx.recv_from(&mut buf).await {
                Ok(r) => r,
                Err(e) if is_udp_icmp_reset(&e) => {
                    // Windows turns ICMP "port unreachable" into WSAECONNRESET
                    // on UDP sockets. Log quietly and keep receiving.
                    tracing::debug!(error = %e, "UDP recv reset by ICMP; continuing");
                    continue;
                }
                Err(e) => {
                    tracing::error!(error = %e, "UDP recv failed");
                    continue;
                }
            };
            let packet = &buf[..n];
            metrics_rx.add_rx(n);

            // The packet type is the second byte of the unencrypted outer header.
            let ptype = packet.get(1).copied().unwrap_or(0);

            match ptype {
                lanlink_proto::PUNCH_PROBE_PACKET_TYPE => {
                    if let Err(e) = puncher_rx.on_punch_probe(from, packet).await {
                        tracing::debug!(%from, error = %e, "punch probe handling failed");
                    }
                }
                lanlink_proto::PUNCH_REPLY_PACKET_TYPE => {
                    match puncher_rx.on_punch_reply(from, packet) {
                        Ok(Some(path)) => {
                            tracing::info!(
                                ?path.peer,
                                addr = %path.addr,
                                "punch reply verified path"
                            );
                            switch_rx.lock().await.set_peer_addr(path.peer, path.addr);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            tracing::debug!(%from, error = %e, "punch reply handling failed");
                        }
                    }
                }
                _ => match switch_rx.lock().await.on_packet(from, packet) {
                    Ok(lanlink_switch::UdpOutcome::Frame(frame)) => {
                        // Gateway mode: intercept ARP requests for the shared
                        // subnet and reply on behalf of the physical LAN.
                        let out_frame = if let Some(ref gw) = gateway_rx {
                            gw.process_inbound_frame(&frame).unwrap_or(frame)
                        } else {
                            frame
                        };
                        if let Err(e) = nic_rx.lock().await.write_frame(&out_frame).await {
                            tracing::error!(error = %e, "TAP write failed");
                        } else {
                            tracing::trace!(bytes = out_frame.len(), "wrote decrypted frame");
                        }
                    }
                    Ok(lanlink_switch::UdpOutcome::Reply(reply)) => {
                        // Latency-probe PONG: route it straight back.
                        if let Err(e) = socket_rx.send_to(&reply, from).await {
                            tracing::debug!(%from, error = %e, "probe reply send failed");
                        }
                    }
                    Ok(lanlink_switch::UdpOutcome::HandshakeReply(reply)) => {
                        if let Err(e) = socket_rx.send_to(&reply, from).await {
                            tracing::debug!(%from, error = %e, "handshake reply send failed");
                        }
                    }
                    Ok(lanlink_switch::UdpOutcome::Ignored) => {
                        tracing::trace!(%from, "dropped/fragment not ready/keepalive");
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, %from, "failed to process packet");
                    }
                },
            }
        }
    });

    // TCP -> switch. Incoming packets from direct TCP peers and TCP relay
    // servers share the same outer framing as UDP packets, so they are fed
    // into the switch as if they had arrived over UDP. Replies (handshakes,
    // PONGs) are written back to the same TCP connection.
    let switch_tcp = switch.clone();
    let mut tcp_rx = tcp_incoming;
    let tcp_reply = tcp_transport.clone();
    let nic_tcp = nic.clone();
    let gateway_tcp = gateway_manager.clone();
    set.spawn(async move {
        while let Some((from, packet)) = tcp_rx.recv().await {
            match switch_tcp.lock().await.on_packet(from, &packet) {
                Ok(lanlink_switch::UdpOutcome::Frame(frame)) => {
                    let out_frame = if let Some(ref gw) = gateway_tcp {
                        gw.process_inbound_frame(&frame).unwrap_or(frame)
                    } else {
                        frame
                    };
                    if let Err(e) = nic_tcp.lock().await.write_frame(&out_frame).await {
                        tracing::error!(error = %e, "TAP write failed");
                    } else {
                        tracing::trace!(bytes = out_frame.len(), "wrote decrypted frame");
                    }
                }
                Ok(lanlink_switch::UdpOutcome::Reply(reply)) => {
                    tcp_reply.send(from, reply);
                }
                Ok(lanlink_switch::UdpOutcome::HandshakeReply(reply)) => {
                    tcp_reply.send(from, reply);
                }
                Ok(lanlink_switch::UdpOutcome::Ignored) => {
                    tracing::trace!(%from, "dropped/fragment not ready/keepalive");
                }
                Err(e) => {
                    tracing::debug!(error = %e, %from, "failed to process TCP packet");
                }
            }
        }
    });

    // Keepalive: send an empty authenticated packet to every known peer every
    // 20 s to keep NAT mappings alive.
    let switch_kp = switch.clone();
    let socket_kp = socket.clone();
    let tcp_kp = tcp_transport.clone();
    set.spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(20));
        loop {
            interval.tick().await;
            let mut sw = switch_kp.lock().await;
            let peers = sw.peer_addrs();
            let mut packets: Vec<lanlink_switch::Outgoing> = Vec::new();
            for (id, _addr) in peers {
                match sw.seal_keepalive(id) {
                    Ok(out) => packets.push(out),
                    Err(e) => tracing::debug!(?id, error = %e, "keepalive seal failed"),
                }
            }
            drop(sw);
            for out in packets {
                match out.transport {
                    lanlink_switch::Transport::Udp => {
                        if let Err(e) = socket_kp.send_to(&out.packet, out.addr).await {
                            tracing::debug!(%out.addr, error = %e, "keepalive UDP send failed");
                        }
                    }
                    lanlink_switch::Transport::TcpDirect | lanlink_switch::Transport::TcpRelay => {
                        tcp_kp.send(out.addr, out.packet);
                    }
                }
            }
        }
    });

    // Latency probes: ping every known peer every 5 s so the status API has
    // fresh RTT numbers. Rides the same sealed control channel and counter
    // stream as data packets; PONG replies are handled in the rx loop.
    let switch_ping = switch.clone();
    let socket_ping = socket.clone();
    let tcp_ping = tcp_transport.clone();
    set.spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            let mut sends = Vec::new();
            {
                let mut sw = switch_ping.lock().await;
                let now = sw.probe_clock_ms();
                for (id, _addr) in sw.peer_addrs() {
                    match sw.seal_ping(id, now) {
                        Ok(out) => sends.push(out),
                        Err(e) => tracing::debug!(?id, error = %e, "ping seal failed"),
                    }
                }
            }
            for out in sends {
                match out.transport {
                    lanlink_switch::Transport::Udp => {
                        if let Err(e) = socket_ping.send_to(&out.packet, out.addr).await {
                            tracing::debug!(%out.addr, error = %e, "ping UDP send failed");
                        }
                    }
                    lanlink_switch::Transport::TcpDirect | lanlink_switch::Transport::TcpRelay => {
                        tcp_ping.send(out.addr, out.packet);
                    }
                }
            }
        }
    });

    loop {
        tokio::select! {
            _ = shutdown().wait() => {
                tracing::info!("shutting down");
                set.abort_all();
                break;
            }
            _ = cancel.cancelled() => {
                tracing::info!("session cancelled; leaving network");
                set.abort_all();
                break;
            }
            res = set.join_next() => {
                match res {
                    Some(Ok(())) => {
                        tracing::debug!("pump subtask exited");
                    }
                    Some(Err(e)) if e.is_cancelled() => {}
                    Some(Err(e)) => {
                        if let Ok(payload) = e.try_into_panic() {
                            std::panic::resume_unwind(payload);
                        }
                    }
                    None => break,
                }
            }
        }
    }

    // Wait for every spawned pump task to actually stop so its sockets are
    // released before the caller re-binds the same ports.
    while set.join_next().await.is_some() {}

    Ok(())
}

/// Shared TAP <-> UDP loop used by the static-peer mode.
async fn start_packet_pump(
    nic: Arc<Mutex<Box<dyn lanlink_tap::VirtualNic>>>,
    switch: Arc<Mutex<lanlink_switch::Switch>>,
    socket: Arc<tokio::net::UdpSocket>,
    mtu: usize,
) -> anyhow::Result<()> {
    // TAP -> UDP
    let nic_tx = nic.clone();
    let switch_tx = switch.clone();
    let socket_tx = socket.clone();
    let mut tx_task = tokio::spawn(async move {
        let mut buf = vec![0u8; mtu + 128];
        loop {
            let n = match nic_tx.lock().await.read_frame(&mut buf).await {
                Ok(0) => {
                    tracing::warn!("TAP returned 0 bytes");
                    continue;
                }
                Ok(n) => n,
                Err(e) => {
                    tracing::error!(error = %e, "TAP read failed");
                    break;
                }
            };
            let frame = &buf[..n];
            let packets = match switch_tx.lock().await.on_tap_frame(frame) {
                Ok(p) => p,
                Err(e) => {
                    tracing::debug!(error = %e, "switch dropped outgoing frame");
                    continue;
                }
            };
            for out in packets {
                if let Err(e) = socket_tx.send_to(&out.packet, out.addr).await {
                    tracing::debug!(error = %e, addr = %out.addr, "UDP send failed");
                }
            }
            tracing::trace!(bytes = n, "sent encrypted frame(s)");
        }
    });

    // UDP -> TAP
    let nic_rx = nic.clone();
    let switch_rx = switch.clone();
    let socket_rx = socket.clone();
    let mut rx_task = tokio::spawn(async move {
        let mut buf = vec![0u8; mtu + 256];
        loop {
            let (n, from) = match socket_rx.recv_from(&mut buf).await {
                Ok(r) => r,
                Err(e) if is_udp_icmp_reset(&e) => {
                    tracing::debug!(error = %e, "UDP recv reset by ICMP; continuing");
                    continue;
                }
                Err(e) => {
                    tracing::error!(error = %e, "UDP recv failed");
                    continue;
                }
            };
            let packet = &buf[..n];
            match switch_rx.lock().await.on_packet(from, packet) {
                Ok(lanlink_switch::UdpOutcome::Frame(frame)) => {
                    if let Err(e) = nic_rx.lock().await.write_frame(&frame).await {
                        tracing::error!(error = %e, "TAP write failed");
                    } else {
                        tracing::trace!(bytes = frame.len(), "wrote decrypted frame");
                    }
                }
                Ok(lanlink_switch::UdpOutcome::Reply(reply)) => {
                    if let Err(e) = socket_rx.send_to(&reply, from).await {
                        tracing::debug!(%from, error = %e, "probe reply send failed");
                    }
                }
                Ok(lanlink_switch::UdpOutcome::HandshakeReply(reply)) => {
                    if let Err(e) = socket_rx.send_to(&reply, from).await {
                        tracing::debug!(%from, error = %e, "handshake reply send failed");
                    }
                }
                Ok(lanlink_switch::UdpOutcome::Ignored) => {
                    tracing::trace!(%from, "dropped/fragment not ready");
                }
                Err(e) => {
                    tracing::debug!(error = %e, %from, "failed to process packet");
                }
            }
        }
    });

    tokio::select! {
        _ = shutdown().wait() => {
            tracing::info!("shutting down");
            tx_task.abort();
            rx_task.abort();
        }
        _ = &mut tx_task => {}
        _ = &mut rx_task => {}
    }

    Ok(())
}

fn build_telemetry_report(
    start_time: &Instant,
    metrics: &Metrics,
    switch: &Arc<Mutex<lanlink_switch::Switch>>,
    gateway_enabled: bool,
    firewall_enabled: bool,
    relay_servers: &[SocketAddr],
) -> lanlink_proto::telemetry::TelemetryReport {
    let addrs = {
        let sw = switch.blocking_lock();
        sw.peer_addrs()
    };
    let peer_count = addrs.len() as u32;
    let relay_set: std::collections::HashSet<SocketAddr> = relay_servers.iter().copied().collect();
    let relay_peer_count = addrs
        .iter()
        .filter(|(_, addr)| relay_set.contains(addr))
        .count() as u32;
    let direct_peer_count = peer_count.saturating_sub(relay_peer_count);

    let uptime_seconds = start_time.elapsed().as_secs();
    let timestamp = format_utc_rfc3339();

    lanlink_proto::telemetry::TelemetryReport {
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::ARCH.to_string() + "-" + std::env::consts::OS,
        uptime_seconds,
        peer_count,
        direct_peer_count,
        relay_peer_count,
        gateway_enabled,
        firewall_enabled,
        tx_bytes: metrics.tx_bytes(),
        rx_bytes: metrics.rx_bytes(),
        timestamp,
    }
}

fn format_utc_rfc3339() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days_since_epoch = secs / 86400;
    // 1970-01-01 was a Thursday; offset so day 0 = Thursday.
    let mut year = 1970;
    let mut remaining_days = days_since_epoch as i64;
    loop {
        let leap = is_leap_year(year);
        let year_days = if leap { 366 } else { 365 };
        if remaining_days < year_days {
            break;
        }
        remaining_days -= year_days;
        year += 1;
    }
    let (month, day) = day_of_year_to_month_day(remaining_days as u32, is_leap_year(year));
    let hour = (secs % 86400) / 3600;
    let min = (secs % 3600) / 60;
    let sec = secs % 60;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hour, min, sec
    )
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn day_of_year_to_month_day(day: u32, leap: bool) -> (u32, u32) {
    let days_in_month: &[u32] = if leap {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut remaining = day;
    for (i, &dim) in days_in_month.iter().enumerate() {
        if remaining < dim {
            return (i as u32 + 1, remaining + 1);
        }
        remaining -= dim;
    }
    (12, remaining + 1)
}

fn prefix_to_netmask(prefix: u8) -> anyhow::Result<std::net::Ipv4Addr> {
    if prefix > 32 {
        anyhow::bail!("prefix must be <= 32");
    }
    let bits = if prefix == 0 {
        0u32
    } else {
        (!0u32) << (32 - prefix)
    };
    // `Ipv4Addr::from(u32)` interprets the value as big-endian (network order)
    // directly. Do NOT call `.to_be()` — on little-endian that swaps the bytes
    // and produces an invalid netmask such as 0.255.255.255 for /24.
    Ok(std::net::Ipv4Addr::from(bits))
}

/// Bind a UDP socket and disable ICMP "port unreachable" / "network
/// unreachable" errors on Windows. Without this, a UDP send to a closed port
/// causes later `recv_from` to fail with WSAECONNRESET (10054), even though
/// UDP is connectionless.
async fn bind_udp_socket(bind: SocketAddr) -> anyhow::Result<tokio::net::UdpSocket> {
    let std_socket = std::net::UdpSocket::bind(bind)?;

    #[cfg(windows)]
    disable_udp_icmp_errors(&std_socket);

    Ok(tokio::net::UdpSocket::from_std(std_socket)?)
}

/// True when a UDP recv/send error is just Windows reporting an ICMP
/// "port/network unreachable" back to the socket (WSAECONNRESET, os error 10054).
/// These are expected when a peer endpoint goes away and should not be treated
/// as fatal.
fn is_udp_icmp_reset(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::ConnectionReset || e.raw_os_error() == Some(10054)
}

#[test]
fn is_udp_icmp_reset_detects_winsock_connreset() {
    let e = std::io::Error::from_raw_os_error(10054);
    assert!(
        is_udp_icmp_reset(&e),
        "raw os error 10054 should be classified as ICMP reset"
    );
}

#[test]
fn is_udp_icmp_reset_detects_connection_reset_kind() {
    let e = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "icmp reset");
    assert!(
        is_udp_icmp_reset(&e),
        "ConnectionReset kind should be classified as ICMP reset"
    );
}

#[test]
fn is_udp_icmp_reset_ignores_other_errors() {
    let e = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
    assert!(
        !is_udp_icmp_reset(&e),
        "unrelated errors should not be classified as ICMP reset"
    );
}

#[cfg(windows)]
fn disable_udp_icmp_errors(socket: &std::net::UdpSocket) {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{WSAIoctl, IOC_IN, IOC_VENDOR, SOCKET_ERROR};

    const SIO_UDP_CONNRESET: u32 = IOC_IN | IOC_VENDOR | 12;
    const SIO_UDP_NETRESET: u32 = IOC_IN | IOC_VENDOR | 15;

    fn set_ioctl(sock: usize, code: u32, name: &str) {
        let mut disabled: u32 = 0;
        let mut bytes_returned: u32 = 0;
        let ret = unsafe {
            WSAIoctl(
                sock,
                code,
                &mut disabled as *mut _ as *mut _,
                std::mem::size_of::<u32>() as u32,
                std::ptr::null_mut(),
                0,
                &mut bytes_returned,
                std::ptr::null_mut(),
                None,
            )
        };
        if ret == SOCKET_ERROR {
            tracing::warn!("failed to disable {name} on UDP socket; continuing anyway");
        } else {
            tracing::debug!("disabled {name} on UDP socket");
        }
    }

    let sock = socket.as_raw_socket() as usize;
    set_ioctl(sock, SIO_UDP_CONNRESET, "SIO_UDP_CONNRESET");
    set_ioctl(sock, SIO_UDP_NETRESET, "SIO_UDP_NETRESET");
}

#[test]
fn prefix_to_netmask_common_prefixes() {
    assert_eq!(
        prefix_to_netmask(24).unwrap(),
        std::net::Ipv4Addr::new(255, 255, 255, 0)
    );
    assert_eq!(
        prefix_to_netmask(8).unwrap(),
        std::net::Ipv4Addr::new(255, 0, 0, 0)
    );
    assert_eq!(
        prefix_to_netmask(16).unwrap(),
        std::net::Ipv4Addr::new(255, 255, 0, 0)
    );
    assert_eq!(
        prefix_to_netmask(30).unwrap(),
        std::net::Ipv4Addr::new(255, 255, 255, 252)
    );
    assert_eq!(
        prefix_to_netmask(0).unwrap(),
        std::net::Ipv4Addr::new(0, 0, 0, 0)
    );
    assert_eq!(
        prefix_to_netmask(32).unwrap(),
        std::net::Ipv4Addr::new(255, 255, 255, 255)
    );
    assert!(prefix_to_netmask(33).is_err());
}

fn node_id_from_u64(v: u64) -> lanlink_proto::NodeId {
    lanlink_proto::NodeId(v.to_be_bytes())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Load the persistent Ed25519 identity, creating one if it does not exist.
fn load_or_create_identity() -> anyhow::Result<lanlink_crypto::Identity> {
    use std::fs;
    use std::path::PathBuf;

    // Prefer a system-wide location when running as root, otherwise fall back
    // to the user's config directory.
    let mut candidates: Vec<PathBuf> = Vec::new();
    #[cfg(unix)]
    candidates.push(PathBuf::from("/var/lib/lanlink/identity.key"));
    if let Some(home) = std::env::var_os("HOME") {
        let mut p = PathBuf::from(home);
        p.push(".config");
        p.push("lanlink");
        p.push("identity.key");
        candidates.push(p);
    }
    #[cfg(windows)]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let mut p = PathBuf::from(appdata);
        p.push("LanLink");
        p.push("identity.key");
        candidates.push(p);
    }
    candidates.push(PathBuf::from("/tmp/lanlink_identity.key"));

    let path = candidates
        .into_iter()
        .find(|p| p.exists() || p.parent().map(|d| d.exists()).unwrap_or(false))
        .unwrap_or_else(|| PathBuf::from("/tmp/lanlink_identity.key"));

    if path.exists() {
        let bytes = fs::read(&path)?;
        if bytes.len() != 32 {
            anyhow::bail!("identity file does not contain a 32-byte Ed25519 secret key");
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        return Ok(lanlink_crypto::Identity::from_secret_bytes(key));
    }

    let identity = lanlink_crypto::Identity::generate();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&path, identity.secret_key_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&path, perms)?;
    }
    tracing::info!(path = %path.display(), "created new identity");
    Ok(identity)
}
