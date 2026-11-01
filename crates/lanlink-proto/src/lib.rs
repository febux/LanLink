//! Common protocol types and wire format definitions for LanLink.
//!
//! This crate is meant to be language-agnostic: the data-plane frame layout,
//! control-plane API messages, and versioning constants live here.

pub mod control;
pub mod status;
pub mod telemetry;
pub mod update;

/// Current protocol version.
pub const PROTOCOL_VERSION: u32 = 1;

/// Network identifier (stable, coordinator-assigned).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NetworkId(pub u64);

use lanlink_types::{NodeId, VirtualIp};

/// Newtype wrapper for a secret string (e.g. a network password).
///
/// `Debug` and `Display` print a redacted placeholder to avoid leaking
/// secrets into logs or panic messages. Serialization/deserialization
/// pass through to the underlying string for wire compatibility.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    /// Wrap an existing string.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Borrow the secret as a plain string.
    ///
    /// Only use this when the value is needed for computation; never log it.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the wrapper and return the underlying string.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl std::fmt::Display for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl Default for SecretString {
    fn default() -> Self {
        Self(String::new())
    }
}

impl From<String> for SecretString {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for SecretString {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<SecretString> for String {
    fn from(s: SecretString) -> Self {
        s.0
    }
}

impl AsRef<str> for SecretString {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for SecretString {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

/// Control-plane request: create a new network.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateNetworkRequest {
    pub name: String,
    pub password: SecretString,
    /// Mark the network as public/gaming; inbound firewall drops dangerous
    /// ports (SMB, RDP, SSH, etc.) by default.
    #[serde(default)]
    pub is_public: bool,
}

/// Response after creating a network.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateNetworkResponse {
    pub network_id: NetworkId,
    pub subnet: String,
    pub is_public: bool,
}

/// Idempotent request: resolve an existing network by name+password, or
/// create it if it does not yet exist. Used by `lanlinkd up` so users can
/// start a daemon with just a network name instead of a numeric id.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ResolveNetworkRequest {
    pub name: String,
    pub password: SecretString,
    #[serde(default)]
    pub is_public: bool,
}

/// Response for [`ResolveNetworkRequest`].
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ResolveNetworkResponse {
    pub network_id: NetworkId,
    pub subnet: String,
    pub is_public: bool,
    /// True when the coordinator had to create the network; false when it
    /// already existed and the password matched.
    pub created: bool,
}

/// Request to join an existing network.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct JoinNetworkRequest {
    pub password: SecretString,
    pub node_id: NodeId,
    pub public_key: Vec<u8>,
    pub direct_candidates: Vec<std::net::SocketAddr>,
    pub relay_candidates: Vec<std::net::SocketAddr>,
    /// TCP candidates for direct TCP fallback when UDP hole punching fails.
    #[serde(default)]
    pub tcp_candidates: Vec<std::net::SocketAddr>,
    /// TCP relay candidates for environments where UDP is blocked.
    #[serde(default)]
    pub tcp_relay_candidates: Vec<std::net::SocketAddr>,
    /// Advertise this node as a gateway sharing its physical LAN subnet (e.g.
    /// "192.168.1.0/24"). Empty means no gateway role.
    #[serde(default)]
    pub shared_subnet: String,
}

/// Control-plane response after joining a network.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JoinNetworkResponse {
    pub network_id: NetworkId,
    pub assigned_ip: VirtualIp,
    pub peers: Vec<PeerInfo>,
    /// Relay servers that this network may use for fallback traffic. The daemon
    /// binds to the first reachable one and advertises it as a relay candidate.
    pub relay_servers: Vec<std::net::SocketAddr>,
    /// TCP relay servers for environments where UDP is blocked.
    #[serde(default)]
    pub tcp_relay_servers: Vec<std::net::SocketAddr>,
    /// Whether the network is flagged public/gaming, which enables the inbound
    /// firewall profile by default.
    pub is_public: bool,
    /// Virtual subnet assigned by the coordinator, in CIDR notation (e.g.
    /// "26.1.2.0/24`). The daemon uses this instead of hard-coding a /24.
    #[serde(default)]
    pub subnet: String,
}

/// Minimal peer metadata broadcast by the coordinator.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PeerInfo {
    pub node_id: NodeId,
    pub virtual_ip: VirtualIp,
    pub public_key: Vec<u8>,
    pub direct_candidates: Vec<std::net::SocketAddr>,
    /// Relay server addresses this peer is reachable through. Traffic to these
    /// addresses is still E2E-encrypted; the relay only reads the outer receiver
    /// id to route the packet.
    pub relay_candidates: Vec<std::net::SocketAddr>,
    /// TCP candidates for direct TCP fallback when UDP hole punching fails.
    #[serde(default)]
    pub tcp_candidates: Vec<std::net::SocketAddr>,
    /// TCP relay candidates for environments where UDP is blocked.
    #[serde(default)]
    pub tcp_relay_candidates: Vec<std::net::SocketAddr>,
    /// True if this peer advertises itself as a gateway for its physical LAN.
    #[serde(default)]
    pub is_gateway: bool,
    /// Subnet (e.g. "192.168.1.0/24") this gateway shares with the virtual LAN.
    #[serde(default)]
    pub shared_subnet: String,
}

/// Public directory entry for a network that opted into discovery.
/// Passwords are never exposed; this is purely a lobby-style listing.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PublicNetworkInfo {
    pub network_id: NetworkId,
    pub name: String,
    pub subnet: String,
    /// Approximate number of currently-online members.
    pub member_count: u64,
    /// Whether the network uses the public/gaming firewall profile.
    pub is_public: bool,
}

/// Event pushed by the coordinator over a WebSocket/control channel.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum MemberEvent {
    Joined { peer: PeerInfo },
    Left { node_id: NodeId },
    AddressChange { peer: PeerInfo },
    Keepalive,
}

/// Daemon-to-coordinator keepalive / address update.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PresenceUpdate {
    pub node_id: NodeId,
    pub direct_candidates: Vec<std::net::SocketAddr>,
    /// Relay candidates for this node. Each address is a shared relay server
    /// that the node has bound to; peers send normal LanLink packets there and
    /// the relay forwards them based on the plaintext receiver id.
    pub relay_candidates: Vec<std::net::SocketAddr>,
    /// TCP candidates for direct TCP fallback when UDP hole punching fails.
    #[serde(default)]
    pub tcp_candidates: Vec<std::net::SocketAddr>,
    /// TCP relay candidates for environments where UDP is blocked.
    #[serde(default)]
    pub tcp_relay_candidates: Vec<std::net::SocketAddr>,
}

/// Relay bind message sent to a LanLink relay server over UDP.
/// The relay learns the sender's public UDP endpoint from the packet source
/// address and associates it with `node_id` so it can forward traffic.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RelayBind {
    pub node_id: NodeId,
}

/// Daemon-to-coordinator notification that this node is leaving the network.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LeaveNetworkRequest {
    pub password: SecretString,
    pub node_id: NodeId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_string_debug_display_redacted() {
        let s = SecretString::new("hunter2");
        assert_eq!(format!("{:?}", s), "[REDACTED]");
        assert_eq!(format!("{}", s), "[REDACTED]");
    }

    #[test]
    fn secret_string_serializes_as_plain_string() {
        let s = SecretString::new("secret");
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"secret\"");
        let back: SecretString = serde_json::from_str(&json).unwrap();
        assert_eq!(back.as_str(), "secret");
    }

    #[test]
    fn secret_string_deserializes_from_string() {
        let s: SecretString = serde_json::from_str("\"plain\"").unwrap();
        assert_eq!(s.as_str(), "plain");
    }
}
