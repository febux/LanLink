# Design

## Context

- `crates/coordinator/src/api.rs` currently combines network CRUD, directory,
  telemetry, metrics aggregation, and candidate exchange in one Axum app. The
  `AppState` carries SQLite `pool`, `relay_servers`, and `tcp_relay_servers`.
- `crates/lanlinkd/src/main.rs` joins through `POST /networks/{id}/join`, then
  refreshes candidates through `POST /networks/{id}/candidates` every 30 s in
  `refresh_candidates_and_punch`. It binds to UDP/TCP relays and stores peer
  state in the switch and `status::StatusRegistry`.
- `crates/lanlink-relay/src/lib.rs` implements a content-blind UDP relay and
  `crates/lanlink-relay/src/tcp.rs` implements a content-blind TCP relay. Both
  track active bindings/mappings in memory but expose no HTTP surface.
- Existing systemd units in `dist/systemd/` (`lanlink-coordinator.service`,
  `lanlink-relayd.service`, `lanlinkd@.service`) already use `ProtectSystem=strict`,
  capability bounding, and user `lanlink`.
- `crates/lanlink-proto/src/lib.rs` defines `JoinNetworkRequest`,
  `JoinNetworkResponse`, `PeerInfo`, `PresenceUpdate`, `MemberEvent`, and relay
  wire constants. `crates/lanlink-proto/src/telemetry.rs` defines
  `AggregatedMetrics` returned by the coordinator's `GET /metrics`.

See `proposal.md` for motivation and scope.

## Goals / Non-Goals

**Goals:**
- Extract real-time presence/candidate exchange into a new `lanlink-signal`
  crate/service that is lightweight and horizontally scalable.
- Add `GET /health` and `GET /metrics` to `lanlink-signal` and `lanlink-relayd`,
  reusing the JSON counter style used by the coordinator.
- Keep the coordinator focused on membership, directory, telemetry, and relay-URL
  advertisement.
- Preserve a migration path for existing `lanlinkd` deployments that still
  point only at a coordinator URL.
- Ship a systemd unit for `lanlink-signald` with the same sandboxing posture as
  the coordinator.

**Non-Goals:**
- Replacing the SQLite-backed coordinator or changing telemetry aggregation.
- Adding authentication beyond the existing network password/token model.
- Changing the data-plane wire format, relay forwarding logic, or encryption.
- WebSocket push notifications for presence updates in this change (the API
  shape should leave room for them later).
- Prometheus exposition format for `/metrics` (keep JSON for consistency).

## Decisions

### 1. New crate `crates/lanlink-signal` with binary `lanlink-signald`

**Rationale:** A separate crate keeps the signal service buildable and
deployable independently of the coordinator. It mirrors NetBird's `signal/`
layout and lets operators run signal nodes close to peers without dragging in
SQLite/sqlx.

**Scope of the crate:**
- `src/main.rs`: CLI (`--listen`, `--relay-udp-url`, `--relay-tcp-url`, optional
  `--coordinator-url` for relay discovery, `--metrics-listen`).
- `src/api.rs`: Axum routes (`/health`, `/metrics`, `/networks/{id}/peers`,
  `/networks/{id}/register` or a combined presence endpoint).
- `src/state.rs`: in-memory `Mutex<HashMap<u64, NetworkPeers>>` with TTL.
- No database; state rebuilds from peer re-registration after restart.

### 2. Candidate exchange moves from coordinator to signal service

**Rationale:** The coordinator should not be on the hot path of 30-second peer
refreshes. The signal service is stateful-in-memory only and cheap to restart.

**Mapping of old endpoints:**
- `POST /networks/{id}/candidates` (coordinator) → `POST /networks/{id}/peers`
  (signal). The request body remains `lanlink_proto::PresenceUpdate`.
- The response remains `Vec<PeerInfo>`. The signal service keeps the same
  `PeerInfo` shape so `lanlink_switch` and `status.rs` need no changes.
- The join response from the coordinator gains a `signal_url` field. The daemon
  uses that URL for all subsequent candidate operations.

### 3. `/health` and `/metrics` on a separate HTTP listener for edge services

**Rationale:** Mixing HTTP scraping with the UDP/TCP data plane on the same
socket complicates sandboxing and can interfere with forwarding latency. A
separate HTTP listener keeps concerns separated and lets `lanlink-relayd` add
scraping without changing relay bind behavior.

**Choices:**
- `lanlink-signald` HTTP listener defaults to `0.0.0.0:8081` and can be disabled
  with `--metrics-listen none`.
- `lanlink-relayd` adds `--metrics-listen 0.0.0.0:9090` (off by default to keep
  the change safe for existing deployments; enable by default in a later
  release).
- Both services expose `/health` (HTTP 200/503) and `/metrics` (JSON).

### 4. Metrics JSON shape consistent with coordinator

**Rationale:** Operators already consume the coordinator `GET /metrics` JSON
response. Keeping the same format reduces tooling fragmentation.

**Example response for `lanlink-signald`:**
```json
{
  "service": "lanlink-signal",
  "version": "0.6.1",
  "uptime_seconds": 3600,
  "networks": 3,
  "peers": 12
}
```

**Example response for `lanlink-relayd`:**
```json
{
  "service": "lanlink-relay",
  "version": "0.6.1",
  "uptime_seconds": 3600,
  "udp_bindings": 8,
  "tcp_sessions": 2,
  "udp_packets_relayed": 1234567,
  "udp_bytes_relayed": 2345678901,
  "tcp_packets_relayed": 98765,
  "tcp_bytes_relayed": 123456789
}
```

Both responses stay aggregate and avoid PII. The types can live in
`lanlink-proto` under a new `metrics` module or remain local to each crate; the
latter is preferred to avoid coupling edge-service internals to the protocol
crate.

### 5. Authentication model for signal endpoints

**Rationale:** We cannot require a new token model in this change without
updating every client and config. Reusing the network password (supplied at
join time) is the smallest viable migration.

**Approach:**
- The daemon passes the network password in a header or request field when
  registering/fetching peers from the signal service. The signal service
  validates it by asking the coordinator (or by caching a hash) on first use.
- In the migration window, the coordinator can proxy legacy candidate traffic
  to the signal service so old daemons work without a signal URL.

### 6. Coordinator keeps relay-URL advertisement

**Rationale:** Networks are created by the coordinator, which already knows the
relay addresses from CLI args. Passing relay URLs into the signal service
(either via a coordinator callback or startup flags) avoids forcing operators
to configure relays twice.

**Approach:**
- `lanlink-signald` accepts `--relay-udp-url` / `--relay-tcp-url` CLI args, or
  `--coordinator-url` to fetch them on demand.
- The signal service includes relay URLs in peer-list responses so the daemon
  receives them in the same field it already uses (`relay_servers`,
  `tcp_relay_servers`).

### 7. Migration strategy

**Rationale:** Existing deployments use the coordinator for candidate exchange.
A hard cut would break every running daemon.

**Three-phase approach:**
1. **Introduce** `lanlink-signald` and add signal URL support to `lanlinkd`,
   but keep coordinator candidate endpoints working.
2. **Deprecate** coordinator candidate endpoints: log warnings when used,
   include `signal_url` in join responses, and allow the coordinator to proxy
   legacy candidate traffic to the signal service.
3. **Remove** coordinator candidate endpoints after a documented migration
   window. The default `signal_url` falls back to the coordinator URL until then.

## Risks / Trade-offs

- **Signal service becomes a new SPOF for peer discovery** → Mitigation: keep it
  stateless-in-memory; multiple instances can run behind a load balancer
  because each instance rebuilds state from registrations. Later work can add
  Redis/shared state.
- **Coordinator-to-signal password validation adds latency and coupling** →
  Mitigation: cache validation results per network with a short TTL; provide an
  optional `--disable-auth` flag only for single-node test deployments.
- **Old daemons break when coordinator endpoints are removed too early** →
  Mitigation: keep the endpoints returning 410 Gone with `signal_url` in the
  body for one release before removing them.
- **Metrics endpoint on relay could expose internal topology** → Mitigation:
  counters only; no node ids, network ids, or IP addresses.
- **HTTP listener in relay crate increases attack surface** → Mitigation:
  separate listener, default off, bound to localhost in the default systemd
  unit, and sandboxing identical to the coordinator unit.

## Migration Plan

1. Deploy `lanlink-signald` on the same host or a nearby host as the
   coordinator.
2. Update `lanlink-coordinator.service` environment or startup flags to pass the
   signal URL to join responses (`--signal-url`).
3. Add `signal = "http://localhost:8081"` (or the public signal URL) to
   `lanlinkd.toml` for each network.
4. Restart daemons. They will begin using the signal service for candidates
   while still joining via the coordinator.
5. After all daemons are upgraded, configure the coordinator to return 410 Gone
   on `POST /networks/{id}/candidates` with the signal URL in the error body.
6. In the following release, remove the candidate-handling code from
   `crates/coordinator/src/api.rs`.

Rollback: if a signal service fails, operators can set `signal` equal to the
coordinator URL in `lanlinkd.toml` to use the legacy candidate endpoint until
it is removed.

## Open Questions

- Should the signal service validate passwords itself (e.g., by querying the
  coordinator) or should the coordinator issue a short-lived join token that the
  daemon presents to the signal service? The token approach is cleaner but is
  a larger change; the password-header approach is the migration minimum.
- Should `lanlink-relayd` expose `/metrics` by default or remain opt-in? The
  proposal says add endpoints; the default can be decided during implementation.
- Do we want a WebSocket endpoint on `lanlink-signald` now, or only REST
  polling? This change keeps REST polling to match the existing refresh loop;
  a WebSocket push channel can be added later without breaking the REST API.
