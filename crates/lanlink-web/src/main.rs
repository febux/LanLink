//! `lanlink-web` — browser-based management UI for LanLink.
//!
//! Serves a small vanilla-JS SPA and proxies two existing surfaces:
//! - the daemon's local Unix-domain status socket (read-only snapshots), and
//! - the coordinator REST API (create/join/directory).
//!
//! The binary embeds its HTML/CSS/JS assets with `include_str!`, so no extra
//! build toolchain is required.

mod api;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::Router;
use clap::Parser;
use tokio::sync::Mutex;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use api::AppState;

const INDEX_HTML: &str = include_str!("assets/index.html");
const STYLE_CSS: &str = include_str!("assets/style.css");
const APP_JS: &str = include_str!("assets/app.js");

#[derive(Parser, Debug)]
#[command(
    name = "lanlink-web",
    about = "LanLink browser UI: live daemon status, directory, create/join"
)]
struct Args {
    /// Address to bind the HTTP server.
    #[arg(long, default_value = "127.0.0.1:8686")]
    bind: SocketAddr,

    /// Coordinator base URL.
    #[arg(long, default_value = "http://localhost:8080")]
    coordinator: String,

    /// Daemon status socket path. Defaults to auto-discovery of
    /// /run/lanlink/lanlinkd-*.sock or /tmp/lanlinkd-*.sock.
    #[arg(long)]
    status_socket: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("failed to build HTTP client");

    let state = AppState {
        coordinator: args.coordinator,
        status_socket: Arc::new(Mutex::new(args.status_socket)),
        http,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(index))
        .route("/style.css", get(style_css))
        .route("/app.js", get(app_js))
        .route("/api/health", get(api::health))
        .route("/api/snapshot", get(api::snapshot))
        .route("/api/directory", get(api::directory))
        .route("/api/networks", post(api::create_network))
        .route("/api/networks/:id/join", post(api::join_network))
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(cors),
        )
        .with_state(state);

    tracing::info!(%args.bind, "lanlink-web listening");
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )],
        Html(INDEX_HTML),
    )
}

async fn style_css() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, HeaderValue::from_static("text/css"))],
        STYLE_CSS,
    )
}

async fn app_js() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/javascript; charset=utf-8"),
        )],
        APP_JS,
    )
}
