pub mod api;
pub mod db;

use std::sync::Arc;

/// Build the coordinator axum application around an existing SQLite pool.
pub fn app(
    pool: sqlx::SqlitePool,
    relay_servers: Vec<std::net::SocketAddr>,
    tcp_relay_servers: Vec<std::net::SocketAddr>,
) -> axum::Router {
    let state = Arc::new(api::AppState {
        pool,
        relay_servers,
        tcp_relay_servers,
    });

    axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .route("/networks", axum::routing::post(api::create_network))
        .route(
            "/networks/resolve",
            axum::routing::post(api::resolve_network),
        )
        .route("/networks/:id/join", axum::routing::post(api::join_network))
        .route(
            "/networks/:id/members",
            axum::routing::get(api::list_members),
        )
        .route(
            "/networks/:id/candidates",
            axum::routing::post(api::update_candidates),
        )
        .route("/directory", axum::routing::get(api::directory))
        .route("/telemetry", axum::routing::post(api::record_telemetry))
        .route("/metrics", axum::routing::get(api::metrics))
        .with_state(state)
}
