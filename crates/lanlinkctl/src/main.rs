use base64::Engine;
use clap::{Parser, Subcommand};
use lanlink_types::RECOMMENDED_MTU;
use rand::RngCore;
use std::net::SocketAddr;

mod control;

#[derive(Parser, Debug)]
#[command(
    name = "lanlinkctl",
    about = "Control the LanLink daemon and coordinator"
)]
struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Print the client, daemon protocol, and wire versions.
    Version,
    /// Local daemon control/status commands.
    #[command(subcommand)]
    Daemon(DaemonCmd),
    /// Update-channel commands (opt-in; LanLink never phones home unless asked).
    #[command(subcommand)]
    Update(UpdateCmd),
    /// Coordinator commands.
    #[command(subcommand)]
    Coord(CoordCmd),
    /// Public gaming/directory server commands.
    #[command(subcommand)]
    Directory(DirectoryCmd),
}

#[derive(Subcommand, Debug)]
enum DaemonCmd {
    /// Print the daemon status snapshot.
    Status {
        #[arg(short, long)]
        socket: Option<String>,
        /// Output raw JSON instead of the human-readable table.
        #[arg(long)]
        json: bool,
    },
    /// Gracefully stop the daemon. Returns only after the process exits.
    Stop {
        #[arg(short, long)]
        socket: Option<String>,
        /// Maximum seconds to wait for the daemon to exit after the ack.
        #[arg(long, default_value = "30")]
        timeout: u64,
    },
    /// Disconnect from the current network but keep the daemon running.
    Disconnect {
        #[arg(short, long)]
        socket: Option<String>,
    },
    /// Connect the running daemon to a network.
    Connect {
        #[arg(short, long)]
        socket: Option<String>,
        #[arg(long, default_value = "http://localhost:8080")]
        coordinator: String,
        #[arg(short, long)]
        network_id: u64,
        #[arg(short, long)]
        password: String,
        #[arg(long, default_value = "0.0.0.0:9000")]
        bind: SocketAddr,
        #[arg(long, default_value = "lanlink0")]
        tap: String,
        #[arg(long, default_value_t = RECOMMENDED_MTU)]
        mtu: usize,
        /// Comma-separated list of STUN servers (empty means disable STUN).
        #[arg(long, value_delimiter = ',')]
        stun_servers: Vec<String>,
        /// Act as a gateway for the shared physical subnet.
        #[arg(long)]
        gateway: bool,
        /// Physical subnet to share when gateway mode is enabled.
        #[arg(long, default_value = "")]
        shared_subnet: String,
        /// Outbound physical interface used by gateway mode.
        #[arg(long, default_value = "")]
        phys_interface: String,
    },
}

#[derive(Subcommand, Debug)]
enum DirectoryCmd {
    /// List public networks on this coordinator.
    List {
        #[arg(long, default_value = "http://localhost:8080")]
        coordinator: String,
    },
}

#[derive(Subcommand, Debug)]
enum UpdateCmd {
    /// Fetch the release manifest and compare it against this build.
    Check {
        /// Manifest URL (defaults to the public update channel).
        #[arg(long)]
        manifest: Option<String>,
        /// Print the full manifest for inspection.
        #[arg(long)]
        verbose: bool,
    },
}

#[derive(Subcommand, Debug)]
enum CoordCmd {
    /// Create a new virtual LAN network.
    Create {
        #[arg(long, default_value = "http://localhost:8080")]
        coordinator: String,
        #[arg(short, long)]
        name: String,
        #[arg(short, long)]
        password: String,
        /// Mark the network as public/gaming (enables inbound firewall).
        #[arg(long)]
        public: bool,
    },
    /// Join a network and print assigned IP + peers.
    Join {
        #[arg(long, default_value = "http://localhost:8080")]
        coordinator: String,
        #[arg(short, long)]
        network_id: u64,
        #[arg(short, long)]
        password: String,
        /// Direct UDP candidate to advertise (defaults to 127.0.0.1:9000).
        #[arg(long, default_value = "127.0.0.1:9000")]
        candidate: SocketAddr,
    },
    /// List members of a network.
    List {
        #[arg(long, default_value = "http://localhost:8080")]
        coordinator: String,
        #[arg(short, long)]
        network_id: u64,
        #[arg(short, long)]
        password: String,
    },
    /// Show aggregated telemetry metrics (operator view).
    Metrics {
        #[arg(long, default_value = "http://localhost:8080")]
        coordinator: String,
    },
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    if let Err(e) = run(args.cmd).await {
        eprintln!("lanlinkctl error: {e}");
        std::process::exit(1);
    }
}

async fn run(cmd: Cmd) -> anyhow::Result<()> {
    let client = reqwest::Client::new();

    match cmd {
        Cmd::Daemon(sub) => match sub {
            DaemonCmd::Status { socket, json } => {
                let resp =
                    control::send_request(lanlink_proto::control::ControlRequest::Status, socket)
                        .await?;
                match resp {
                    lanlink_proto::control::ControlResponse::Ok {
                        result: lanlink_proto::control::ControlResult::Status(snap),
                    } => {
                        if json {
                            println!("{}", serde_json::to_string_pretty(&snap)?);
                        } else {
                            control::print_status(&snap);
                        }
                    }
                    lanlink_proto::control::ControlResponse::Ok { .. } => {
                        anyhow::bail!("unexpected status response");
                    }
                    lanlink_proto::control::ControlResponse::Error { error } => {
                        anyhow::bail!("daemon error: {error}");
                    }
                }
            }
            DaemonCmd::Stop { socket, timeout } => {
                control::stop_daemon(socket, std::time::Duration::from_secs(timeout)).await?;
                println!("daemon stopped");
            }
            DaemonCmd::Disconnect { socket } => {
                let resp = control::send_request(
                    lanlink_proto::control::ControlRequest::Disconnect,
                    socket,
                )
                .await?;
                match resp {
                    lanlink_proto::control::ControlResponse::Ok { .. } => {
                        println!("disconnected");
                    }
                    lanlink_proto::control::ControlResponse::Error { error } => {
                        anyhow::bail!("daemon error: {error}");
                    }
                }
            }
            DaemonCmd::Connect {
                socket,
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
                let req = lanlink_proto::control::ControlRequest::Connect {
                    coordinator,
                    network_id,
                    password: password.into(),
                    bind,
                    tap,
                    mtu,
                    stun_servers,
                    gateway,
                    shared_subnet,
                    phys_interface,
                };
                let resp = control::send_request(req, socket).await?;
                match resp {
                    lanlink_proto::control::ControlResponse::Ok { .. } => {
                        println!("connect requested");
                    }
                    lanlink_proto::control::ControlResponse::Error { error } => {
                        anyhow::bail!("daemon error: {error}");
                    }
                }
            }
        },
        Cmd::Version => {
            println!("lanlinkctl {}", env!("CARGO_PKG_VERSION"));
            println!("protocol {}", lanlink_proto::PROTOCOL_VERSION);
            println!("wire {}", lanlink_types::WIRE_PROTOCOL_VERSION);
        }
        Cmd::Update(UpdateCmd::Check { manifest, verbose }) => {
            let url =
                manifest.unwrap_or_else(|| lanlink_proto::update::DEFAULT_MANIFEST_URL.to_string());
            let m: lanlink_proto::update::UpdateManifest = client
                .get(&url)
                .send()
                .await?
                .error_for_status()
                .map_err(|e| anyhow::anyhow!("failed to fetch manifest: {e}"))?
                .json()
                .await
                .map_err(|e| anyhow::anyhow!("failed to parse manifest: {e}"))?;

            if m.product != "lanlink" {
                anyhow::bail!("manifest is for product {:?}, not \"lanlink\"", m.product);
            }
            if verbose {
                println!("{}", serde_json::to_string_pretty(&m)?);
            }

            match lanlink_proto::update::check_manifest(&m, env!("CARGO_PKG_VERSION")) {
                lanlink_proto::update::UpdateCheck::Available {
                    running,
                    latest,
                    notes,
                } => {
                    println!("update available: {running} -> {latest}");
                    if let Some(notes) = notes {
                        println!("  notes: {notes}");
                    }
                    println!("  see: {url}");
                    println!("  install via your package manager (deb/rpm/MSI/pkg) or the platform artifacts above");
                }
                lanlink_proto::update::UpdateCheck::UpToDate { running, latest } => {
                    println!("up to date (running {running}, channel {latest})");
                }
                lanlink_proto::update::UpdateCheck::Unknown => {
                    println!(
                        "could not compare manifest version {:?} with running {}",
                        m.version,
                        env!("CARGO_PKG_VERSION")
                    );
                }
            }
        }
        Cmd::Coord(CoordCmd::Create {
            coordinator,
            name,
            password,
            public,
        }) => {
            let req = lanlink_proto::CreateNetworkRequest {
                name,
                password: password.into(),
                is_public: public,
            };
            let resp: lanlink_proto::CreateNetworkResponse = client
                .post(format!("{}/networks", coordinator))
                .json(&req)
                .send()
                .await?
                .error_for_status()
                .map_err(|e| anyhow::anyhow!("create failed: {e}"))?
                .json()
                .await?;
            println!(
                "Created network {} (id={}, subnet={}, public={})",
                resp.network_id.0, resp.network_id.0, resp.subnet, resp.is_public
            );
        }
        Cmd::Coord(CoordCmd::Join {
            coordinator,
            network_id,
            password,
            candidate,
        }) => {
            let mut node_id = [0u8; 8];
            rand::thread_rng().fill_bytes(&mut node_id);
            let req = lanlink_proto::JoinNetworkRequest {
                password: password.into(),
                node_id: lanlink_types::NodeId(node_id),
                public_key: vec![],
                direct_candidates: vec![candidate],
                relay_candidates: vec![],
                tcp_candidates: vec![],
                tcp_relay_candidates: vec![],
                shared_subnet: String::new(),
            };
            let resp: lanlink_proto::JoinNetworkResponse = client
                .post(format!("{}/networks/{}/join", coordinator, network_id))
                .json(&req)
                .send()
                .await?
                .error_for_status()
                .map_err(|e| anyhow::anyhow!("join failed: {e}"))?
                .json()
                .await?;
            println!("Joined network {}", resp.network_id.0);
            println!("Assigned IP: {}", resp.assigned_ip.0);
            println!("Peers: {}", resp.peers.len());
            for p in resp.peers {
                println!(
                    "  {} {} direct={:?} relay={:?} gateway={} shared={}",
                    base64::engine::general_purpose::STANDARD.encode(p.node_id.0),
                    p.virtual_ip.0,
                    p.direct_candidates,
                    p.relay_candidates,
                    p.is_gateway,
                    p.shared_subnet
                );
            }
        }
        Cmd::Coord(CoordCmd::List {
            coordinator,
            network_id,
            password,
        }) => {
            let peers: Vec<lanlink_proto::PeerInfo> = client
                .get(format!("{}/networks/{}/members", coordinator, network_id))
                .header("Authorization", password)
                .send()
                .await?
                .error_for_status()
                .map_err(|e| anyhow::anyhow!("list failed: {e}"))?
                .json()
                .await?;
            println!("Members of network {}: {}", network_id, peers.len());
            for p in peers {
                println!(
                    "  {} {} direct={:?} relay={:?} gateway={} shared={}",
                    base64::engine::general_purpose::STANDARD.encode(p.node_id.0),
                    p.virtual_ip.0,
                    p.direct_candidates,
                    p.relay_candidates,
                    p.is_gateway,
                    p.shared_subnet
                );
            }
        }
        Cmd::Coord(CoordCmd::Metrics { coordinator }) => {
            let metrics: lanlink_proto::telemetry::AggregatedMetrics = client
                .get(format!("{}/metrics", coordinator))
                .send()
                .await?
                .error_for_status()
                .map_err(|e| anyhow::anyhow!("metrics failed: {e}"))?
                .json()
                .await?;
            println!(
                "Aggregated telemetry (window start: {})",
                metrics.window_start
            );
            println!("  reports: {}", metrics.report_count);
            println!(
                "  avg peers: {:.1} (direct {:.1}, relay {:.1})",
                metrics.avg_peer_count, metrics.avg_direct_peers, metrics.avg_relay_peers
            );
            println!(
                "  gateway fraction: {:.1}%",
                metrics.gateway_fraction * 100.0
            );
            println!(
                "  firewall fraction: {:.1}%",
                metrics.firewall_fraction * 100.0
            );
            println!("  total tx: {} bytes", metrics.total_tx_bytes);
            println!("  total rx: {} bytes", metrics.total_rx_bytes);
            if !metrics.versions.is_empty() {
                println!("  versions:");
                for v in metrics.versions {
                    println!("    {}: {}", v.version, v.count);
                }
            }
            if !metrics.platforms.is_empty() {
                println!("  platforms:");
                for p in metrics.platforms {
                    println!("    {}: {}", p.platform, p.count);
                }
            }
        }
        Cmd::Directory(DirectoryCmd::List { coordinator }) => {
            let networks: Vec<lanlink_proto::PublicNetworkInfo> = client
                .get(format!("{}/directory", coordinator))
                .send()
                .await?
                .error_for_status()
                .map_err(|e| anyhow::anyhow!("directory failed: {e}"))?
                .json()
                .await?;
            println!("Public networks: {}", networks.len());
            for n in networks {
                println!(
                    "  {} {} {} members={} public={}",
                    n.network_id.0, n.name, n.subnet, n.member_count, n.is_public
                );
            }
        }
    }

    Ok(())
}
