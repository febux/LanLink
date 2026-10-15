//! UDP relay for LanLink — content-blind fallback path.
//!
//! The relay server learns each peer's public UDP endpoint from bind/keepalive
//! packets and forwards encrypted LanLink datagrams between peers. Because the
//! relay never decrypts traffic, it is E2E-blind: it only reads the plaintext
//! receiver node id in the LanLink outer header to decide where to forward.
//!
//! Wire formats:
//!
//! Relay control packet (bind / keepalive):
//! ```text
//! [magic: b"LLR\x01" 4 bytes]
//! [type: u8]              1 = bind, 2 = keepalive
//! [node_id: 8 bytes]
//! ```
//!
//! Forwarded LanLink packet (as received from a peer):
//! ```text
//! [LanLink outer header: 4 bytes]
//! [receiver node id: 8 bytes]   <-- read by relay to route
//! [nonce + ciphertext]
//! ```
//!
//! The relay forwards the LanLink packet verbatim to the bound address for the
//! receiver id.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

pub use codec::{decode_control, encode_bind, encode_keepalive, ControlKind, ControlMsg};

pub mod tcp;

pub use tcp::{client as tcp_client, TcpRelayServer};

/// How long a binding remains valid without a keepalive.
const BINDING_LIFETIME: Duration = Duration::from_secs(90);
/// How often to purge stale bindings.
const PRUNE_INTERVAL: Duration = Duration::from_secs(30);
/// Minimum length of a LanLink outer packet so the relay can read the receiver id.
const MIN_LANLINK_LEN: usize = 4 + lanlink_proto::NODE_ID_LEN;

#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("malformed control packet")]
    MalformedControl,
}

/// A content-blind UDP relay server entry.
#[derive(Debug, Clone)]
pub struct Binding {
    pub node_id: lanlink_proto::NodeId,
    pub addr: SocketAddr,
    pub last_seen: Instant,
}

struct RelayState {
    bindings: HashMap<lanlink_proto::NodeId, Binding>,
    last_prune: Instant,
}

/// UDP relay server.
pub struct RelayServer {
    socket: Arc<UdpSocket>,
    state: Arc<Mutex<RelayState>>,
}

impl RelayServer {
    /// Bind a new relay server to `addr`.
    pub async fn bind(addr: SocketAddr) -> anyhow::Result<Self> {
        let socket = Arc::new(UdpSocket::bind(addr).await?);
        tracing::info!(%addr, "relay server listening");
        Ok(Self {
            socket,
            state: Arc::new(Mutex::new(RelayState {
                bindings: HashMap::new(),
                last_prune: Instant::now(),
            })),
        })
    }

    /// Return the local bound address.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// Run the relay loop until the socket closes.
    pub async fn run(self) -> anyhow::Result<()> {
        let mut buf = vec![0u8; 2048];
        let mut prune_interval = tokio::time::interval(PRUNE_INTERVAL);
        prune_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = prune_interval.tick() => {
                    self.prune().await;
                }
                recv = self.socket.recv_from(&mut buf) => {
                    let (n, from) = recv?;
                    let packet = &buf[..n];
                    if let Err(e) = self.handle_packet(from, packet).await {
                        tracing::debug!(%from, error = %e, "relay packet dropped");
                    }
                }
            }
        }
    }

    async fn handle_packet(&self, from: SocketAddr, packet: &[u8]) -> Result<(), RelayError> {
        // Relay control packet?
        if let Some(control) = decode_control(packet) {
            let mut state = self.state.lock().await;
            let binding = Binding {
                node_id: control.node_id,
                addr: from,
                last_seen: Instant::now(),
            };
            tracing::debug!(?control.node_id, %from, kind = ?control.kind, "relay binding updated");
            state.bindings.insert(control.node_id, binding);
            return Ok(());
        }

        // Otherwise it must be a LanLink packet that we can route.
        if packet.len() < MIN_LANLINK_LEN {
            return Err(RelayError::MalformedControl);
        }

        let mut receiver = [0u8; lanlink_proto::NODE_ID_LEN];
        receiver.copy_from_slice(&packet[4..4 + lanlink_proto::NODE_ID_LEN]);
        let receiver = lanlink_proto::NodeId(receiver);

        let state = self.state.lock().await;
        let Some(binding) = state.bindings.get(&receiver) else {
            tracing::debug!(?receiver, %from, "no binding for receiver; dropping");
            return Ok(());
        };

        let dest = binding.addr;
        drop(state);

        self.socket.send_to(packet, dest).await?;
        tracing::trace!(?receiver, %from, %dest, bytes = packet.len(), "relayed packet");
        Ok(())
    }

    async fn prune(&self) {
        let now = Instant::now();
        let mut state = self.state.lock().await;
        let before = state.bindings.len();
        state
            .bindings
            .retain(|_, b| now.duration_since(b.last_seen) <= BINDING_LIFETIME);
        let after = state.bindings.len();
        if before != after {
            tracing::info!(
                pruned = before - after,
                remaining = after,
                "relay bindings pruned"
            );
        }
        state.last_prune = now;
    }

    /// Return a snapshot of active bindings (useful for diagnostics).
    pub async fn bindings(&self) -> Vec<Binding> {
        self.state.lock().await.bindings.values().cloned().collect()
    }
}

/// Client-side helpers for interacting with a LanLink relay server.
pub mod codec {
    use lanlink_proto::{NodeId, RELAY_BIND_PACKET_TYPE, RELAY_KEEPALIVE_PACKET_TYPE, RELAY_MAGIC};

    /// Decoded relay control message.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ControlKind {
        Bind,
        Keepalive,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ControlMsg {
        pub kind: ControlKind,
        pub node_id: NodeId,
    }

    const CONTROL_LEN: usize = RELAY_MAGIC.len() + 1 + lanlink_proto::NODE_ID_LEN;

    /// Encode a relay bind packet for `node_id`.
    pub fn encode_bind(node_id: NodeId) -> Vec<u8> {
        encode(RELAY_BIND_PACKET_TYPE, node_id)
    }

    /// Encode a relay keepalive packet for `node_id`.
    pub fn encode_keepalive(node_id: NodeId) -> Vec<u8> {
        encode(RELAY_KEEPALIVE_PACKET_TYPE, node_id)
    }

    fn encode(kind: u8, node_id: NodeId) -> Vec<u8> {
        let mut out = Vec::with_capacity(CONTROL_LEN);
        out.extend_from_slice(RELAY_MAGIC);
        out.push(kind);
        out.extend_from_slice(&node_id.0);
        out
    }

    /// Try to decode a relay control packet. Returns `None` if the buffer is
    /// too short or the magic does not match (i.e. this is a normal LanLink
    /// packet).
    pub fn decode_control(packet: &[u8]) -> Option<ControlMsg> {
        if packet.len() < CONTROL_LEN {
            return None;
        }
        if &packet[..RELAY_MAGIC.len()] != RELAY_MAGIC {
            return None;
        }
        let kind = match packet[RELAY_MAGIC.len()] {
            RELAY_BIND_PACKET_TYPE => ControlKind::Bind,
            RELAY_KEEPALIVE_PACKET_TYPE => ControlKind::Keepalive,
            _ => return None,
        };
        let mut node_id = [0u8; lanlink_proto::NODE_ID_LEN];
        node_id.copy_from_slice(&packet[RELAY_MAGIC.len() + 1..CONTROL_LEN]);
        Some(ControlMsg {
            kind,
            node_id: NodeId(node_id),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{decode_control, encode_bind, ControlKind, RelayServer};
    use std::time::Duration;
    use tokio::net::UdpSocket;

    fn node(id: u8) -> lanlink_proto::NodeId {
        let mut b = [0u8; 8];
        b[7] = id;
        lanlink_proto::NodeId(b)
    }

    #[test]
    fn bind_encode_decode_roundtrip() {
        let n = node(7);
        let packet = encode_bind(n);
        let control = decode_control(&packet).expect("valid bind");
        assert_eq!(control.kind, ControlKind::Bind);
        assert_eq!(control.node_id, n);
    }

    #[test]
    fn normal_lanlink_packet_not_control() {
        let mut packet = vec![0u8; 20];
        packet[0] = lanlink_proto::WIRE_PROTOCOL_VERSION;
        packet[1] = lanlink_proto::DATA_PACKET_TYPE;
        assert!(decode_control(&packet).is_none());
    }

    #[tokio::test]
    async fn relay_forwards_between_bound_clients() {
        let node_a = node(1);
        let node_b = node(2);

        let relay = RelayServer::bind("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind relay");
        let relay_addr = relay.local_addr().unwrap();
        tokio::spawn(relay.run());

        let socket_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let socket_b = UdpSocket::bind("127.0.0.1:0").await.unwrap();

        // Both clients bind to the relay.
        socket_a
            .send_to(&encode_bind(node_a), relay_addr)
            .await
            .unwrap();
        socket_b
            .send_to(&encode_bind(node_b), relay_addr)
            .await
            .unwrap();

        // Give the relay a moment to process the binds.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Client A sends a fake LanLink packet addressed to node B.
        let mut data = vec![0u8; 4 + lanlink_proto::NODE_ID_LEN + 4];
        data[0] = lanlink_proto::WIRE_PROTOCOL_VERSION;
        data[1] = lanlink_proto::DATA_PACKET_TYPE;
        data[4..4 + lanlink_proto::NODE_ID_LEN].copy_from_slice(&node_b.0);
        data[4 + lanlink_proto::NODE_ID_LEN..].copy_from_slice(b"ping");

        socket_a.send_to(&data, relay_addr).await.unwrap();

        let mut buf = [0u8; 256];
        let (n, from) = socket_b.recv_from(&mut buf).await.unwrap();
        assert_eq!(from, relay_addr);
        assert_eq!(&buf[..n], &data);
    }
}
