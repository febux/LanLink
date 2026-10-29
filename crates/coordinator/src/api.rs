use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;

pub struct AppState {
    pub pool: sqlx::SqlitePool,
    /// Relay server addresses advertised to every network created on this coordinator.
    pub relay_servers: Vec<std::net::SocketAddr>,
    /// TCP relay server addresses advertised to every network created on this coordinator.
    pub tcp_relay_servers: Vec<std::net::SocketAddr>,
}

pub async fn create_network(
    State(state): State<Arc<AppState>>,
    Json(req): Json<lanlink_proto::CreateNetworkRequest>,
) -> impl IntoResponse {
    let hash = match crate::db::hash_password(&req.password) {
        Ok(h) => h,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "password hashing failed").into_response()
        }
    };

    let relay_servers_json = match serde_json::to_string(&state.relay_servers) {
        Ok(s) => s,
        Err(_) => "[]".to_string(),
    };
    let tcp_relay_servers_json = match serde_json::to_string(&state.tcp_relay_servers) {
        Ok(s) => s,
        Err(_) => "[]".to_string(),
    };

    match crate::db::create_network(
        &state.pool,
        &req.name,
        &hash,
        &relay_servers_json,
        &tcp_relay_servers_json,
        req.is_public,
    )
    .await
    {
        Ok(id) => {
            let resp = lanlink_proto::CreateNetworkResponse {
                network_id: lanlink_proto::NetworkId(id as u64),
                subnet: format!("26.{}.{}.0/24", (id >> 8) & 0xff, id & 0xff),
                is_public: req.is_public,
            };
            (StatusCode::CREATED, Json(resp)).into_response()
        }
        Err(e) => {
            tracing::warn!(error = %e, "create network failed");
            (StatusCode::CONFLICT, "network name may already exist").into_response()
        }
    }
}

pub async fn join_network(
    State(state): State<Arc<AppState>>,
    Path(network_id): Path<u64>,
    Json(req): Json<lanlink_proto::JoinNetworkRequest>,
) -> impl IntoResponse {
    if !crate::db::network_exists(&state.pool, network_id as i64)
        .await
        .unwrap_or(false)
    {
        return (StatusCode::NOT_FOUND, "network not found").into_response();
    }

    let hash = match crate::db::get_network_password_hash(&state.pool, network_id as i64).await {
        Ok(Some(h)) => h,
        Ok(None) => return (StatusCode::NOT_FOUND, "network not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response(),
    };

    if !crate::db::verify_password(&req.password, &hash) {
        return (StatusCode::UNAUTHORIZED, "invalid password").into_response();
    }

    let direct_candidates_json = match serde_json::to_string(&req.direct_candidates) {
        Ok(s) => s,
        Err(_) => "[]".to_string(),
    };
    let relay_candidates_json = match serde_json::to_string(&req.relay_candidates) {
        Ok(s) => s,
        Err(_) => "[]".to_string(),
    };
    let tcp_candidates_json = match serde_json::to_string(&req.tcp_candidates) {
        Ok(s) => s,
        Err(_) => "[]".to_string(),
    };
    let tcp_relay_candidates_json = match serde_json::to_string(&req.tcp_relay_candidates) {
        Ok(s) => s,
        Err(_) => "[]".to_string(),
    };
    use base64::Engine;
    let public_key = base64::engine::general_purpose::STANDARD.encode(&req.public_key);

    let is_gateway = !req.shared_subnet.is_empty();
    let assigned_ip = match crate::db::upsert_member(
        &state.pool,
        network_id as i64,
        &req.node_id.0,
        &public_key,
        &direct_candidates_json,
        &relay_candidates_json,
        &tcp_candidates_json,
        &tcp_relay_candidates_json,
        is_gateway,
        &req.shared_subnet,
    )
    .await
    {
        Ok(ip) => ip,
        Err(e) => {
            tracing::warn!(error = %e, "upsert member failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "failed to assign IP").into_response();
        }
    };

    let peers = match crate::db::list_peers(&state.pool, network_id as i64, &req.node_id.0).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "list peers failed");
            Vec::new()
        }
    };

    let relay_servers =
        match crate::db::get_network_relay_servers(&state.pool, network_id as i64).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "failed to load relay servers");
                Vec::new()
            }
        };
    let tcp_relay_servers =
        match crate::db::get_network_tcp_relay_servers(&state.pool, network_id as i64).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "failed to load TCP relay servers");
                Vec::new()
            }
        };

    let is_public = crate::db::get_network_is_public(&state.pool, network_id as i64)
        .await
        .unwrap_or(false);

    let resp = lanlink_proto::JoinNetworkResponse {
        network_id: lanlink_proto::NetworkId(network_id),
        assigned_ip: lanlink_proto::VirtualIp(assigned_ip),
        peers,
        relay_servers,
        tcp_relay_servers,
        is_public,
    };

    (StatusCode::OK, Json(resp)).into_response()
}

pub async fn leave_network(
    State(state): State<Arc<AppState>>,
    Path(network_id): Path<u64>,
    Json(req): Json<lanlink_proto::LeaveNetworkRequest>,
) -> impl IntoResponse {
    if !crate::db::network_exists(&state.pool, network_id as i64)
        .await
        .unwrap_or(false)
    {
        return (StatusCode::NOT_FOUND, "network not found").into_response();
    }

    let hash = match crate::db::get_network_password_hash(&state.pool,
        network_id as i64,
    )
    .await
    {
        Ok(Some(h)) => h,
        Ok(None) => return (StatusCode::NOT_FOUND, "network not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response(),
    };

    if !crate::db::verify_password(&req.password, &hash) {
        return (StatusCode::UNAUTHORIZED, "invalid password").into_response();
    }

    match crate::db::delete_member(&state.pool, network_id as i64, &req.node_id.0).await {
        Ok(true) => (StatusCode::NO_CONTENT, "")
            .into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "node not a member of this network").into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "leave network failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "failed to remove member").into_response()
        }
    }
}

pub async fn list_members(
    State(state): State<Arc<AppState>>,
    Path(network_id): Path<u64>,
) -> impl IntoResponse {
    // For M3 this endpoint is intentionally unauthenticated; it only returns virtual IPs
    // and public candidates for members of the network. A production deployment should
    // require the same auth token/password used at join time.
    let peers = match crate::db::list_peers(&state.pool, network_id as i64, &[0u8; 8]).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "list members failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
    };

    Json(peers).into_response()
}

/// Resolve or create a network by name. If the network already exists and the
/// password matches, return its current info. If it does not exist, create
/// it. This lets `lanlinkd up` start from a human-readable name rather than a
/// numeric network id.
pub async fn resolve_network(
    State(state): State<Arc<AppState>>,
    Json(req): Json<lanlink_proto::ResolveNetworkRequest>,
) -> impl IntoResponse {
    match crate::db::get_network_by_name(&state.pool, &req.name).await {
        Ok(Some((id, subnet, hash, is_public))) => {
            if !crate::db::verify_password(&req.password, &hash) {
                return (StatusCode::UNAUTHORIZED, "invalid password").into_response();
            }
            let resp = lanlink_proto::ResolveNetworkResponse {
                network_id: lanlink_proto::NetworkId(id as u64),
                subnet,
                is_public: is_public != 0,
                created: false,
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Ok(None) => {
            let hash = match crate::db::hash_password(&req.password) {
                Ok(h) => h,
                Err(_) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, "password hashing failed")
                        .into_response()
                }
            };
            let relay_servers_json = match serde_json::to_string(&state.relay_servers) {
                Ok(s) => s,
                Err(_) => "[]".to_string(),
            };
            let tcp_relay_servers_json = match serde_json::to_string(&state.tcp_relay_servers) {
                Ok(s) => s,
                Err(_) => "[]".to_string(),
            };
            match crate::db::create_network(
                &state.pool,
                &req.name,
                &hash,
                &relay_servers_json,
                &tcp_relay_servers_json,
                req.is_public,
            )
            .await
            {
                Ok(id) => {
                    let resp = lanlink_proto::ResolveNetworkResponse {
                        network_id: lanlink_proto::NetworkId(id as u64),
                        subnet: format!("26.{}.{}.0/24", (id >> 8) & 0xff, id & 0xff),
                        is_public: req.is_public,
                        created: true,
                    };
                    (StatusCode::CREATED, Json(resp)).into_response()
                }
                Err(e) => {
                    tracing::warn!(error = %e, "resolve create network failed");
                    (StatusCode::CONFLICT, "network name may already exist").into_response()
                }
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "resolve network lookup failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

pub async fn directory(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match crate::db::list_public_networks(&state.pool).await {
        Ok(networks) => Json(networks).into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "directory listing failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

pub async fn record_telemetry(
    State(state): State<Arc<AppState>>,
    Json(req): Json<lanlink_proto::telemetry::TelemetryReport>,
) -> impl IntoResponse {
    if let Err(e) = crate::db::record_telemetry(&state.pool, &req).await {
        tracing::warn!(error = %e, "telemetry recording failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
    }
    (StatusCode::CREATED, "ok").into_response()
}

pub async fn metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match crate::db::aggregate_metrics(
        &state.pool,
        lanlink_proto::telemetry::DEFAULT_METRICS_WINDOW_SECS,
    )
    .await
    {
        Ok(metrics) => Json(metrics).into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "metrics aggregation failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

pub async fn update_candidates(
    State(state): State<Arc<AppState>>,
    Path(network_id): Path<u64>,
    Json(req): Json<lanlink_proto::PresenceUpdate>,
) -> impl IntoResponse {
    if !crate::db::network_exists(&state.pool, network_id as i64)
        .await
        .unwrap_or(false)
    {
        return (StatusCode::NOT_FOUND, "network not found").into_response();
    }

    let direct_candidates_json = match serde_json::to_string(&req.direct_candidates) {
        Ok(s) => s,
        Err(_) => "[]".to_string(),
    };
    let relay_candidates_json = match serde_json::to_string(&req.relay_candidates) {
        Ok(s) => s,
        Err(_) => "[]".to_string(),
    };
    let tcp_candidates_json = match serde_json::to_string(&req.tcp_candidates) {
        Ok(s) => s,
        Err(_) => "[]".to_string(),
    };
    let tcp_relay_candidates_json = match serde_json::to_string(&req.tcp_relay_candidates) {
        Ok(s) => s,
        Err(_) => "[]".to_string(),
    };

    if let Err(e) = crate::db::update_candidates(
        &state.pool,
        network_id as i64,
        &req.node_id.0,
        &direct_candidates_json,
        &relay_candidates_json,
        &tcp_candidates_json,
        &tcp_relay_candidates_json,
    )
    .await
    {
        tracing::warn!(error = %e, "update candidates failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
    }

    let peers = match crate::db::list_peers(&state.pool, network_id as i64, &req.node_id.0).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "list peers failed");
            Vec::new()
        }
    };

    Json(peers).into_response()
}
