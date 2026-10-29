//! Local control commands sent to the daemon over its local control/status
//! socket.
//!
//! The socket speaks newline-delimited JSON. Clients write one
//! [`ControlRequest`] per line; the daemon answers each line with one JSON
//! [`ControlResponse`]. If a line does not parse as a control request, the
//! daemon tries to parse it as a read-only [`crate::status::StatusRequest`].
//!
//! These commands are intentionally local-only: they rely on OS access control
//! (Unix domain socket permissions / TCP loopback binding) rather than tokens.

use crate::status::StatusSnapshot;

/// A control request from a local frontend to the daemon.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ControlRequest {
    /// Read-only: return the current status snapshot.
    Status,
    /// Join a network described by an in-memory runtime config.
    Connect {
        /// Coordinator base URL.
        coordinator: String,
        /// Network id to join.
        network_id: u64,
        /// Network password.
        password: String,
        /// UDP bind address.
        #[serde(default = "default_bind")]
        bind: std::net::SocketAddr,
        /// TAP/virtual NIC name.
        #[serde(default = "default_tap")]
        tap: String,
        /// Virtual NIC MTU.
        #[serde(default = "default_mtu")]
        mtu: usize,
        /// Optional STUN servers.
        #[serde(default)]
        stun_servers: Vec<String>,
        /// Enable gateway mode.
        #[serde(default)]
        gateway: bool,
        /// Physical subnet to share when gateway mode is enabled.
        #[serde(default)]
        shared_subnet: String,
        /// Physical interface to MASQUERADE to when gateway mode is enabled.
        #[serde(default)]
        phys_interface: String,
    },
    /// Leave the current network and release resources, but keep the daemon
    /// process running. Also accepts the `leave_network` command name for
    /// frontend compatibility.
    #[serde(alias = "leave_network")]
    Disconnect,
    /// Gracefully shut down the daemon.
    Stop,
}

fn default_bind() -> std::net::SocketAddr {
    std::net::SocketAddr::from(([0, 0, 0, 0], 9000))
}

fn default_tap() -> String {
    "lanlink0".to_string()
}

fn default_mtu() -> usize {
    1400
}

/// Response envelope for one [`ControlRequest`] line.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ControlResponse {
    /// Command succeeded. For `status` the snapshot is here.
    Ok { result: ControlResult },
    /// Command failed.
    Error { error: String },
}

/// Successful payload for a [`ControlResponse`].
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlResult {
    /// Result of a `status` request.
    Status(StatusSnapshot),
    /// Generic success for commands that have no other data.
    Ack,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_request_round_trip() {
        let req = ControlRequest::Connect {
            coordinator: "http://localhost:8080".into(),
            network_id: 7,
            password: "hunter2".into(),
            bind: default_bind(),
            tap: default_tap(),
            mtu: default_mtu(),
            stun_servers: vec![],
            gateway: false,
            shared_subnet: String::new(),
            phys_interface: String::new(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: ControlRequest = serde_json::from_str(&json).unwrap();
        match back {
            ControlRequest::Connect { network_id, .. } => assert_eq!(network_id, 7),
            _ => panic!("expected connect"),
        }
    }

    #[test]
    fn disconnect_parses() {
        let req: ControlRequest = serde_json::from_str("{\"cmd\":\"disconnect\"}").unwrap();
        assert!(matches!(req, ControlRequest::Disconnect));
    }

    #[test]
    fn leave_network_alias_parses() {
        let req: ControlRequest = serde_json::from_str("{\"cmd\":\"leave_network\"}").unwrap();
        assert!(matches!(req, ControlRequest::Disconnect));
    }

    #[test]
    fn stop_parses() {
        let req: ControlRequest = serde_json::from_str("{\"cmd\":\"stop\"}").unwrap();
        assert!(matches!(req, ControlRequest::Stop));
    }

    #[test]
    fn ack_response_round_trip() {
        let resp = ControlResponse::Ok {
            result: ControlResult::Ack,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: ControlResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, ControlResponse::Ok { .. }));
    }
}
