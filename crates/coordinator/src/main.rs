use clap::Parser;
use std::net::SocketAddr;

#[derive(Parser, Debug)]
#[command(name = "coordinator", about = "LanLink membership coordination server")]
struct Args {
    /// Listen address.
    #[arg(long, default_value = "0.0.0.0:8080")]
    listen: SocketAddr,
    /// SQLite database path.
    #[arg(long, default_value = "lanlink.db")]
    db: String,
    /// Relay server address(es) advertised to joining members for fallback traffic.
    /// This should be the public address where `lanlink-relayd` is reachable, not
    /// the relay's local bind address.
    #[arg(long, value_delimiter = ',')]
    relay_server: Vec<SocketAddr>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let relay_servers = if args.relay_server.is_empty() {
        vec!["127.0.0.1:3478"
            .parse()
            .expect("default relay address valid")]
    } else {
        args.relay_server
    };

    let pool = coordinator::db::connect(&args.db)
        .await
        .expect("failed to open database");
    let app = coordinator::app(pool, relay_servers.clone());

    let listener = tokio::net::TcpListener::bind(args.listen).await.unwrap();
    tracing::info!(%args.listen, ?relay_servers, "coordinator listening");
    axum::serve(listener, app).await.unwrap();
}
