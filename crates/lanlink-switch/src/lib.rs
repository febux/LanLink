//! Ethernet switch core for the virtual LAN.
//!
//! Responsibilities:
//! - read raw Ethernet frames from a TAP device,
//! - learn source-MAC -> peer associations,
//! - forward unicast frames to the right peer, flood unknown/broadcast/multicast,
//! - split large Ethernet frames into chunks that fit under the UDP MTU budget,
//! - reassemble chunks and return a complete frame to be written back to the TAP.
//!
//! All per-peer state (address, replay window, last seen counter) lives here so
//! that `lanlinkd` can be a thin async I/O wrapper.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

pub use self::table::{MacEntry, SwitchTable};

/// Inner packet layout used by the switch:
/// ```text
/// [sender node id: 8 bytes]
/// [counter: 8 bytes BE]
/// [frame id: 4 bytes BE]
/// [chunk index: 2 bytes BE]
/// [total chunks: 2 bytes BE]
/// [chunk payload: variable]
/// ```
const INNER_HEADER_LEN: usize = lanlink_proto::NODE_ID_LEN + 8 + 4 + 2 + 2;
const MAX_CHUNK_PAYLOAD: usize = lanlink_proto::MAX_INNER_PAYLOAD - INNER_HEADER_LEN;
const FRAGMENT_TIMEOUT: Duration = Duration::from_secs(5);
const MAC_TABLE_MAX_AGE: Duration = Duration::from_secs(300);

#[derive(Debug, thiserror::Error)]
pub enum SwitchError {
    #[error("crypto error: {0}")]
    Crypto(#[from] lanlink_crypto::CryptoError),
    #[error("packet rejected: {0}")]
    Rejected(&'static str),
    #[error("malformed inner packet: {0}")]
    Malformed(&'static str),
}

/// Per-peer data-plane state.
#[derive(Debug, Clone)]
pub struct PeerState {
    pub addr: SocketAddr,
    pub last_seen: Instant,
    pub rx_counter: u64,
    pub replay: lanlink_crypto::ReplayWindow,
    /// Bytes transmitted to this peer (sealed UDP payloads).
    pub tx_bytes: u64,
    /// Bytes received from this peer (sealed UDP payloads).
    pub rx_bytes: u64,
    /// Last latency-probe round trip, if a PONG was received.
    pub rtt: Option<Duration>,
}

/// Read-only per-peer statistics snapshot used by the local status API.
#[derive(Debug, Clone)]
pub struct PeerStat {
    pub node_id: lanlink_proto::NodeId,
    pub addr: SocketAddr,
    pub last_seen: Instant,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub rtt: Option<Duration>,
}

/// Result of feeding a UDP datagram into the switch.
#[derive(Debug)]
pub enum UdpOutcome {
    /// A complete Ethernet frame to write to the local TAP device.
    Frame(Vec<u8>),
    /// A control packet (e.g. a PONG reply) to send back to the datagram's
    /// source address.
    Reply(Vec<u8>),
    /// Nothing to do: keepalive, ping, drop, replay, or an incomplete fragment.
    Ignored,
}

struct ReassemblyBuffer {
    total: u16,
    chunks: Vec<Option<Vec<u8>>>,
    received: u16,
    last_update: Instant,
}

/// Inner payload length of a control probe packet: sender id + counter +
/// probe timestamp (milliseconds since the sender's switch was created).
const PROBE_INNER_LEN: usize = lanlink_proto::NODE_ID_LEN + 8 + 8;

/// A MAC-learning virtual Ethernet switch with integrated chunking and reassembly.
pub struct Switch {
    local: lanlink_proto::NodeId,
    session: lanlink_crypto::StaticSession,
    mac_table: SwitchTable,
    peers: HashMap<lanlink_proto::NodeId, PeerState>,
    fragments: HashMap<(lanlink_proto::NodeId, u32), ReassemblyBuffer>,
    next_frame_id: u32,
    tx_counter: u64,
    last_prune: Instant,
    /// Creation time; probe timestamps are encoded as milliseconds since this
    /// instant so RTT math stays monotonic regardless of wall-clock jumps.
    start: Instant,
}

impl Switch {
    /// Create a new switch for the local node with the given network key session.
    pub fn new(local: lanlink_proto::NodeId, session: lanlink_crypto::StaticSession) -> Self {
        Self {
            local,
            session,
            mac_table: SwitchTable::new(),
            peers: HashMap::new(),
            fragments: HashMap::new(),
            next_frame_id: rand::random(),
            tx_counter: 1,
            last_prune: Instant::now(),
            start: Instant::now(),
        }
    }

    /// Register or update a peer that the switch may send frames to.
    pub fn add_peer(&mut self, id: lanlink_proto::NodeId, addr: SocketAddr) {
        match self.peers.get_mut(&id) {
            Some(p) => {
                p.addr = addr;
                p.last_seen = Instant::now();
            }
            None => {
                self.peers.insert(
                    id,
                    PeerState {
                        addr,
                        last_seen: Instant::now(),
                        rx_counter: 0,
                        replay: lanlink_crypto::ReplayWindow::new(),
                        tx_bytes: 0,
                        rx_bytes: 0,
                        rtt: None,
                    },
                );
            }
        }
    }

    /// Remove a peer and any MAC table entries that pointed to it.
    pub fn remove_peer(&mut self, id: lanlink_proto::NodeId) {
        self.peers.remove(&id);
        self.mac_table.retain(|_, entry| entry.peer != id);
        self.fragments.retain(|(sender, _), _| sender != &id);
    }

    /// Synchronize the peer table with a list from the coordinator.
    pub fn sync_peers(&mut self, peers: &[lanlink_proto::PeerInfo]) {
        let mut keep = HashMap::new();
        for p in peers {
            let addr = p
                .direct_candidates
                .first()
                .copied()
                .or_else(|| p.relay_candidates.first().copied());
            if let Some(addr) = addr {
                keep.insert(p.node_id, addr);
            }
        }

        // Remove peers that disappeared from the coordinator view.
        let gone: Vec<lanlink_proto::NodeId> = self
            .peers
            .keys()
            .copied()
            .filter(|id| !keep.contains_key(id))
            .collect();
        for id in gone {
            self.remove_peer(id);
        }

        // Add or update current peers.
        for (id, addr) in keep {
            self.add_peer(id, addr);
        }
    }

    /// Number of known peers.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Read a frame from the local TAP, decide where it goes, encrypt+chunk it,
    /// and return a list of UDP packets to transmit.
    pub fn on_tap_frame(
        &mut self,
        frame: &[u8],
    ) -> Result<Vec<(SocketAddr, Vec<u8>)>, SwitchError> {
        if frame.len() < 14 {
            return Err(SwitchError::Malformed("frame shorter than Ethernet header"));
        }

        let dst_mac = &frame[0..6];
        let _src_mac = &frame[6..12];
        let flood = SwitchTable::is_flood_dest(dst_mac.try_into().expect("six bytes"));

        let destinations: Vec<lanlink_proto::NodeId> = if flood {
            self.peers.keys().copied().collect()
        } else {
            let dst = dst_mac.try_into().expect("six bytes");
            match self.mac_table.lookup(dst) {
                Some(entry) => vec![entry.peer],
                None => self.peers.keys().copied().collect(),
            }
        };

        if destinations.is_empty() {
            tracing::trace!(dst_mac = %hex(dst_mac), "no peers known; dropping frame");
            return Ok(Vec::new());
        }

        let frame_id = self.next_frame_id();
        let chunks = fragment(frame, MAX_CHUNK_PAYLOAD);
        let total = chunks.len() as u16;

        let mut out = Vec::new();
        for receiver in destinations {
            let Some(peer) = self.peers.get(&receiver) else {
                continue;
            };
            let addr = peer.addr;

            let mut sent_bytes = 0u64;
            for (idx, chunk) in chunks.iter().enumerate() {
                // Direct field access (not the helper methods) so the borrow
                // checker sees disjoint field borrows with `self.peers`.
                let counter = self.tx_counter;
                self.tx_counter = self.tx_counter.wrapping_add(1);
                let inner = build_inner(self.local, counter, frame_id, idx as u16, total, chunk);
                let packet =
                    self.session
                        .seal(receiver, lanlink_proto::DATA_PACKET_TYPE, &inner)?;
                sent_bytes += packet.len() as u64;
                out.push((addr, packet));
            }
            if let Some(peer) = self.peers.get_mut(&receiver) {
                peer.tx_bytes += sent_bytes;
            }
        }

        self.prune_if_needed();
        Ok(out)
    }

    /// Update the reachable address for a known peer. Called by the puncher
    /// when a verified direct path is discovered.
    pub fn set_peer_addr(&mut self, id: lanlink_proto::NodeId, addr: SocketAddr) {
        if let Some(peer) = self.peers.get_mut(&id) {
            if peer.addr != addr {
                tracing::info!(?id, old = %peer.addr, new = %addr, "updated peer address");
                peer.addr = addr;
            }
            peer.last_seen = Instant::now();
        }
    }

    /// Return a snapshot of known peer ids and their current addresses.
    pub fn peer_addrs(&self) -> Vec<(lanlink_proto::NodeId, SocketAddr)> {
        self.peers
            .iter()
            .map(|(id, state)| (*id, state.addr))
            .collect()
    }

    /// Return a snapshot of per-peer statistics for the local status API.
    pub fn peer_stats(&self) -> Vec<PeerStat> {
        self.peers
            .iter()
            .map(|(id, state)| PeerStat {
                node_id: *id,
                addr: state.addr,
                last_seen: state.last_seen,
                tx_bytes: state.tx_bytes,
                rx_bytes: state.rx_bytes,
                rtt: state.rtt,
            })
            .collect()
    }

    /// Build a sealed keepalive packet for a peer. The inner payload carries
    /// the sender id and a transmit counter so the receiver can attribute the
    /// keepalive and refresh the peer's liveness.
    pub fn seal_keepalive(
        &mut self,
        peer: lanlink_proto::NodeId,
    ) -> Result<Vec<u8>, lanlink_crypto::CryptoError> {
        let counter = self.tx_counter;
        self.tx_counter = self.tx_counter.wrapping_add(1);
        let inner = build_probe_inner(self.local, counter, 0);
        self.session
            .seal(peer, lanlink_proto::KEEPALIVE_PACKET_TYPE, &inner)
    }

    /// Build a sealed latency-probe packet. `now_ms` is milliseconds from
    /// this switch's probe clock; the echo in the PONG reply lets us compute
    /// RTT without trusting any wall clock.
    pub fn seal_ping(
        &mut self,
        peer: lanlink_proto::NodeId,
        now_ms: u64,
    ) -> Result<Vec<u8>, lanlink_crypto::CryptoError> {
        let counter = self.tx_counter;
        self.tx_counter = self.tx_counter.wrapping_add(1);
        let inner = build_probe_inner(self.local, counter, now_ms);
        self.session
            .seal(peer, lanlink_proto::PING_PACKET_TYPE, &inner)
    }

    /// Milliseconds elapsed since this switch was created (probe clock).
    pub fn probe_clock_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    /// Build a sealed PONG reply echoing the probe timestamp.
    fn seal_pong(
        &mut self,
        peer: lanlink_proto::NodeId,
        timestamp_ms: u64,
    ) -> Result<Vec<u8>, lanlink_crypto::CryptoError> {
        let counter = self.tx_counter;
        self.tx_counter = self.tx_counter.wrapping_add(1);
        let inner = build_probe_inner(self.local, counter, timestamp_ms);
        self.session
            .seal(peer, lanlink_proto::PONG_PACKET_TYPE, &inner)
    }

    /// Process a UDP datagram received from the network. Returns
    /// [`UdpOutcome::Frame`] with a complete Ethernet frame for the local TAP,
    /// [`UdpOutcome::Reply`] with a control packet to send back to the
    /// datagram's source (latency-probe PONGs), or [`UdpOutcome::Ignored`]
    /// for keepalives, pings, drops, replays, and fragments that are still
    /// incomplete.
    pub fn on_udp_packet(
        &mut self,
        from: SocketAddr,
        packet: &[u8],
    ) -> Result<UdpOutcome, SwitchError> {
        let (receiver, ptype, inner) = self.session.open(packet)?;
        if receiver != self.local {
            // Not addressed to us. This is normal on shared ports / broadcasts
            // arriving at the wrong receiver id.
            return Ok(UdpOutcome::Ignored);
        }

        if inner.len() < lanlink_proto::NODE_ID_LEN {
            return Err(SwitchError::Malformed("inner plaintext too short"));
        }

        let sender = lanlink_proto::NodeId(
            inner[0..lanlink_proto::NODE_ID_LEN]
                .try_into()
                .expect("eight bytes"),
        );

        // Keepalives carry sender id + counter but no replay-protected state;
        // they refresh peer liveness and learn NAT rebinds.
        if ptype == lanlink_proto::KEEPALIVE_PACKET_TYPE {
            if let Some(peer) = self.peers.get_mut(&sender) {
                if peer.addr != from {
                    tracing::info!(?sender, old = %peer.addr, new = %from, "peer address changed");
                    peer.addr = from;
                }
                peer.last_seen = Instant::now();
            }
            return Ok(UdpOutcome::Ignored);
        }

        if inner.len() < lanlink_proto::NODE_ID_LEN + 8 {
            return Err(SwitchError::Malformed("inner plaintext too short"));
        }
        let counter = read_u64(&inner[lanlink_proto::NODE_ID_LEN..lanlink_proto::NODE_ID_LEN + 8]);

        let Some(peer) = self.peers.get_mut(&sender) else {
            tracing::debug!(?sender, %from, "dropping packet from unknown peer");
            return Ok(UdpOutcome::Ignored);
        };

        // NAT rebind: remember the address this peer actually reached us from.
        if peer.addr != from {
            tracing::info!(?sender, old = %peer.addr, new = %from, "peer address changed");
            peer.addr = from;
        }

        if !peer.replay.accept(counter) {
            tracing::debug!(?sender, counter, "replay window rejected packet");
            return Ok(UdpOutcome::Ignored);
        }
        peer.rx_counter = counter;
        peer.last_seen = Instant::now();
        peer.rx_bytes += packet.len() as u64;

        // Latency probes: a PING timestamp comes from the sender's probe
        // clock and is echoed verbatim in the PONG; when a PONG arrives the
        // timestamp originated on our own clock, so the delta is the RTT.
        if ptype == lanlink_proto::PING_PACKET_TYPE || ptype == lanlink_proto::PONG_PACKET_TYPE {
            if inner.len() < PROBE_INNER_LEN {
                return Err(SwitchError::Malformed("probe payload too short"));
            }
            let ts = read_u64(&inner[lanlink_proto::NODE_ID_LEN + 8..PROBE_INNER_LEN]);
            if ptype == lanlink_proto::PING_PACKET_TYPE {
                let pong = self.seal_pong(sender, ts)?;
                return Ok(UdpOutcome::Reply(pong));
            }
            let now_ms = self.probe_clock_ms();
            let rtt = Duration::from_millis(now_ms.saturating_sub(ts));
            if let Some(peer) = self.peers.get_mut(&sender) {
                peer.rtt = Some(rtt);
            }
            return Ok(UdpOutcome::Ignored);
        }

        if inner.len() < INNER_HEADER_LEN {
            return Err(SwitchError::Malformed("inner plaintext too short for data"));
        }

        let frame_id =
            read_u32(&inner[lanlink_proto::NODE_ID_LEN + 8..lanlink_proto::NODE_ID_LEN + 12]);
        let chunk_idx =
            read_u16(&inner[lanlink_proto::NODE_ID_LEN + 12..lanlink_proto::NODE_ID_LEN + 14]);
        let total =
            read_u16(&inner[lanlink_proto::NODE_ID_LEN + 14..lanlink_proto::NODE_ID_LEN + 16]);
        let payload = &inner[INNER_HEADER_LEN..];

        let frame = if total <= 1 {
            payload.to_vec()
        } else {
            let key = (sender, frame_id);
            let entry = self
                .fragments
                .entry(key)
                .or_insert_with(|| ReassemblyBuffer {
                    total,
                    chunks: vec![None; total as usize],
                    received: 0,
                    last_update: Instant::now(),
                });

            if entry.total != total {
                tracing::debug!(?sender, frame_id, "fragment total mismatch; dropping");
                return Ok(UdpOutcome::Ignored);
            }
            if chunk_idx >= total {
                return Err(SwitchError::Malformed("chunk index out of range"));
            }
            let slot = chunk_idx as usize;
            if entry.chunks[slot].is_none() {
                entry.chunks[slot] = Some(payload.to_vec());
                entry.received += 1;
            }
            entry.last_update = Instant::now();

            if entry.received == total {
                let mut assembled = Vec::new();
                for chunk in entry.chunks.drain(..) {
                    assembled.extend_from_slice(&chunk.expect("received count matches"));
                }
                self.fragments.remove(&key);
                assembled
            } else {
                self.prune_if_needed();
                return Ok(UdpOutcome::Ignored);
            }
        };

        if frame.len() >= 14 {
            let src_mac = frame[6..12].try_into().expect("six bytes");
            self.mac_table.learn(src_mac, sender);
        }

        self.prune_if_needed();
        Ok(UdpOutcome::Frame(frame))
    }

    fn next_frame_id(&mut self) -> u32 {
        let id = self.next_frame_id;
        self.next_frame_id = self.next_frame_id.wrapping_add(1);
        id
    }

    fn prune_if_needed(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_prune) < Duration::from_secs(1) {
            return;
        }
        self.last_prune = now;
        self.fragments
            .retain(|_, b| now.duration_since(b.last_update) <= FRAGMENT_TIMEOUT);
        self.mac_table.prune(MAC_TABLE_MAX_AGE, now);
    }
}

fn fragment(frame: &[u8], max_chunk: usize) -> Vec<Vec<u8>> {
    if frame.len() <= max_chunk {
        return vec![frame.to_vec()];
    }
    frame.chunks(max_chunk).map(|c| c.to_vec()).collect()
}

fn build_inner(
    sender: lanlink_proto::NodeId,
    counter: u64,
    frame_id: u32,
    chunk_idx: u16,
    total: u16,
    payload: &[u8],
) -> Vec<u8> {
    let mut inner = Vec::with_capacity(INNER_HEADER_LEN + payload.len());
    inner.extend_from_slice(&sender.0);
    inner.extend_from_slice(&counter.to_be_bytes());
    inner.extend_from_slice(&frame_id.to_be_bytes());
    inner.extend_from_slice(&chunk_idx.to_be_bytes());
    inner.extend_from_slice(&total.to_be_bytes());
    inner.extend_from_slice(payload);
    inner
}

/// Inner payload for keepalives and latency probes:
/// `[sender id: 8][counter: 8][timestamp ms: 8]`.
fn build_probe_inner(sender: lanlink_proto::NodeId, counter: u64, timestamp_ms: u64) -> Vec<u8> {
    let mut inner = Vec::with_capacity(PROBE_INNER_LEN);
    inner.extend_from_slice(&sender.0);
    inner.extend_from_slice(&counter.to_be_bytes());
    inner.extend_from_slice(&timestamp_ms.to_be_bytes());
    inner
}

fn read_u64(b: &[u8]) -> u64 {
    u64::from_be_bytes(b.try_into().expect("eight bytes"))
}

fn read_u32(b: &[u8]) -> u32 {
    u32::from_be_bytes(b.try_into().expect("four bytes"))
}

fn read_u16(b: &[u8]) -> u16 {
    u16::from_be_bytes(b.try_into().expect("two bytes"))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

// ---------------------------------------------------------------------------
// MAC table (internal submodule)
// ---------------------------------------------------------------------------

mod table {
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    /// A learned MAC entry.
    #[derive(Debug, Clone)]
    pub struct MacEntry {
        pub peer: lanlink_proto::NodeId,
        pub last_seen: Instant,
    }

    /// Simple MAC-learning switch table.
    #[derive(Debug, Default)]
    pub struct SwitchTable {
        table: HashMap<[u8; 6], MacEntry>,
    }

    impl SwitchTable {
        pub fn new() -> Self {
            Self::default()
        }

        /// Learn that `mac` was seen from `peer`.
        pub fn learn(&mut self, mac: [u8; 6], peer: lanlink_proto::NodeId) {
            self.table.insert(
                mac,
                MacEntry {
                    peer,
                    last_seen: Instant::now(),
                },
            );
        }

        /// Look up a destination MAC.
        pub fn lookup(&self, mac: [u8; 6]) -> Option<&MacEntry> {
            self.table.get(&mac)
        }

        /// Remove entries older than `max_age`.
        pub fn prune(&mut self, max_age: Duration, now: Instant) {
            self.table
                .retain(|_, e| now.duration_since(e.last_seen) <= max_age);
        }

        /// Retain only entries matching the predicate.
        pub fn retain<F>(&mut self, mut f: F)
        where
            F: FnMut(&[u8; 6], &MacEntry) -> bool,
        {
            self.table.retain(|k, v| f(k, v));
        }

        /// True for broadcast / IPv4 multicast / IPv6 multicast destination MACs.
        pub fn is_flood_dest(mac: [u8; 6]) -> bool {
            mac == [0xff; 6] || (mac[0] & 0x01) == 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session() -> lanlink_crypto::StaticSession {
        lanlink_crypto::StaticSession::new([0u8; 32])
    }

    fn node(id: u8) -> lanlink_proto::NodeId {
        let mut b = [0u8; 8];
        b[7] = id;
        lanlink_proto::NodeId(b)
    }

    fn eth_frame(src: u8, dst: u8, payload: &[u8]) -> Vec<u8> {
        let mut f = Vec::with_capacity(14 + payload.len());
        let src_mac = [0x02, 0x00, 0x00, 0x00, 0x00, src];
        let dst_mac = [0x02, 0x00, 0x00, 0x00, 0x00, dst];
        f.extend_from_slice(&dst_mac);
        f.extend_from_slice(&src_mac);
        f.extend_from_slice(&[0x08, 0x00]); // IPv4 ethertype
        f.extend_from_slice(payload);
        f
    }

    #[test]
    fn unicast_forwarded_to_learned_peer() {
        let mut sw = Switch::new(node(1), make_session());
        sw.add_peer(node(2), "10.0.0.2:9000".parse().unwrap());

        // Learn that node 2 has MAC ending in 0x02.
        sw.mac_table.learn([0x02, 0, 0, 0, 0, 2], node(2));

        let frame = eth_frame(1, 2, &[1, 2, 3]);
        let out = sw.on_tap_frame(&frame).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "10.0.0.2:9000".parse().unwrap());
    }

    #[test]
    fn unknown_unicast_flooded() {
        let mut sw = Switch::new(node(1), make_session());
        sw.add_peer(node(2), "10.0.0.2:9000".parse().unwrap());
        sw.add_peer(node(3), "10.0.0.3:9000".parse().unwrap());

        let frame = eth_frame(1, 9, &[1, 2, 3]);
        let out = sw.on_tap_frame(&frame).unwrap();
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn broadcast_flooded_to_all_peers() {
        let mut sw = Switch::new(node(1), make_session());
        sw.add_peer(node(2), "10.0.0.2:9000".parse().unwrap());

        let mut frame = eth_frame(1, 0xff, &[1, 2, 3]);
        frame[0..6].copy_from_slice(&[0xff; 6]);
        let out = sw.on_tap_frame(&frame).unwrap();
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn no_peers_returns_empty() {
        let mut sw = Switch::new(node(1), make_session());
        let frame = eth_frame(1, 2, &[1, 2, 3]);
        let out = sw.on_tap_frame(&frame).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn roundtrip_single_chunk() {
        let mut a = Switch::new(node(1), make_session());
        let mut b = Switch::new(node(2), make_session());
        a.add_peer(node(2), "10.0.0.2:9000".parse().unwrap());
        b.add_peer(node(1), "10.0.0.1:9000".parse().unwrap());

        let frame = eth_frame(1, 2, &[0xaa; 100]);
        let packets = a.on_tap_frame(&frame).unwrap();
        assert_eq!(packets.len(), 1);

        let decoded = match b.on_udp_packet(packets[0].0, &packets[0].1).unwrap() {
            UdpOutcome::Frame(f) => f,
            other => panic!("expected frame, got {other:?}"),
        };
        assert_eq!(decoded, frame);
        assert_eq!(
            b.mac_table.lookup([0x02, 0, 0, 0, 0, 1]).map(|e| e.peer),
            Some(node(1))
        );
    }

    #[test]
    fn roundtrip_fragmented_frame() {
        let mut a = Switch::new(node(1), make_session());
        let mut b = Switch::new(node(2), make_session());
        a.add_peer(node(2), "10.0.0.2:9000".parse().unwrap());
        b.add_peer(node(1), "10.0.0.1:9000".parse().unwrap());

        // Payload large enough to need two chunks.
        let big = vec![0xbb; MAX_CHUNK_PAYLOAD + 100];
        let frame = eth_frame(1, 2, &big);
        let packets = a.on_tap_frame(&frame).unwrap();
        assert_eq!(packets.len(), 2);

        let first = b.on_udp_packet(packets[0].0, &packets[0].1).unwrap();
        assert!(matches!(first, UdpOutcome::Ignored));
        let second = match b.on_udp_packet(packets[1].0, &packets[1].1).unwrap() {
            UdpOutcome::Frame(f) => f,
            other => panic!("expected frame, got {other:?}"),
        };
        assert_eq!(second, frame);
    }

    #[test]
    fn replayed_packet_rejected() {
        let mut a = Switch::new(node(1), make_session());
        let mut b = Switch::new(node(2), make_session());
        a.add_peer(node(2), "10.0.0.2:9000".parse().unwrap());
        b.add_peer(node(1), "10.0.0.1:9000".parse().unwrap());

        let frame = eth_frame(1, 2, &[0xcc; 50]);
        let packets = a.on_tap_frame(&frame).unwrap();
        assert!(matches!(
            b.on_udp_packet(packets[0].0, &packets[0].1).unwrap(),
            UdpOutcome::Frame(_)
        ));
        assert!(matches!(
            b.on_udp_packet(packets[0].0, &packets[0].1).unwrap(),
            UdpOutcome::Ignored
        ));
    }

    #[test]
    fn packet_for_wrong_receiver_dropped() {
        let mut a = Switch::new(node(1), make_session());
        let mut c = Switch::new(node(3), make_session());
        a.add_peer(node(2), "10.0.0.2:9000".parse().unwrap());
        // c pretends to be the intended receiver to check that the receiver id
        // in the outer packet prevents decryption.
        let frame = eth_frame(1, 2, &[0xdd; 50]);
        let packets = a.on_tap_frame(&frame).unwrap();
        assert!(matches!(
            c.on_udp_packet(packets[0].0, &packets[0].1).unwrap(),
            UdpOutcome::Ignored
        ));
    }

    #[test]
    fn sync_peers_falls_back_to_relay_candidate() {
        let mut sw = Switch::new(node(1), make_session());
        let relay_addr: SocketAddr = "203.0.113.50:3478".parse().unwrap();
        sw.sync_peers(&[lanlink_proto::PeerInfo {
            node_id: node(2),
            virtual_ip: lanlink_proto::VirtualIp("26.0.0.2".parse().unwrap()),
            direct_candidates: vec![],
            relay_candidates: vec![relay_addr],
            is_gateway: false,
            shared_subnet: String::new(),
        }]);
        assert_eq!(sw.peer_count(), 1);
        assert_eq!(sw.peer_addrs(), vec![(node(2), relay_addr)]);
    }

    #[test]
    fn ping_pong_measures_rtt() {
        let addr_a: SocketAddr = "10.0.0.1:9000".parse().unwrap();
        let addr_b: SocketAddr = "10.0.0.2:9000".parse().unwrap();
        let mut a = Switch::new(node(1), make_session());
        let mut b = Switch::new(node(2), make_session());
        a.add_peer(node(2), addr_b);
        b.add_peer(node(1), addr_a);

        let ping = a.seal_ping(node(2), a.probe_clock_ms()).unwrap();
        // A's ping carries its own probe-clock timestamp; B echoes it back.
        let pong = match b.on_udp_packet(addr_a, &ping).unwrap() {
            UdpOutcome::Reply(p) => p,
            other => panic!("expected reply, got {other:?}"),
        };
        // Simulate network delay so the RTT is measurable.
        std::thread::sleep(Duration::from_millis(15));
        assert!(matches!(
            a.on_udp_packet(addr_b, &pong).unwrap(),
            UdpOutcome::Ignored
        ));

        let stats = a.peer_stats();
        let rtt = stats
            .iter()
            .find(|s| s.node_id == node(2))
            .and_then(|s| s.rtt);
        assert!(rtt.expect("rtt recorded").as_millis() >= 10);
    }

    #[test]
    fn keepalive_refreshes_liveness_and_learns_rebind() {
        let old_addr: SocketAddr = "10.0.0.2:9000".parse().unwrap();
        let new_addr: SocketAddr = "10.0.0.9:9100".parse().unwrap();
        let mut a = Switch::new(node(1), make_session());
        let mut b = Switch::new(node(2), make_session());
        a.add_peer(node(2), old_addr);
        b.add_peer(node(1), "10.0.0.1:9000".parse().unwrap());

        // Age a's view of the peer by construction: no packet seen yet.
        let keep = b.seal_keepalive(node(1)).unwrap();
        // The keepalive arrives from a rebound source address.
        assert!(matches!(
            a.on_udp_packet(new_addr, &keep).unwrap(),
            UdpOutcome::Ignored
        ));

        let stats = a.peer_stats();
        let peer = stats.iter().find(|s| s.node_id == node(2)).unwrap();
        assert_eq!(
            peer.addr, new_addr,
            "keepalive should learn the new address"
        );
        assert!(peer.last_seen.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn per_peer_byte_counters_accumulate() {
        let mut a = Switch::new(node(1), make_session());
        let mut b = Switch::new(node(2), make_session());
        a.add_peer(node(2), "10.0.0.2:9000".parse().unwrap());
        b.add_peer(node(1), "10.0.0.1:9000".parse().unwrap());

        let frame = eth_frame(1, 2, &[0xee; 60]);
        let packets = a.on_tap_frame(&frame).unwrap();
        assert!(!packets.is_empty());
        let tx: u64 = packets.iter().map(|(_, p)| p.len() as u64).sum();
        assert_eq!(
            a.peer_stats()[0].tx_bytes,
            tx,
            "tx side counts sealed bytes"
        );

        match b.on_udp_packet(packets[0].0, &packets[0].1).unwrap() {
            UdpOutcome::Frame(_) => {}
            other => panic!("expected frame, got {other:?}"),
        }
        assert_eq!(b.peer_stats()[0].rx_bytes, packets[0].1.len() as u64);
    }

    #[test]
    fn ping_counters_share_replay_window_with_data() {
        let mut a = Switch::new(node(1), make_session());
        let mut b = Switch::new(node(2), make_session());
        a.add_peer(node(2), "10.0.0.2:9000".parse().unwrap());
        b.add_peer(node(1), "10.0.0.1:9000".parse().unwrap());

        // Probe and data use the same counter stream, so a data packet
        // sealed after a ping must still pass the replay window.
        let ping = a.seal_ping(node(2), 0).unwrap();
        assert!(matches!(
            b.on_udp_packet(packets_addr(), &ping).unwrap(),
            UdpOutcome::Reply(_)
        ));

        let frame = eth_frame(1, 2, &[0xff; 30]);
        let data = a.on_tap_frame(&frame).unwrap();
        assert!(matches!(
            b.on_udp_packet(data[0].0, &data[0].1).unwrap(),
            UdpOutcome::Frame(_)
        ));
    }

    fn packets_addr() -> SocketAddr {
        "10.0.0.2:9000".parse().unwrap()
    }
}
