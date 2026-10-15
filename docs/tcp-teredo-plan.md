# TCP and Teredo fallback plan

## Goal

Improve LanLink's NAT-traversal resilience so that peers can still connect when UDP is
blocked or when UDP hole punching fails. Radmin VPN supports direct TCP, TCP over NAT,
Teredo, and TCP relay; LanLink should eventually match that matrix.

## Current state

- LanLink uses a single UDP socket for data, keepalives, pings, and NAT hole punching.
- If direct UDP fails, the daemon falls back to a UDP relay server (`lanlink-relayd`).
- There is no TCP transport at all.

## Desired transport matrix

| Path | Today | Planned |
|---|---|---|
| Direct UDP | ✅ | ✅ |
| UDP hole punch | ✅ | ✅ |
| UDP relay | ✅ | ✅ |
| Direct TCP | ❌ | 🚧 |
| TCP relay | ❌ | 🚧 |
| Teredo / IPv6 | ❌ | ⏳ |

## Architecture

### 1. TCP direct fallback

Each daemon opens a TCP listener on a configurable port (can be the same as the UDP port
if the OS allows, or a dedicated port). The listener accepts length-prefixed encrypted
LanLink frames:

```text
[length: u16 BE]
[LanLink outer packet: version, type, receiver id, nonce + ciphertext]
```

On the first accepted connection, the remote peer is identified from the inner plaintext
sender id once a packet is decrypted. The connection is then associated with that peer id.

`PeerInfo` gains a `tcp_candidates: Vec<SocketAddr>` field. The puncher tries these after
UDP hole punching fails.

### 2. TCP relay fallback

A new `TcpRelayServer` (either a second mode of `lanlink-relayd` or a separate binary)
listens on TCP. Clients connect and send a relay bind message (same wire format as the UDP
relay bind, length-prefixed). The server maintains `node_id -> TcpStream` mappings and
forwards length-prefixed LanLink packets using the receiver id in the outer header.

`JoinNetworkResponse` gains `tcp_relay_servers: Vec<SocketAddr>`. The daemon connects to
the first reachable one and keeps the connection alive with periodic binds.

### 3. Switch integration

The switch currently produces `Vec<(SocketAddr, Vec<u8>)>` for outgoing frames and expects
UDP datagrams in `on_udp_packet`. To support TCP:

- Add a transport abstraction so the switch can emit either UDP datagrams or TCP-stream
  packets.
- Receiving: TCP packets are decrypted through the same `session.open()` path, but they
  arrive from a `TcpStream` task instead of the UDP socket.
- Per-peer `path` classification: Direct UDP, Relay UDP, Direct TCP, Relay TCP. The daemon
  status API already exposes `direct`/`relay`; it should be extended with a `tcp` variant.

### 4. Teredo / IPv6

Teredo is an IPv6 transition technology that tunnels IPv6 over UDP through NAT. Implementing
a full Teredo client in Rust is a multi-week project. The practical interim step is to use
IPv6 when both peers have public or NAT-mapped IPv6 addresses; Teredo can be added later for
IPv4-only peers behind restrictive NATs.

## Protocol changes

- `JoinNetworkRequest` / `PresenceUpdate`: add `tcp_candidates: Vec<SocketAddr>` and
  `tcp_relay_candidates: Vec<SocketAddr>`.
- `JoinNetworkResponse`: add `tcp_relay_servers: Vec<SocketAddr>`.
- `PeerInfo`: add `tcp_candidates` and `tcp_relay_candidates`.
- New wire constants for TCP relay bind/keepalive (or reuse UDP relay constants inside the
  length-prefixed framing).

## Implementation order

1. Add protocol fields (non-breaking; serde defaults keep old clients compatible).
2. Implement `TcpRelayServer` in `lanlink-relay`.
3. Add TCP listener to `lanlinkd` and advertise TCP candidates.
4. Add TCP connect fallback in `lanlink-punch`.
5. Wire TCP receive path into the daemon main loop.
6. Extend the daemon status API with TCP path classification.
7. Add a minimal Teredo/IPv6 discovery phase (future work).

## Testing

- Two-Linux test with UDP artificially blocked (iptables drop) should fall back to TCP relay.
- Linux ↔ Windows test should use whatever path both sides support.
- Simulate symmetric NAT where UDP hole punching fails; verify TCP direct or TCP relay.
