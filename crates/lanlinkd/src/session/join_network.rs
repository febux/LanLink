use std::net::SocketAddr;

use lanlink_crypto::Identity;

/// Join a network through the coordinator and parse the assignment response.
#[allow(clippy::too_many_arguments)]
pub async fn join_network(
    coordinator: &str,
    network_id: u64,
    password: &str,
    bind: SocketAddr,
    tcp_addr: SocketAddr,
    identity: &Identity,
    client: &reqwest::Client,
    shared_subnet: &str,
) -> anyhow::Result<lanlink_proto::JoinNetworkResponse> {
    let initial_candidates = lanlink_punch::gather_host_candidates(bind.port());
    let tcp_candidates = lanlink_punch::gather_host_candidates(tcp_addr.port());

    let join_req = lanlink_proto::JoinNetworkRequest {
        password: password.into(),
        node_id: identity.node_id(),
        public_key: identity.public_key().to_vec(),
        direct_candidates: initial_candidates.iter().map(|c| c.addr).collect(),
        relay_candidates: Vec::new(), // updated after we bind to the network relay
        tcp_candidates: tcp_candidates.iter().map(|c| c.addr).collect(),
        tcp_relay_candidates: Vec::new(),
        shared_subnet: shared_subnet.to_string(),
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
        subnet = %resp.subnet,
        peer_count = resp.peers.len(),
        relay_servers = ?resp.relay_servers,
        "joined network via coordinator"
    );

    Ok(resp)
}
