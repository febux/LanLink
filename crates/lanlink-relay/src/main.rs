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
            tracing::error!(error = %e, "failed to bind relay server");
            std::process::exit(1);
        }
    };

    tracing::info!(addr = %args.listen, "lanlink-relayd running; press Ctrl-C to stop");

    if let Err(e) = server.run().await {
        tracing::error!(error = %e, "relay server exited");
        std::process::exit(1);
    }
}
