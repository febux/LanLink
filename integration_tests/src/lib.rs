//! Shared helpers for the LanLink workspace integration-test crate.
//!
//! These utilities are intentionally small and deterministic: they give each
//! test stable node IDs, network keys, and short timeouts so the rootless
//! integration suite is fast and repeatable.

use std::net::SocketAddr;
use std::time::Duration;

/// Default timeout used by async integration tests.
pub const TEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Build a deterministic 8-byte [`lanlink_types::NodeId`] from a small seed.
///
/// Seed 0 is reserved for the local node in multi-peer tests; use seeds >= 1
/// for distinct peers. The public key bytes are predictable, so tests that
/// need Ed25519 identities should use [`node_identity`] instead.
pub fn node_id(seed: u8) -> lanlink_types::NodeId {
    let mut bytes = [0u8; lanlink_types::NODE_ID_LEN];
    bytes[lanlink_types::NODE_ID_LEN - 1] = seed;
    lanlink_types::NodeId(bytes)
}

/// Build a deterministic [`lanlink_crypto::Identity`] for tests.
///
/// The secret key is derived from `seed` so the same seed always produces the
/// same node id and public key. This makes peer public keys predictable, which
/// is required for Noise handshake tests.
pub fn node_identity(seed: u8) -> lanlink_crypto::Identity {
    let mut secret = [0u8; 32];
    secret[31] = seed;
    lanlink_crypto::Identity::from_secret_bytes(secret)
}

/// Derive a network PSK from a deterministic test password.
pub fn network_psk(network_id: u64) -> [u8; 32] {
    lanlink_crypto::derive_network_psk("integration-test-password", network_id)
        .expect("test PSK derivation should succeed")
}

/// Parse a local loopback address for tests.
pub fn localhost(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

/// Build a minimal Ethernet frame with the given source and destination MACs.
/// The ethertype is IPv4 (0x0800) and the payload follows.
pub fn eth_frame(src_mac: [u8; 6], dst_mac: [u8; 6], payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(14 + payload.len());
    frame.extend_from_slice(&dst_mac);
    frame.extend_from_slice(&src_mac);
    frame.extend_from_slice(&[0x08, 0x00]); // IPv4
    frame.extend_from_slice(payload);
    frame
}

/// Wait for a TCP port to become reachable, polling every 100 ms.
pub async fn wait_for_tcp(addr: SocketAddr) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + TEST_TIMEOUT;
    loop {
        match tokio::net::TcpStream::connect(addr).await {
            Ok(_) => return Ok(()),
            Err(e) if tokio::time::Instant::now() >= deadline => {
                return Err(anyhow::anyhow!("port {addr} did not open: {e}"));
            }
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}
