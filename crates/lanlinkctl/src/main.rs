use base64::Engine;
use clap::{Parser, Subcommand};
use rand::RngCore;
use std::net::SocketAddr;

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
        Cmd::Version => {
            println!("lanlinkctl {}", env!("CARGO_PKG_VERSION"));
            println!("protocol {}", lanlink_proto::PROTOCOL_VERSION);
            println!("wire {}", lanlink_proto::WIRE_PROTOCOL_VERSION);
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
                password,
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
                password,
                node_id: lanlink_proto::NodeId(node_id),
                public_key: vec![],
                direct_candidates: vec![candidate],
                relay_candidates: vec![],
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
        }) => {
            let peers: Vec<lanlink_proto::PeerInfo> = client
                .get(format!("{}/networks/{}/members", coordinator, network_id))
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
