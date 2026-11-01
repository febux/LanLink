//! `lanlinkd up` — one-command daemon startup.
//!
//! Resolves (or creates) a network by human-readable name via the coordinator's
//! `POST /networks/resolve` endpoint, then builds a [`RunConfig`] in memory and
//! runs the daemon. No config file or numeric network id is required.

use std::net::SocketAddr;

use crate::config::{CliOverrides, RunConfig};

/// Resolve a network by name and build a runtime config for `lanlinkd run`.
///
/// `password` comes from the `--password` flag or the `LANLINK_PASSWORD` env
/// var. `--coordinator` defaults to `http://localhost:8080`.
#[allow(clippy::too_many_arguments)]
pub async fn resolve_config(
    coordinator: String,
    name: String,
    password: String,
    is_public: bool,
    bind: Option<SocketAddr>,
    tap: Option<String>,
    mtu: Option<usize>,
    status_socket: Option<String>,
) -> anyhow::Result<RunConfig> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build http client: {e}"))?;

    let req = lanlink_proto::ResolveNetworkRequest {
        name,
        password: password.into(),
        is_public,
    };
    let resp: lanlink_proto::ResolveNetworkResponse = client
        .post(format!(
            "{}/networks/resolve",
            coordinator.trim_end_matches('/')
        ))
        .json(&req)
        .send()
        .await?
        .error_for_status()
        .map_err(|e| anyhow::anyhow!("failed to resolve network: {e}"))?
        .json()
        .await?;

    tracing::info!(
        network_id = resp.network_id.0,
        subnet = %resp.subnet,
        created = resp.created,
        "resolved network via coordinator"
    );

    let file = crate::config::FileConfig::default();
    let cli = CliOverrides {
        coordinator: Some(coordinator),
        network_id: Some(resp.network_id.0),
        password: Some(req.password.into_string()),
        bind,
        tap,
        mtu,
        stun_servers: None,
        gateway: None,
        shared_subnet: None,
        phys_interface: None,
        status_socket,
    };

    crate::config::resolve(&file, &cli).map_err(|e| anyhow::anyhow!("configuration error: {e}"))
}
