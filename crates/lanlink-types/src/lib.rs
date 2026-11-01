//! Base protocol primitives and wire-format constants for LanLink.
//!
//! This crate sits below `lanlink-proto` and `lanlink-crypto`. It contains only
//! the identifiers and data-plane constants that both sides need, so crates
//! lower in the stack (e.g. cryptography) do not have to depend on the full
//! control-plane protocol crate.

/// Data-plane wire format version.
pub const WIRE_PROTOCOL_VERSION: u8 = 1;

/// Data-plane packet types.
pub const DATA_PACKET_TYPE: u8 = 1;
pub const PING_PACKET_TYPE: u8 = 2;
pub const PONG_PACKET_TYPE: u8 = 3;
pub const PUNCH_PROBE_PACKET_TYPE: u8 = 4;
pub const PUNCH_REPLY_PACKET_TYPE: u8 = 5;
pub const KEEPALIVE_PACKET_TYPE: u8 = 6;
/// Noise protocol handshake message.
pub const NOISE_HANDSHAKE_PACKET_TYPE: u8 = 7;

/// Outer header flag: packet payload is encrypted with a Noise transport session
/// rather than the network-wide PSK.
pub const FLAG_NOISE: u8 = 0x01;
/// Outer header flag mask reserved for future use.
pub const FLAG_RESERVED_MASK: u8 = 0xfe;

/// Node identifier length in bytes.
pub const NODE_ID_LEN: usize = 8;

/// Target UDP payload size. We stay well below the IPv4 typical path MTU so that
/// the OS never fragments our outer datagrams; Ethernet frames larger than the
/// inner capacity are split by LanLink's own chunker/reassembler.
pub const MAX_UDP_PAYLOAD: usize = 1300;

/// Maximum size of the AEAD plaintext for a DATA packet given the current
/// 4-byte header + 8-byte receiver id + 12-byte nonce + 16-byte Poly1305 tag.
pub const MAX_INNER_PAYLOAD: usize = MAX_UDP_PAYLOAD - 4 - NODE_ID_LEN - 12 - 16;

/// Inner DATA chunking header laid out by the switch (see `lanlink-switch`).
/// ```text
/// [sender node id: 8 bytes]
/// [counter: 8 bytes BE]
/// [frame id: 4 bytes BE]
/// [chunk index: 2 bytes BE]
/// [total chunks: 2 bytes BE]
/// ```
pub const DATA_CHUNK_HEADER_LEN: usize = NODE_ID_LEN + 8 + 4 + 2 + 2;

/// Maximum raw Ethernet frame payload that fits into a single DATA packet.
pub const MAX_CHUNK_PAYLOAD: usize = MAX_INNER_PAYLOAD - DATA_CHUNK_HEADER_LEN;

/// Ethernet header length for a typical untagged frame.
pub const ETHERNET_HEADER_LEN: usize = 14;

/// Recommended MTU for the virtual TAP interface so that a full Ethernet frame
/// carrying an MTU-sized IPv4 packet fits into one LanLink DATA packet without
/// triggering LanLink-level chunking.
///
/// Computed as `MAX_CHUNK_PAYLOAD - ETHERNET_HEADER_LEN`.
pub const RECOMMENDED_MTU: usize = MAX_CHUNK_PAYLOAD - ETHERNET_HEADER_LEN;

/// Node identifier (Ed25519 public key hash / random stable id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NodeId(pub [u8; NODE_ID_LEN]);

/// A virtual IP address inside a LanLink network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VirtualIp(pub std::net::Ipv4Addr);

impl std::fmt::Display for VirtualIp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Relay keep-alive / bind message wire constants.
pub const RELAY_MAGIC: &[u8] = b"LLR\x01";
pub const RELAY_BIND_PACKET_TYPE: u8 = 1;
pub const RELAY_KEEPALIVE_PACKET_TYPE: u8 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommended_mtu_fits_in_single_inner_packet() {
        // A full Ethernet frame (MTU + 14-byte Ethernet header) plus the switch
        // chunking header must fit in one DATA packet's AEAD plaintext.
        assert_eq!(
            RECOMMENDED_MTU + ETHERNET_HEADER_LEN + DATA_CHUNK_HEADER_LEN,
            MAX_INNER_PAYLOAD
        );
    }
}
