use clap::Parser;
use std::net::SocketAddr;

#[derive(Parser, Debug)]
#[command(
    name = "lanlink-relayd",
    about = "LanLink content-blind UDP relay server"
)]
struct Args {
    /// UDP listen address.
    #[arg(long, default_value = "0.0.0.0:3478")]
    listen: SocketAddr,
    /// Optional TCP listen address. When set, a TCP relay server is started
    /// alongside the UDP relay. Clients connect and send length-prefixed
    /// LanLink frames.
    #[arg(long)]
    tcp_listen: Option<SocketAddr>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let server = match lanlink_relay::RelayServer::bind(args.listen).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "failed to bind UDP relay server");
            std::process::exit(1);
        }
    };

    let local_addr = server.local_addr().unwrap_or(args.listen);

    let mut tcp_task = None;
    if let Some(tcp_addr) = args.tcp_listen {
        match lanlink_relay::TcpRelayServer::bind(tcp_addr).await {
            Ok(tcp_server) => {
                tracing::info!(%tcp_addr, "TCP relay server listening");
                tcp_task = Some(tokio::spawn(async move {
                    if let Err(e) = tcp_server.run().await {
                        tracing::error!(error = %e, "TCP relay server exited");
                    }
                }));
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to bind TCP relay server");
                std::process::exit(1);
            }
        }
    }

    tracing::info!(%local_addr, "lanlink-relayd running; press Ctrl-C to stop");

    let result = server.run().await;
    if let Some(t) = tcp_task {
        t.abort();
    }
    if let Err(e) = result {
        tracing::error!(error = %e, "relay server exited");
        std::process::exit(1);
    }
}
