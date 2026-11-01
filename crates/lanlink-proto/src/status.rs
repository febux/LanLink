//! Local status API types shared between the daemon (`lanlinkd`) and
//! frontends such as `lanlink-tui`.
//!
//! The daemon serves these over a local Unix domain socket (named pipe on
//! Windows) using newline-delimited JSON: the client writes one
//! [`StatusRequest`] per line and the daemon answers each line with one
//! JSON-serialized [`StatusResponse`]. The protocol is read-only; it exposes
//! no secrets (no passwords or key material).

use std::net::SocketAddr;

use crate::NetworkId;
use lanlink_types::{NodeId, VirtualIp};

/// Request a status client can send to the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum StatusRequest {
    /// Full snapshot: daemon info, peer table, and traffic counters.
    Snapshot,
}

/// How the daemon currently reaches a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathKind {
    /// Verified direct UDP path (hole punch succeeded or direct candidate).
    Direct,
    /// Traffic flows through a UDP relay server (still E2E-encrypted).
    Relay,
    /// Verified direct TCP path.
    TcpDirect,
    /// Traffic flows through a TCP relay server.
    TcpRelay,
    /// Hole punching is in progress.
    Punching,
    /// No usable address yet.
    Waiting,
}

impl std::fmt::Display for PathKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathKind::Direct => write!(f, "direct"),
            PathKind::Relay => write!(f, "relay"),
            PathKind::TcpDirect => write!(f, "tcp_direct"),
            PathKind::TcpRelay => write!(f, "tcp_relay"),
            PathKind::Punching => write!(f, "punching"),
            PathKind::Waiting => write!(f, "waiting"),
        }
    }
}

/// Daemon-level status of one joined network.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DaemonStatus {
    /// Daemon build version.
    pub version: String,
    /// Operating-system process id of the daemon (useful for `daemon stop`).
    pub pid: u32,
    pub network_id: NetworkId,
    pub assigned_ip: VirtualIp,
    /// Virtual NIC (TAP/Wintun/utun) interface name.
    pub tap_name: String,
    /// UDP bind address of the data-plane socket.
    pub bind: SocketAddr,
    /// Relay server this daemon bound to, if any.
    pub relay_addr: Option<SocketAddr>,
    /// Gateway mode (sharing a physical LAN subnet) is active.
    pub gateway_enabled: bool,
    /// Per-network inbound firewall profile is active (public network).
    pub firewall_enabled: bool,
    /// Seconds since the daemon joined the network.
    pub uptime_seconds: u64,
    /// Total transmitted bytes on the data plane.
    pub tx_bytes: u64,
    /// Total received bytes on the data plane.
    pub rx_bytes: u64,
    /// Whether the daemon currently has an active network session.
    pub connected: bool,
}

/// Status of a single peer as seen by the daemon.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PeerStatus {
    pub node_id: NodeId,
    pub virtual_ip: VirtualIp,
    /// Current UDP endpoint the daemon sends to.
    pub endpoint: Option<SocketAddr>,
    pub path: PathKind,
    /// Last measured round-trip time of the latency probe, if any.
    pub latency_ms: Option<u64>,
    /// Seconds since the last authenticated packet from this peer.
    pub last_seen_secs_ago: Option<u64>,
    /// Bytes transmitted to this peer (data plane).
    pub tx_bytes: u64,
    /// Bytes received from this peer (data plane).
    pub rx_bytes: u64,
    /// Peer advertises itself as a gateway for its physical LAN.
    pub is_gateway: bool,
    /// Subnet shared by the gateway peer (when `is_gateway`).
    pub shared_subnet: String,
}

/// Complete status snapshot served to frontends.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StatusSnapshot {
    pub status: DaemonStatus,
    pub peers: Vec<PeerStatus>,
}

/// Response envelope for one [`StatusRequest`] line.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", content = "data", rename_all = "snake_case")]
pub enum StatusResponse {
    Snapshot(StatusSnapshot),
    Error { error: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_round_trip() {
        let snap = StatusSnapshot {
            status: DaemonStatus {
                version: "0.1.0".into(),
                pid: std::process::id(),
                network_id: NetworkId(7),
                assigned_ip: VirtualIp("26.0.0.5".parse().unwrap()),
                tap_name: "lanlink0".into(),
                bind: "0.0.0.0:9000".parse().unwrap(),
                relay_addr: Some("203.0.113.1:9001".parse().unwrap()),
                gateway_enabled: false,
                firewall_enabled: true,
                uptime_seconds: 42,
                tx_bytes: 1234,
                rx_bytes: 5678,
                connected: true,
            },
            peers: vec![PeerStatus {
                node_id: NodeId([1; 8]),
                virtual_ip: VirtualIp("26.0.0.6".parse().unwrap()),
                endpoint: Some("198.51.100.2:5000".parse().unwrap()),
                path: PathKind::Direct,
                latency_ms: Some(12),
                last_seen_secs_ago: Some(1),
                tx_bytes: 10,
                rx_bytes: 20,
                is_gateway: true,
                shared_subnet: "192.168.1.0/24".into(),
            }],
        };
        let resp = StatusResponse::Snapshot(snap);
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"latency_ms\":12"));
        let back: StatusResponse = serde_json::from_str(&json).unwrap();
        match back {
            StatusResponse::Snapshot(s) => {
                assert_eq!(s.status.network_id.0, 7);
                assert_eq!(s.peers[0].path, PathKind::Direct);
                assert_eq!(s.peers[0].shared_subnet, "192.168.1.0/24");
            }
            StatusResponse::Error { .. } => panic!("expected snapshot"),
        }
    }

    #[test]
    fn request_parses() {
        let req: StatusRequest = serde_json::from_str("{\"cmd\":\"snapshot\"}").unwrap();
        assert_eq!(req, StatusRequest::Snapshot);
    }

    #[test]
    fn error_round_trip() {
        let resp = StatusResponse::Error {
            error: "boom".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: StatusResponse = serde_json::from_str(&json).unwrap();
        match back {
            StatusResponse::Error { error } => assert_eq!(error, "boom"),
            StatusResponse::Snapshot(_) => panic!("expected error"),
        }
    }
}
