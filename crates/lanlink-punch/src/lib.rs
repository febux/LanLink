//! STUN client and NAT hole-punching logic for LanLink.
//!
//! Lifecycle:
//! 1. Gather local candidates (host IPs + ports).
//! 2. Query STUN server(s) for server-reflexive candidates using the same UDP
//!    socket that will carry data traffic, so the discovered public port is the
//!    one peers need to punch.
//! 3. Exchange candidates through the coordinator.
//! 4. Fire authenticated PUNCH_PROBE packets at candidate pairs until a verified
//!    PUNCH_REPLY comes back.
//! 5. The verified remote address is handed to the switch; data traffic keeps
//!    the NAT mapping alive.
//!
//! This crate does **not** implement a full ICE stack. It is intentionally
//! simple: one UDP socket per network, symmetric simultaneous open, and a
//! small fixed set of STUN servers.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::time::{interval, timeout, Instant};

pub use candidate::{gather_host_candidates, Candidate, CandidateKind};

/// Default STUN servers used to discover server-reflexive addresses.
pub const DEFAULT_STUN_SERVERS: &[&str] = &[
    "stun.l.google.com:19302",
    "stun1.l.google.com:19302",
    "stun2.l.google.com:19302",
];

/// How long we keep firing probes at a peer before giving up on a direct path.
pub const PUNCH_TIMEOUT: Duration = Duration::from_secs(8);

/// Delay between probe bursts.
pub const PROBE_INTERVAL: Duration = Duration::from_millis(250);

/// Number of candidate pairs to try in one burst.
pub const PROBES_PER_BURST: usize = 4;

#[derive(Debug, Error)]
pub enum PunchError {
    #[error("stun binding failed: {0}")]
    Stun(String),
    #[error("no viable candidate pair")]
    NoPair,
    #[error("crypto error: {0}")]
    Crypto(#[from] lanlink_crypto::CryptoError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type for punch operations.
pub type Result<T> = std::result::Result<T, PunchError>;

/// A verified address change event produced by the puncher.
#[derive(Debug, Clone)]
pub struct VerifiedPath {
    pub peer: lanlink_proto::NodeId,
    pub addr: SocketAddr,
}

/// Hole-punching engine tied to a single UDP socket and network key.
pub struct Puncher {
    local_node: lanlink_proto::NodeId,
    socket: Arc<UdpSocket>,
    session: lanlink_crypto::StaticSession,
    stun_servers: Vec<SocketAddr>,
}

impl Puncher {
    /// Create a puncher for `local_node` using the shared `socket`. The same
    /// `session` must be shared with the switch so probes and data are mutually
    /// authenticated.
    pub fn new(
        local_node: lanlink_proto::NodeId,
        socket: Arc<UdpSocket>,
        session: lanlink_crypto::StaticSession,
        stun_servers: Vec<SocketAddr>,
    ) -> Self {
        Self {
            local_node,
            socket,
            session,
            stun_servers,
        }
    }

    /// Discover host and server-reflexive candidates for the local socket.
    ///
    /// The returned list always includes the local socket's bound address as a
    /// host candidate, followed by one server-reflexive candidate per reachable
    /// STUN server. If STUN fails, host candidates are still returned so LAN and
    /// port-forwarded scenarios continue to work.
    pub async fn gather_candidates(&self) -> Vec<Candidate> {
        let mut candidates = gather_host_candidates_for_socket(&self.socket).await;

        for stun_server in &self.stun_servers {
            match stun_binding(&self.socket, *stun_server).await {
                Ok(reflexive) => {
                    tracing::info!(%stun_server, %reflexive, "STUN binding succeeded");
                    candidates.push(Candidate {
                        addr: reflexive,
                        kind: CandidateKind::ServerReflexive,
                    });
                }
                Err(e) => {
                    tracing::warn!(%stun_server, error = %e, "STUN binding failed");
                }
            }
        }

        candidates
    }

    /// Attempt to establish a direct path to `peer` using the supplied
    /// `remote_candidates`. We also re-advertise `local_candidates` implicitly
    /// because the coordinator already shared them with the peer.
    ///
    /// Returns the verified remote `SocketAddr` once a bidirectional path is
    /// confirmed, or `PunchError::NoPair` after `PUNCH_TIMEOUT`.
    pub async fn punch_peer(
        &self,
        peer: lanlink_proto::NodeId,
        remote_candidates: Vec<Candidate>,
    ) -> Result<VerifiedPath> {
        let local_candidates = gather_host_candidates_for_socket(&self.socket).await;

        // If the peer is on the same host, host candidates usually win.
        let pairs = build_candidate_pairs(&local_candidates, &remote_candidates);
        if pairs.is_empty() {
            return Err(PunchError::NoPair);
        }

        let mut ticker = interval(PROBE_INTERVAL);
        let deadline = Instant::now() + PUNCH_TIMEOUT;

        // Burst probes at the start, then periodically.
        self.fire_probe_burst(peer, &pairs, PROBES_PER_BURST)
            .await?;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }

            tokio::select! {
                _ = ticker.tick() => {
                    self.fire_probe_burst(peer, &pairs, PROBES_PER_BURST).await?;
                }
                _ = tokio::time::sleep(remaining) => {
                    break;
                }
            }
        }

        Err(PunchError::NoPair)
    }

    /// Process a PUNCH_PROBE received on the shared socket. If the probe is
    /// authenticated and addressed to us, send a PUNCH_REPLY back to the source
    /// address. Returns true if a reply was sent.
    ///
    /// This must be called from the daemon's UDP receive loop **before** the
    /// packet is handed to the switch.
    pub async fn on_punch_probe(&self, from: SocketAddr, packet: &[u8]) -> Result<bool> {
        let (receiver, ptype, inner) = self.session.open(packet)?;
        if receiver != self.local_node || ptype != lanlink_proto::PUNCH_PROBE_PACKET_TYPE {
            return Ok(false);
        }
        if inner.len() < lanlink_proto::NODE_ID_LEN + 8 {
            return Ok(false);
        }

        let mut sender_bytes = [0u8; lanlink_proto::NODE_ID_LEN];
        sender_bytes.copy_from_slice(&inner[..lanlink_proto::NODE_ID_LEN]);
        let peer = lanlink_proto::NodeId(sender_bytes);

        let mut cookie_bytes = [0u8; 8];
        cookie_bytes
            .copy_from_slice(&inner[lanlink_proto::NODE_ID_LEN..lanlink_proto::NODE_ID_LEN + 8]);

        let reply_inner = self.build_punch_reply_inner(peer, cookie_bytes);
        let reply =
            self.session
                .seal(peer, lanlink_proto::PUNCH_REPLY_PACKET_TYPE, &reply_inner)?;
        self.socket.send_to(&reply, from).await?;
        Ok(true)
    }

    /// Process a PUNCH_REPLY received on the shared socket. If the reply is
    /// authenticated and addressed to us, the verified remote address is
    /// returned.
    ///
    /// This must be called from the daemon's UDP receive loop **before** the
    /// packet is handed to the switch.
    pub fn on_punch_reply(&self, from: SocketAddr, packet: &[u8]) -> Result<Option<VerifiedPath>> {
        let (receiver, ptype, inner) = self.session.open(packet)?;
        if receiver != self.local_node || ptype != lanlink_proto::PUNCH_REPLY_PACKET_TYPE {
            return Ok(None);
        }
        if inner.len() < lanlink_proto::NODE_ID_LEN + 8 {
            return Ok(None);
        }

        let mut sender_bytes = [0u8; lanlink_proto::NODE_ID_LEN];
        sender_bytes.copy_from_slice(&inner[..lanlink_proto::NODE_ID_LEN]);
        let peer = lanlink_proto::NodeId(sender_bytes);

        // The address the reply actually came from is the working path, not
        // necessarily the candidate we sent the probe to.
        Ok(Some(VerifiedPath { peer, addr: from }))
    }

    /// Build and send one authenticated PUNCH_PROBE to a candidate address.
    async fn send_probe(
        &self,
        peer: lanlink_proto::NodeId,
        target: SocketAddr,
        cookie: u64,
    ) -> Result<()> {
        let mut inner = Vec::with_capacity(lanlink_proto::NODE_ID_LEN + 8);
        inner.extend_from_slice(&self.local_node.0);
        inner.extend_from_slice(&cookie.to_be_bytes());

        let packet = self
            .session
            .seal(peer, lanlink_proto::PUNCH_PROBE_PACKET_TYPE, &inner)?;
        self.socket.send_to(&packet, target).await?;
        Ok(())
    }

    /// Build the inner plaintext for a PUNCH_REPLY.
    fn build_punch_reply_inner(&self, _to: lanlink_proto::NodeId, cookie: [u8; 8]) -> Vec<u8> {
        let mut inner = Vec::with_capacity(lanlink_proto::NODE_ID_LEN + 8);
        inner.extend_from_slice(&self.local_node.0);
        inner.extend_from_slice(&cookie);
        inner
    }

    /// Fire a small number of probes at distinct candidate pairs.
    async fn fire_probe_burst(
        &self,
        peer: lanlink_proto::NodeId,
        pairs: &[(SocketAddr, SocketAddr)],
        count: usize,
    ) -> Result<()> {
        let cookie = rand::random::<u64>();
        for pair in pairs.iter().take(count) {
            let target = pair.1;
            if let Err(e) = self.send_probe(peer, target, cookie).await {
                tracing::debug!(%target, error = %e, "probe send failed");
            }
        }
        Ok(())
    }
}

/// Query a STUN server for the public address mapped to our local socket.
async fn stun_binding(socket: &UdpSocket, stun_server: SocketAddr) -> Result<SocketAddr> {
    use stun::agent::TransactionId;
    use stun::message::{Getter, Message, BINDING_REQUEST, BINDING_SUCCESS};
    use stun::xoraddr::XorMappedAddress;

    let mut msg = Message::new();
    msg.build(&[Box::<TransactionId>::default(), Box::new(BINDING_REQUEST)])
        .map_err(|e| PunchError::Stun(e.to_string()))?;

    let request = msg.raw;

    socket.send_to(&request, stun_server).await?;

    let mut buf = [0u8; 512];
    let recv = timeout(Duration::from_secs(2), socket.recv_from(&mut buf))
        .await
        .map_err(|_| PunchError::Stun("timeout".to_string()))?
        .map_err(|e| PunchError::Stun(e.to_string()))?;

    let (n, from) = recv;
    if from != stun_server {
        return Err(PunchError::Stun(
            "response from unexpected server".to_string(),
        ));
    }

    let mut resp = Message::new();
    resp.raw.extend_from_slice(&buf[..n]);
    resp.decode().map_err(|e| PunchError::Stun(e.to_string()))?;

    if resp.typ != BINDING_SUCCESS {
        return Err(PunchError::Stun(
            "not a binding success response".to_string(),
        ));
    }

    let mut xor_addr = XorMappedAddress::default();
    xor_addr
        .get_from(&resp)
        .map_err(|e| PunchError::Stun(e.to_string()))?;

    Ok(SocketAddr::new(xor_addr.ip, xor_addr.port))
}

/// Gather host candidates: every local interface IP combined with the socket's
/// local port, plus the loopback address for same-machine testing.
async fn gather_host_candidates_for_socket(socket: &UdpSocket) -> Vec<Candidate> {
    let local_addr = match socket.local_addr() {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!(error = %e, "failed to get local socket address");
            return Vec::new();
        }
    };

    let port = local_addr.port();
    let mut candidates = Vec::new();

    // Always advertise the bound address itself when it is specific.
    if !is_unspecified(local_addr) {
        candidates.push(Candidate {
            addr: local_addr,
            kind: CandidateKind::Host,
        });
    }

    // Enumerate local interfaces.
    match if_addrs::get_if_addrs() {
        Ok(interfaces) => {
            for iface in interfaces {
                let ip = match iface.addr.ip() {
                    std::net::IpAddr::V4(ip) => ip,
                    _ => continue,
                };
                let addr = SocketAddr::new(std::net::IpAddr::V4(ip), port);
                candidates.push(Candidate {
                    addr,
                    kind: CandidateKind::Host,
                });
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to enumerate network interfaces");
        }
    }

    candidates
}

fn is_unspecified(addr: SocketAddr) -> bool {
    match addr.ip() {
        std::net::IpAddr::V4(ip) => ip.is_unspecified(),
        std::net::IpAddr::V6(ip) => ip.is_unspecified(),
    }
}

/// Build candidate pairs from local and remote candidate lists. The result is a
/// list of `(local_addr, remote_addr)` tuples ordered so that host/host pairs
/// come first, then any pair involving a server-reflexive address.
fn build_candidate_pairs(
    local: &[Candidate],
    remote: &[Candidate],
) -> Vec<(SocketAddr, SocketAddr)> {
    let mut scored = Vec::new();
    for lc in local {
        for rc in remote {
            // Only IPv4 for v1.0.
            if lc.addr.is_ipv4() && rc.addr.is_ipv4() {
                let score = pair_score(lc.kind, rc.kind);
                scored.push((score, lc.addr, rc.addr));
            }
        }
    }

    scored.sort_by_key(|(score, _, _)| *score);
    scored.into_iter().map(|(_, l, r)| (l, r)).collect()
}

fn pair_score(local: CandidateKind, remote: CandidateKind) -> u8 {
    match (local, remote) {
        (CandidateKind::Host, CandidateKind::Host) => 0,
        (CandidateKind::Host, CandidateKind::ServerReflexive)
        | (CandidateKind::ServerReflexive, CandidateKind::Host) => 1,
        (CandidateKind::ServerReflexive, CandidateKind::ServerReflexive) => 2,
        (CandidateKind::Relay, _) | (_, CandidateKind::Relay) => 3,
    }
}

mod candidate {
    use std::net::SocketAddr;

    /// A single local or reflexive transport candidate.
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct Candidate {
        pub addr: SocketAddr,
        pub kind: CandidateKind,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    pub enum CandidateKind {
        Host,
        ServerReflexive,
        Relay,
    }

    /// Gather host candidates for a fixed local port.
    ///
    /// This is the synchronous variant used by callers that do not yet have a
    /// bound socket. It enumerates local network interfaces and pairs each IPv4
    /// address with `port`.
    pub fn gather_host_candidates(port: u16) -> Vec<Candidate> {
        let mut candidates = Vec::new();
        match if_addrs::get_if_addrs() {
            Ok(interfaces) => {
                for iface in interfaces {
                    let ip = match iface.addr.ip() {
                        std::net::IpAddr::V4(ip) => ip,
                        _ => continue,
                    };
                    let addr = SocketAddr::new(std::net::IpAddr::V4(ip), port);
                    candidates.push(Candidate {
                        addr,
                        kind: CandidateKind::Host,
                    });
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to enumerate network interfaces");
            }
        }

        if candidates.is_empty() {
            candidates.push(Candidate {
                addr: ([127, 0, 0, 1], port).into(),
                kind: CandidateKind::Host,
            });
        }

        candidates
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u8) -> lanlink_proto::NodeId {
        let mut b = [0u8; 8];
        b[7] = id;
        lanlink_proto::NodeId(b)
    }

    fn make_session() -> lanlink_crypto::StaticSession {
        lanlink_crypto::StaticSession::new([0u8; 32])
    }

    #[test]
    fn candidate_pair_prefers_lan() {
        let local = vec![
            Candidate {
                addr: "192.168.1.10:9000".parse().unwrap(),
                kind: CandidateKind::Host,
            },
            Candidate {
                addr: "203.0.113.5:9000".parse().unwrap(),
                kind: CandidateKind::ServerReflexive,
            },
        ];
        let remote = vec![
            Candidate {
                addr: "192.168.1.20:9000".parse().unwrap(),
                kind: CandidateKind::Host,
            },
            Candidate {
                addr: "198.51.100.7:9000".parse().unwrap(),
                kind: CandidateKind::ServerReflexive,
            },
        ];
        let pairs = build_candidate_pairs(&local, &remote);
        assert_eq!(
            pairs[0],
            (
                "192.168.1.10:9000".parse().unwrap(),
                "192.168.1.20:9000".parse().unwrap()
            )
        );
    }

    #[test]
    fn pair_score_orders_correctly() {
        assert_eq!(pair_score(CandidateKind::Host, CandidateKind::Host), 0);
        assert_eq!(
            pair_score(CandidateKind::Host, CandidateKind::ServerReflexive),
            1
        );
        assert_eq!(
            pair_score(
                CandidateKind::ServerReflexive,
                CandidateKind::ServerReflexive
            ),
            2
        );
        assert_eq!(pair_score(CandidateKind::Relay, CandidateKind::Host), 3);
    }

    #[test]
    fn host_candidates_include_requested_port() {
        let candidates = gather_host_candidates(12345);
        assert!(
            candidates.iter().any(|c| c.addr.port() == 12345),
            "candidates should include the requested port"
        );
        assert!(candidates.iter().all(|c| c.kind == CandidateKind::Host));
    }

    #[test]
    fn unspecified_addresses_detected() {
        assert!(is_unspecified("0.0.0.0:9000".parse().unwrap()));
        assert!(!is_unspecified("127.0.0.1:9000".parse().unwrap()));
    }

    #[tokio::test]
    async fn punch_probe_reply_roundtrip() {
        let local_a = node(1);
        let local_b = node(2);

        let socket_a = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let socket_b = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let _addr_a = socket_a.local_addr().unwrap();
        let addr_b = socket_b.local_addr().unwrap();

        let session_a = make_session();
        let session_b = make_session();

        let puncher_a = Puncher::new(local_a, socket_a.clone(), session_a.clone(), vec![]);
        let puncher_b = Puncher::new(local_b, socket_b.clone(), session_b.clone(), vec![]);

        // A manually sends an authenticated PUNCH_PROBE to B.
        let cookie = 0xdeadbeef_u64;
        let mut probe_inner = Vec::with_capacity(lanlink_proto::NODE_ID_LEN + 8);
        probe_inner.extend_from_slice(&local_a.0);
        probe_inner.extend_from_slice(&cookie.to_be_bytes());
        let probe = session_a
            .seal(
                local_b,
                lanlink_proto::PUNCH_PROBE_PACKET_TYPE,
                &probe_inner,
            )
            .unwrap();
        socket_a.send_to(&probe, addr_b).await.unwrap();

        // B receives the probe and replies.
        let mut buf = [0u8; 512];
        let (n, from) = socket_b.recv_from(&mut buf).await.unwrap();
        let handled = puncher_b
            .on_punch_probe(from, &buf[..n])
            .await
            .expect("probe decode");
        assert!(handled);

        // A receives the reply and reports the verified path.
        let (n, from) = socket_a.recv_from(&mut buf).await.unwrap();
        let path = puncher_a
            .on_punch_reply(from, &buf[..n])
            .expect("reply decode")
            .expect("reply accepted");
        assert_eq!(path.peer, local_b);
        assert_eq!(path.addr, addr_b);
    }
}
