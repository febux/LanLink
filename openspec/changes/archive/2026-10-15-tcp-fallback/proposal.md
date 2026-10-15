# Proposal

## Why

LanLink's NAT-traversal originally had UDP only (direct, hole punch, relay). When UDP is blocked or punching fails against symmetric NATs, peers could not connect at all. Radmin VPN supports a TCP fallback matrix; LanLink needed to match it.

## What Changes

- Add a TCP listener to `lanlinkd` accepting length-prefixed encrypted LanLink frames, advertised via `PeerInfo.tcp_candidates`.
- Add a `TcpRelayServer` (TCP mode of the relay path) with node-id→stream forwarding; `JoinNetworkResponse` carries `tcp_relay_servers`.
- Extend the switch/daemon transport path so TCP packets flow through the same `session.open()` decryption, with per-peer path classification (Direct/Relay × UDP/TCP) surfaced in the status API.

## Capabilities

TCP direct + TCP relay fallback shipped (git `feat(tcp,noise): implement TCP direct/relay fallback and per-device Noise identity`); the Teredo/IPv6 item was deferred and now lives in the active `windows-parity`/future work. Archived historical change, `skip_specs: true`.

### New Capabilities

- None.

### Modified Capabilities

- None.

## Impact

- `lanlink-proto` (candidate/relay fields), `lanlink-relay` (TCP mode), `lanlinkd` (listener, receive loop), `lanlink-punch` (TCP connect fallback), status API (path classification).
