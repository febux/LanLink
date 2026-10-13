//! Common protocol types and wire format definitions for LanLink.
//!
//! This crate is meant to be language-agnostic: the data-plane frame layout,
//! control-plane API messages, and versioning constants live here.

pub mod status;
pub mod telemetry;
pub mod update;

/// Current protocol version.
pub const PROTOCOL_VERSION: u32 = 1;

/// Data-plane wire format version.
pub const WIRE_PROTOCOL_VERSION: u8 = 1;

/// Data-plane packet types.
pub const DATA_PACKET_TYPE: u8 = 1;
pub const PING_PACKET_TYPE: u8 = 2;
pub const PONG_PACKET_TYPE: u8 = 3;
pub const PUNCH_PROBE_PACKET_TYPE: u8 = 4;
pub const PUNCH_REPLY_PACKET_TYPE: u8 = 5;
pub const KEEPALIVE_PACKET_TYPE: u8 = 6;

/// Node identifier length in bytes.
pub const NODE_ID_LEN: usize = 8;

/// Target UDP payload size. We stay well below the IPv4 typical path MTU so that
/// the OS never fragments our outer datagrams; Ethernet frames larger than the
/// inner capacity are split by LanLink's own chunker/reassembler.
pub const MAX_UDP_PAYLOAD: usize = 1300;

/// Maximum size of the AEAD plaintext for a DATA packet given the current
/// 4-byte header + 8-byte receiver id + 12-byte nonce + 16-byte Poly1305 tag.
pub const MAX_INNER_PAYLOAD: usize = MAX_UDP_PAYLOAD - 4 - NODE_ID_LEN - 12 - 16;

/// Network identifier (stable, coordinator-assigned).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NetworkId(pub u64);

/// Node identifier (Ed25519 public key hash / random stable id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NodeId(pub [u8; 8]);

/// A virtual IP address inside a LanLink network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VirtualIp(pub std::net::Ipv4Addr);

impl std::fmt::Display for VirtualIp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Control-plane request: create a new network.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateNetworkRequest {
    pub name: String,
    pub password: String,
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
    pub password: String,
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
    pub password: String,
    pub node_id: NodeId,
    pub public_key: Vec<u8>,
    pub direct_candidates: Vec<std::net::SocketAddr>,
    pub relay_candidates: Vec<std::net::SocketAddr>,
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
    /// Whether the network is flagged public/gaming, which enables the inbound
    /// firewall profile by default.
    pub is_public: bool,
}

/// Minimal peer metadata broadcast by the coordinator.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PeerInfo {
    pub node_id: NodeId,
    pub virtual_ip: VirtualIp,
    pub direct_candidates: Vec<std::net::SocketAddr>,
    /// Relay server addresses this peer is reachable through. Traffic to these
    /// addresses is still E2E-encrypted; the relay only reads the outer receiver
    /// id to route the packet.
    pub relay_candidates: Vec<std::net::SocketAddr>,
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
}

/// Relay bind message sent to a LanLink relay server over UDP.
/// The relay learns the sender's public UDP endpoint from the packet source
/// address and associates it with `node_id` so it can forward traffic.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RelayBind {
    pub node_id: NodeId,
}

/// Relay keep-alive / bind message wire constants.
pub const RELAY_MAGIC: &[u8] = b"LLR\x01";
pub const RELAY_BIND_PACKET_TYPE: u8 = 1;
pub const RELAY_KEEPALIVE_PACKET_TYPE: u8 = 2;
