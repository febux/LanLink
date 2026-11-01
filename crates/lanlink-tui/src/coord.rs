//! Coordinator REST client: mirrors the operations `lanlinkctl` performs
//! (`coord create`, `coord join`, `directory list`) using the shared
//! `lanlink-proto` request/response types.

use lanlink_proto::control::ControlRequest;
use lanlink_proto::{CreateNetworkRequest, CreateNetworkResponse, JoinNetworkRequest, JoinNetworkResponse, PublicNetworkInfo};
use rand::RngCore;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::sync::mpsc;

use crate::app::Event;

/// A control-plane operation the UI wants performed (off the UI thread).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    CreateNetwork {
        name: String,
        password: String,
        public: bool,
    },
    JoinNetwork {
        network_id: u64,
        password: String,
    },
    /// Disconnect the running daemon from its current network.
    DaemonDisconnect,
    /// Gracefully stop the running daemon.
    DaemonStop,
}

/// Result of a successful action.
#[derive(Debug)]
pub enum CoordOutcome {
    Created(CreateNetworkResponse),
    Joined(JoinNetworkResponse),
    Ack,
}

/// Spawn a task that runs one action and feeds the result back into the
/// event loop. Keeps the reducer pure: it only produces [`Action`]s.
pub fn dispatch(action: Action, base: String, tx: mpsc::Sender<Event>) {
    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("failed to build http client");
        let result = run_action(&client, &base, &action)
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(Event::Coord(action, result)).await;
    });
}

/// List public networks from the coordinator directory.
pub async fn list_directory(
    client: &reqwest::Client,
    base: &str,
) -> anyhow::Result<Vec<PublicNetworkInfo>> {
    let networks: Vec<PublicNetworkInfo> = client
        .get(format!("{}/directory", base))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(networks)
}

/// Execute one action against the coordinator.
pub async fn run_action(
    client: &reqwest::Client,
    base: &str,
    action: &Action,
) -> anyhow::Result<CoordOutcome> {
    match action {
        Action::CreateNetwork {
            name,
            password,
            public,
        } => {
            let req = CreateNetworkRequest {
                name: name.clone(),
                password: password.clone().into(),
                is_public: *public,
            };
            let resp: CreateNetworkResponse = client
                .post(format!("{}/networks", base))
                .json(&req)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            Ok(CoordOutcome::Created(resp))
        }
        Action::JoinNetwork {
            network_id,
            password,
        } => {
            // Same semantics as `lanlinkctl coord join`: a coordinator-level
            // join that returns the assigned IP and current peer list. The
            // data plane is run by `lanlinkd`, not by the TUI.
            let mut node_id = [0u8; 8];
            rand::thread_rng().fill_bytes(&mut node_id);
            let candidate: SocketAddr = "127.0.0.1:9000".parse().expect("static addr parses");
            let req = JoinNetworkRequest {
                password: password.clone().into(),
                node_id: lanlink_types::NodeId(node_id),
                public_key: vec![],
                direct_candidates: vec![candidate],
                relay_candidates: vec![],
                tcp_candidates: vec![],
                tcp_relay_candidates: vec![],
                shared_subnet: String::new(),
            };
            let resp: JoinNetworkResponse = client
                .post(format!("{}/networks/{}/join", base, network_id))
                .json(&req)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            Ok(CoordOutcome::Joined(resp))
        }
        Action::DaemonDisconnect => {
            // The control socket path is resolved by the caller (daemon::dispatch).
            anyhow::bail!("DaemonDisconnect must be handled by daemon::dispatch")
        }
        Action::DaemonStop => {
            anyhow::bail!("DaemonStop must be handled by daemon::dispatch")
        }
    }
}

/// Spawn a daemon control command and report the result back to the UI loop.
/// Kept here so `coord::Action` remains the single UI action type.
pub fn dispatch_daemon(action: Action, socket: PathBuf, tx: mpsc::Sender<Event>) {
    tokio::spawn(async move {
        let result = match action {
            Action::DaemonDisconnect => {
                crate::daemon::send_command(&socket, ControlRequest::Disconnect)
                    .await
                    .map(|_| CoordOutcome::Ack)
            }
            Action::DaemonStop => crate::daemon::send_command(&socket, ControlRequest::Stop)
                .await
                .map(|_| CoordOutcome::Ack),
            _ => Err(anyhow::anyhow!(
                "non-daemon action passed to dispatch_daemon"
            )),
        }
        .map_err(|e| e.to_string());
        let _ = tx.send(Event::Coord(action, result)).await;
    });
}
