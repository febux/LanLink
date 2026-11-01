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

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
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
    pub peer: lanlink_types::NodeId,
    pub addr: SocketAddr,
}

/// Hole-punching engine tied to a single UDP socket and network key.
pub struct Puncher {
    local_node: lanlink_types::NodeId,
    socket: Arc<UdpSocket>,
    session: lanlink_crypto::StaticSession,
    stun_servers: Vec<SocketAddr>,
    /// Expected PUNCH_REPLY cookies keyed by the peer we are currently trying
    /// to reach. Keeping the cookie per-peer allows the daemon to punch
    /// several peers concurrently through the same `Puncher` while still
    /// rejecting stale or reflected replies for each individual peer.
    expected_cookies: Mutex<HashMap<lanlink_types::NodeId, u64>>,
}

/// Removes the expected cookie for a peer when a punch attempt goes out of
/// scope, so stale cookies do not accumulate.
struct CookieGuard<'a> {
    cookies: &'a Mutex<HashMap<lanlink_types::NodeId, u64>>,
    peer: lanlink_types::NodeId,
}

impl<'a> Drop for CookieGuard<'a> {
    fn drop(&mut self) {
        let _ = self.cookies.lock().unwrap().remove(&self.peer);
    }
}

impl Puncher {
    /// Create a puncher for `local_node` using the shared `socket`. The same
    /// `session` must be shared with the switch so probes and data are mutually
    /// authenticated.
    pub fn new(
        local_node: lanlink_types::NodeId,
        socket: Arc<UdpSocket>,
        session: lanlink_crypto::StaticSession,
        stun_servers: Vec<SocketAddr>,
    ) -> Self {
        Self {
            local_node,
            socket,
            session,
            stun_servers,
            expected_cookies: Mutex::new(HashMap::new()),
        }
    }

    /// Discover host and server-reflexive candidates for the local socket.
    ///
    /// The returned list always includes the local socket's bound address as a
    /// host candidate, followed by one server-reflexive candidate per reachable
    /// STUN server. If STUN fails, host candidates are still returned so LAN and
    /// port-forwarded scenarios continue to work.
    pub async fn gather_candidates(&self) -> Vec<Candidate> {
        let mut candidates = candidate::gather_host_candidates_for_socket(&self.socket);

        for stun_server in &self.stun_servers {
            match stun_binding(&self.socket, *stun_server, None).await {
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
        peer: lanlink_types::NodeId,
        remote_candidates: Vec<Candidate>,
    ) -> Result<VerifiedPath> {
        let local_candidates = candidate::gather_host_candidates_for_socket(&self.socket);

        // If the peer is on the same host, host candidates usually win.
        let pairs = build_candidate_pairs(&local_candidates, &remote_candidates);
        if pairs.is_empty() {
            return Err(PunchError::NoPair);
        }

        // Use a single cookie for the whole attempt so that a stale reply from
        // an earlier burst cannot be mistaken for the current one. Store it
        // per-peer so concurrent punches do not clobber each other's cookies.
        let cookie = rand::random::<u64>();
        self.expected_cookies.lock().unwrap().insert(peer, cookie);
        let _guard = CookieGuard {
            cookies: &self.expected_cookies,
            peer,
        };

        let mut ticker = interval(PROBE_INTERVAL);
        let deadline = Instant::now() + PUNCH_TIMEOUT;

        // Burst probes at the start, then periodically.
        self.fire_probe_burst(peer, &pairs, PROBES_PER_BURST, cookie)
            .await?;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }

            tokio::select! {
                _ = ticker.tick() => {
                    self.fire_probe_burst(peer, &pairs, PROBES_PER_BURST, cookie).await?;
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
        if receiver != self.local_node || ptype != lanlink_types::PUNCH_PROBE_PACKET_TYPE {
            return Ok(false);
        }
        if inner.len() < lanlink_types::NODE_ID_LEN + 8 {
            return Ok(false);
        }

        let mut sender_bytes = [0u8; lanlink_types::NODE_ID_LEN];
        sender_bytes.copy_from_slice(&inner[..lanlink_types::NODE_ID_LEN]);
        let peer = lanlink_types::NodeId(sender_bytes);

        let mut cookie_bytes = [0u8; 8];
        cookie_bytes
            .copy_from_slice(&inner[lanlink_types::NODE_ID_LEN..lanlink_types::NODE_ID_LEN + 8]);

        let reply_inner = self.build_punch_reply_inner(peer, cookie_bytes);
        let reply =
            self.session
                .seal(peer, lanlink_types::PUNCH_REPLY_PACKET_TYPE, &reply_inner)?;
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
        if receiver != self.local_node || ptype != lanlink_types::PUNCH_REPLY_PACKET_TYPE {
            return Ok(None);
        }
        if inner.len() < lanlink_types::NODE_ID_LEN + 8 {
            return Ok(None);
        }

        let mut sender_bytes = [0u8; lanlink_types::NODE_ID_LEN];
        sender_bytes.copy_from_slice(&inner[..lanlink_types::NODE_ID_LEN]);
        let peer = lanlink_types::NodeId(sender_bytes);

        let mut cookie_bytes = [0u8; 8];
        cookie_bytes
            .copy_from_slice(&inner[lanlink_types::NODE_ID_LEN..lanlink_types::NODE_ID_LEN + 8]);
        let cookie = u64::from_be_bytes(cookie_bytes);

        // Accept the reply only if we are currently punching this peer and the
        // echoed cookie matches the one we sent. This prevents stale or
        // reflected PUNCH_REPLY packets from being accepted as valid paths.
        let expected = {
            let cookies = self.expected_cookies.lock().unwrap();
            cookies.get(&peer).copied()
        };
        if expected != Some(cookie) {
            return Ok(None);
        }
        self.expected_cookies.lock().unwrap().remove(&peer);

        // The address the reply actually came from is the working path, not
        // necessarily the candidate we sent the probe to.
        Ok(Some(VerifiedPath { peer, addr: from }))
    }

    /// Build and send one authenticated PUNCH_PROBE to a candidate address.
    async fn send_probe(
        &self,
        peer: lanlink_types::NodeId,
        target: SocketAddr,
        cookie: u64,
    ) -> Result<()> {
        let mut inner = Vec::with_capacity(lanlink_types::NODE_ID_LEN + 8);
        inner.extend_from_slice(&self.local_node.0);
        inner.extend_from_slice(&cookie.to_be_bytes());

        let packet = self
            .session
            .seal(peer, lanlink_types::PUNCH_PROBE_PACKET_TYPE, &inner)?;
        self.socket.send_to(&packet, target).await?;
        Ok(())
    }

    /// Build the inner plaintext for a PUNCH_REPLY.
    fn build_punch_reply_inner(&self, _to: lanlink_types::NodeId, cookie: [u8; 8]) -> Vec<u8> {
        let mut inner = Vec::with_capacity(lanlink_types::NODE_ID_LEN + 8);
        inner.extend_from_slice(&self.local_node.0);
        inner.extend_from_slice(&cookie);
        inner
    }

    /// Fire a small number of probes at distinct candidate pairs.
    async fn fire_probe_burst(
        &self,
        peer: lanlink_types::NodeId,
        pairs: &[(SocketAddr, SocketAddr)],
        count: usize,
        cookie: u64,
    ) -> Result<()> {
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
///
/// `integrity_password` is used to verify `MESSAGE-INTEGRITY` when the STUN
/// server includes it in the response. Public STUN servers do not use it, but
/// verifying it when present prevents off-path attackers from injecting
/// forged reflexive addresses.
async fn stun_binding(
    socket: &UdpSocket,
    stun_server: SocketAddr,
    integrity_password: Option<&str>,
) -> Result<SocketAddr> {
    use stun::agent::TransactionId;
    use stun::attributes::{ATTR_FINGERPRINT, ATTR_MESSAGE_INTEGRITY};
    use stun::fingerprint::FINGERPRINT;
    use stun::integrity::MessageIntegrity;
    use stun::message::{Getter, Message, BINDING_REQUEST, BINDING_SUCCESS};
    use stun::xoraddr::XorMappedAddress;

    let mut msg = Message::new();
    msg.build(&[Box::<TransactionId>::default(), Box::new(BINDING_REQUEST)])
        .map_err(|e| PunchError::Stun(e.to_string()))?;

    let transaction_id = msg.transaction_id;
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
    resp.raw.clear();
    resp.raw.extend_from_slice(&buf[..n]);
    resp.decode().map_err(|e| PunchError::Stun(e.to_string()))?;

    if resp.transaction_id != transaction_id {
        return Err(PunchError::Stun("STUN transaction id mismatch".to_string()));
    }

    if resp.typ != BINDING_SUCCESS {
        return Err(PunchError::Stun(
            "not a binding success response".to_string(),
        ));
    }

    if resp.contains(ATTR_FINGERPRINT) {
        FINGERPRINT
            .check(&resp)
            .map_err(|e| PunchError::Stun(format!("STUN fingerprint check failed: {e}")))?;
    }

    if resp.contains(ATTR_MESSAGE_INTEGRITY) {
        match integrity_password {
            Some(password) => {
                MessageIntegrity::new_short_term_integrity(password.to_string())
                    .check(&mut resp)
                    .map_err(|e| PunchError::Stun(format!("STUN integrity check failed: {e}")))?;
            }
            None => {
                return Err(PunchError::Stun(
                    "STUN MESSAGE-INTEGRITY present but no credentials configured".to_string(),
                ));
            }
        }
    }

    let mut xor_addr = XorMappedAddress::default();
    xor_addr
        .get_from(&resp)
        .map_err(|e| PunchError::Stun(e.to_string()))?;

    Ok(SocketAddr::new(xor_addr.ip, xor_addr.port))
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

fn is_unspecified(addr: SocketAddr) -> bool {
    match addr.ip() {
        std::net::IpAddr::V4(ip) => ip.is_unspecified(),
        std::net::IpAddr::V6(ip) => ip.is_unspecified(),
    }
}

mod candidate {
    use super::is_unspecified;
    use std::net::SocketAddr;
    use tokio::net::UdpSocket;

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

    /// Source of the local port when gathering host candidates.
    enum HostCandidateSource<'a> {
        /// A fixed port for callers that do not yet have a bound socket.
        Port(u16),
        /// A bound UDP socket; its local address is also advertised if specific.
        Socket(&'a UdpSocket),
    }

    impl HostCandidateSource<'_> {
        fn resolve(&self) -> Option<(u16, Option<SocketAddr>)> {
            match self {
                HostCandidateSource::Port(port) => Some((*port, None)),
                HostCandidateSource::Socket(socket) => match socket.local_addr() {
                    Ok(addr) => Some((addr.port(), Some(addr))),
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to get local socket address");
                        None
                    }
                },
            }
        }
    }

    /// Gather host candidates from `source`.
    ///
    /// For a bound socket the local address is advertised first (when it is
    /// specific), followed by every local interface IPv4 address paired with the
    /// port.  For a bare port only the interface addresses are used.  If no
    /// addresses are found, a loopback candidate is added as a fallback for
    /// same-machine testing.
    fn collect_host_candidates(source: HostCandidateSource<'_>) -> Vec<Candidate> {
        let mut candidates = Vec::new();

        let (port, bound) = match source.resolve() {
            Some(v) => v,
            None => return candidates,
        };

        if let Some(addr) = bound {
            if !is_unspecified(addr) {
                candidates.push(Candidate {
                    addr,
                    kind: CandidateKind::Host,
                });
            }
        }

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

    /// Gather host candidates for a fixed local port.
    ///
    /// This is the synchronous variant used by callers that do not yet have a
    /// bound socket. It enumerates local network interfaces and pairs each IPv4
    /// address with `port`.
    pub fn gather_host_candidates(port: u16) -> Vec<Candidate> {
        collect_host_candidates(HostCandidateSource::Port(port))
    }

    /// Gather host candidates for a bound UDP socket.
    pub(super) fn gather_host_candidates_for_socket(socket: &UdpSocket) -> Vec<Candidate> {
        collect_host_candidates(HostCandidateSource::Socket(socket))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u8) -> lanlink_types::NodeId {
        let mut b = [0u8; 8];
        b[7] = id;
        lanlink_types::NodeId(b)
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

        let puncher_a = Arc::new(Puncher::new(
            local_a,
            socket_a.clone(),
            session_a.clone(),
            vec![],
        ));
        let puncher_b = Arc::new(Puncher::new(
            local_b,
            socket_b.clone(),
            session_b.clone(),
            vec![],
        ));

        // Start A's punch attempt so the expected cookie is registered.
        let punch_task = tokio::spawn({
            let puncher_a = puncher_a.clone();
            let remote = vec![Candidate {
                addr: addr_b,
                kind: CandidateKind::Host,
            }];
            async move { puncher_a.punch_peer(local_b, remote).await }
        });

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

        punch_task.abort();
    }

    #[tokio::test]
    async fn stale_punch_reply_is_ignored() {
        let local_a = node(1);
        let local_b = node(2);

        let socket_a = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let socket_b = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let addr_a = socket_a.local_addr().unwrap();
        let addr_b = socket_b.local_addr().unwrap();

        let session_a = make_session();
        let session_b = make_session();

        let puncher_a = Arc::new(Puncher::new(
            local_a,
            socket_a.clone(),
            session_a.clone(),
            vec![],
        ));
        let puncher_b = Arc::new(Puncher::new(
            local_b,
            socket_b.clone(),
            session_b.clone(),
            vec![],
        ));

        // B sends a PUNCH_REPLY, but A never started a punch attempt, so
        // there is no expected cookie and the reply must be ignored.
        let stale_cookie = 0xbadc0ffee_u64;
        let reply_inner = puncher_b.build_punch_reply_inner(local_a, stale_cookie.to_be_bytes());
        let reply = session_b
            .seal(
                local_a,
                lanlink_types::PUNCH_REPLY_PACKET_TYPE,
                &reply_inner,
            )
            .unwrap();
        socket_b.send_to(&reply, addr_a).await.unwrap();

        let mut buf = [0u8; 512];
        let (n, from) = socket_a.recv_from(&mut buf).await.unwrap();
        assert_eq!(from, addr_b);
        assert!(puncher_a.on_punch_reply(from, &buf[..n]).unwrap().is_none());
    }

    #[tokio::test]
    async fn wrong_cookie_punch_reply_is_ignored() {
        let local_a = node(1);
        let local_b = node(2);

        let socket_a = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let socket_b = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let addr_a = socket_a.local_addr().unwrap();
        let addr_b = socket_b.local_addr().unwrap();

        let session_a = make_session();
        let session_b = make_session();

        let puncher_a = Arc::new(Puncher::new(
            local_a,
            socket_a.clone(),
            session_a.clone(),
            vec![],
        ));
        let puncher_b = Arc::new(Puncher::new(
            local_b,
            socket_b.clone(),
            session_b.clone(),
            vec![],
        ));

        // Start A's punch attempt so a cookie is registered for B.
        let punch_task = tokio::spawn({
            let puncher_a = puncher_a.clone();
            let remote = vec![Candidate {
                addr: addr_b,
                kind: CandidateKind::Host,
            }];
            async move { puncher_a.punch_peer(local_b, remote).await }
        });

        // Instead of replying to the probe, B sends a reply with a wrong
        // cookie. A must ignore it because it does not match the expected one.
        let wrong_cookie = 0xdeadbeef_u64;
        let reply_inner = puncher_b.build_punch_reply_inner(local_a, wrong_cookie.to_be_bytes());
        let reply = session_b
            .seal(
                local_a,
                lanlink_types::PUNCH_REPLY_PACKET_TYPE,
                &reply_inner,
            )
            .unwrap();
        socket_b.send_to(&reply, addr_a).await.unwrap();

        let mut buf = [0u8; 512];
        let (n, from) = socket_a.recv_from(&mut buf).await.unwrap();
        assert_eq!(from, addr_b);
        assert!(puncher_a.on_punch_reply(from, &buf[..n]).unwrap().is_none());

        punch_task.abort();
    }

    #[tokio::test]
    async fn stun_binding_rejects_wrong_transaction_id() {
        use stun::message::{Message, Setter, BINDING_SUCCESS};
        use stun::xoraddr::XorMappedAddress;

        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();

        let server_task = tokio::spawn(async move {
            let mut buf = [0u8; 512];
            let (n, from) = server.recv_from(&mut buf).await.unwrap();

            // Decode request so we can mirror its transaction id for the
            // XorMappedAddress XOR mask. Then deliberately flip one byte of
            // the id in the response to simulate an off-path spoofed packet.
            let mut req = Message::new();
            req.raw.clear();
            req.raw.extend_from_slice(&buf[..n]);
            req.decode().unwrap();

            let mut bad_id = req.transaction_id;
            bad_id.0[0] ^= 0xff;

            let mut resp = Message::new();
            resp.build(&[Box::new(BINDING_SUCCESS), Box::new(bad_id)])
                .unwrap();

            let xor_addr = XorMappedAddress {
                ip: "203.0.113.1".parse().unwrap(),
                port: 12345,
            };
            xor_addr.add_to(&mut resp).unwrap();

            server.send_to(&resp.raw, from).await.unwrap();
        });

        let result = stun_binding(&client, server_addr, None).await;
        assert!(
            matches!(result, Err(PunchError::Stun(ref s)) if s.contains("transaction id mismatch")),
            "expected transaction id mismatch error, got {result:?}"
        );

        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn stun_binding_accepts_valid_response() {
        use stun::message::{Message, Setter, BINDING_SUCCESS};
        use stun::xoraddr::XorMappedAddress;

        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();

        let server_task = tokio::spawn(async move {
            let mut buf = [0u8; 512];
            let (n, from) = server.recv_from(&mut buf).await.unwrap();

            let mut req = Message::new();
            req.raw.clear();
            req.raw.extend_from_slice(&buf[..n]);
            req.decode().unwrap();

            let mut resp = Message::new();
            resp.build(&[Box::new(BINDING_SUCCESS), Box::new(req.transaction_id)])
                .unwrap();

            let expected_addr: SocketAddr = "203.0.113.1:12345".parse().unwrap();
            let xor_addr = XorMappedAddress {
                ip: expected_addr.ip(),
                port: expected_addr.port(),
            };
            xor_addr.add_to(&mut resp).unwrap();

            server.send_to(&resp.raw, from).await.unwrap();
            expected_addr
        });

        let result = stun_binding(&client, server_addr, None).await.unwrap();
        let expected_addr = server_task.await.unwrap();
        assert_eq!(result, expected_addr);
    }
}
