# Proposal

## Why

The `coordinator` crate currently combines two very different jobs: (1) a
persistent, authenticated control plane for network membership, directory, and
telemetry, and (2) a lightweight, high-churn signaling plane where peers
publish ICE-like candidates and discover each other for NAT hole punching.
This forces the heavy SQLite-backed coordinator to sit on the hot path of every
30-second candidate refresh from every peer, and it gives the content-blind
relay (`lanlink-relayd`) no way to expose operational health or traffic metrics.

NetBird already demonstrates the shape we want: a small, stateless `signal/`
service for peer presence and candidate exchange, separate `relay/` and
`signal/` services with healthcheck and metrics endpoints, and a heavy
`management/` plane that does not touch the data path. Splitting LanLink along
the same seam will let the coordinator scale with networks rather than peers,
let operators run signal/relay edge services with standard observability, and
open the door to WebSocket/push presence updates later.

## What Changes

- **New lightweight `lanlink-signal` service** that owns peer presence,
  candidate exchange, and relay-URL advertisement. It is stateful only in
  memory (per-network peer registries with TTL) and exposes a small REST/WebSocket
  API.
- **Coordinator slim-down**: remove candidate exchange from
  `POST /networks/{id}/join` and `POST /networks/{id}/candidates`. The
  coordinator keeps network CRUD, `/directory`, `/telemetry`, and `/metrics`,
  and continues to advertise relay server URLs so networks can be created
  without a separate signal config.
- **Daemon and relay config additions**: `lanlinkd` TOML/CLI gets a `signal`
  URL; `lanlink-relayd` gets an optional metrics HTTP listen address separate
  from the data-plane sockets.
- **Health and metrics endpoints on edge services**: both `lanlink-signal` and
  `lanlink-relayd` expose `GET /health` and `GET /metrics` (Prometheus text or
  plain-JSON counters, matching the existing coordinator `/metrics` JSON style).
  Metrics cover peers/sessions, relayed packets and bytes over UDP+TCP, and
  active mappings/bindings.
- **systemd unit** for `lanlink-signal` mirroring the existing
  `lanlink-coordinator.service` sandboxing.
- **BREAKING**: after a migration window, the coordinator will stop accepting
  `POST /networks/{id}/candidates`. Old daemons must either be upgraded to talk
  to the signal service or run in a back-compat mode where the coordinator
  proxies candidate traffic to the signal service.

## Capabilities

### New Capabilities

- `signal-plane`: a standalone, lightweight signaling service for peer presence,
  candidate exchange, and relay-URL advertisement. It is separate from the
  coordinator and must remain cheap enough to run as an edge service.
- `edge-service-observability`: standard `GET /health` and `GET /metrics`
  endpoints on `lanlink-signal` and `lanlink-relayd`, using the same JSON
  counter style as the existing coordinator `/metrics` endpoint.
- `coordinator-plane`: the coordinator keeps network membership/directory and
  telemetry/metrics aggregation, but no longer handles peer candidate exchange
  or real-time peer lists. The candidate endpoints are removed from the
  coordinator API contract.
- `lanlinkd`: the daemon learns a signal service URL from config/CLI and routes
  candidate publication and peer discovery through that URL instead of the
  coordinator. The daemon must continue to join networks through the
  coordinator while discovering peers via the signal plane.

### Modified Capabilities

None. The project currently has only one spec (`windows-gateway`), which is
unrelated to this change.

## Impact

- New crate: `crates/lanlink-signal` with binary `lanlink-signald`.
- Modified crates: `crates/coordinator` (remove candidate exchange),
  `crates/lanlinkd` (signal URL config + candidate refresh wiring),
  `crates/lanlink-relay` (HTTP health/metrics listener + counters),
  `crates/lanlink-proto` (new signal-plane message types and possibly a
  shared `/metrics` response shape).
- New systemd unit: `dist/systemd/lanlink-signal.service`.
- Deployment changes: operators must deploy `lanlink-signald` alongside the
  coordinator and relay; existing `lanlinkd` configs need a `signal` URL before
  the coordinator candidate endpoints are removed.
