# Tasks

## 1. Protocol additions in `lanlink-proto`

- [ ] 1.1 Add `signal_url` to `JoinNetworkResponse` in
  `crates/lanlink-proto/src/lib.rs` and verify that existing tests still
  compile.
- [ ] 1.2 Add a `signal` module or extend `lib.rs` with request/response types
  for signal-plane registration (e.g., `SignalRegister`, reusing
  `PresenceUpdate` and `Vec<PeerInfo>`) and verify serialization round-trips
  in a unit test.

## 2. New `crates/lanlink-signal` service

- [ ] 2.1 Create `crates/lanlink-signal/Cargo.toml` with workspace dependencies
  (`tokio`, `axum`, `serde`, `serde_json`, `clap`, `tracing`, `lanlink-proto`)
  and a `[[bin]]` named `lanlink-signald`; verify `cargo check -p lanlink-signal`.
- [ ] 2.2 Implement in-memory network/peer registry with TTL in
  `crates/lanlink-signal/src/state.rs`, including `register(network_id, peer,
  candidates)` and `list_peers(network_id, exclude_node_id)`; verify with a
  tokio test that stale peers are pruned.
- [ ] 2.3 Implement Axum API in `crates/lanlink-signal/src/api.rs` with
  `GET /health`, `GET /metrics`, `POST /networks/{id}/peers`, and
  `GET /networks/{id}/peers`; verify each endpoint with a request in a test.
- [ ] 2.4 Add CLI and main entry in `crates/lanlink-signal/src/main.rs` with
  `--listen`, `--metrics-listen`, and relay URL flags; run
  `cargo run --bin lanlink-signald -- --help` and confirm it prints usage.
- [ ] 2.5 Add a metrics registry counting `networks`, `peers`, and `uptime_seconds`
  and wire it into `GET /metrics`; verify the JSON response in a test.

## 3. Observability in `lanlink-relay`

- [ ] 3.1 Add atomic counters for UDP packets/bytes relayed and active UDP
  binding count in `crates/lanlink-relay/src/lib.rs`; verify counters increment
  in the existing `relay_forwards_between_bound_clients` test.
- [ ] 3.2 Add counters for TCP packets/bytes relayed and active TCP session count
  in `crates/lanlink-relay/src/tcp.rs`; verify counters increment in the
  existing `tcp_relay_forwards_between_bound_clients` test.
- [ ] 3.3 Add an optional HTTP listener with `GET /health` and `GET /metrics` in
  `crates/lanlink-relay/src/main.rs` behind a `--metrics-listen` flag; start
  `lanlink-relayd --metrics-listen 127.0.0.1:9090` and confirm both endpoints
  return HTTP 200 with expected JSON fields.

## 4. Coordinator slim-down

- [ ] 4.1 Add `signal_url` to `AppState` and CLI in
  `crates/coordinator/src/main.rs`; run `cargo run --bin lanlink-coordinator --
  --help` and confirm the new flag appears.
- [ ] 4.2 Return `signal_url` in `JoinNetworkResponse` from
  `crates/coordinator/src/api.rs` (`join_network` and `resolve_network`) and
  verify the field is present in a join response test.
- [ ] 4.3 Mark `POST /networks/{id}/candidates` in
  `crates/coordinator/src/api.rs` as deprecated during the migration window:
  keep it working but log a warning and return the signal URL in the response
  or header; verify with a test that old daemons still receive a peer list.
- [ ] 4.4 Stop returning peer candidates in the `JoinNetworkResponse` from
  `crates/coordinator/src/api.rs` (peers may be empty or carry only metadata)
  and update coordinator tests to assert that candidates come from the signal
  service, not the join response.

## 5. `lanlinkd` signal-plane wiring

- [ ] 5.1 Add `signal` URL to `FileConfig::NetworkSection` and `CliOverrides`
  in `crates/lanlinkd/src/config.rs`; verify `resolve` picks CLI over file and
  defaults to the coordinator URL when absent.
- [ ] 5.2 Add `signal` URL to `RunConfig` and pass it through
  `crates/lanlinkd/src/main.rs` and `crates/lanlinkd/src/up.rs`; verify
  `lanlinkd up --signal http://localhost:8081 --coordinator http://localhost:8080`
  resolves both URLs.
- [ ] 5.3 Refactor `refresh_candidates_and_punch` and
  `refresh_single_peer_candidates` in `crates/lanlinkd/src/main.rs` to target
  the signal URL when present; verify with an integration-style test using a
  local HTTP server that candidates are POSTed to the signal URL.
- [ ] 5.4 Add fallback logic so that when no signal URL is configured or the
  signal service returns 404/410, the daemon posts candidates to the legacy
  coordinator endpoint; verify both paths with tests.
- [ ] 5.5 Confirm `crates/lanlinkd/src/status.rs` still populates peer metadata
  correctly after a refresh from the signal service; verify the status snapshot
  test `snapshot_classifies_paths_and_merges_meta` passes unchanged.

## 6. systemd packaging

- [ ] 6.1 Create `dist/systemd/lanlink-signal.service` mirroring
  `dist/systemd/lanlink-coordinator.service` (user `lanlink`,
  `ProtectSystem=strict`, capability bounding set empty, no `CAP_NET_RAW`);
  verify the unit parses with `systemd-analyze verify` if available.
- [ ] 6.2 Update `dist/systemd/lanlink-coordinator.service` to pass
  `--signal-url` to `lanlink-coordinator`; verify the `ExecStart` line is valid.
- [ ] 6.3 Update `dist/systemd/lanlink-relayd.service` with an optional
  `--metrics-listen` argument bound to `127.0.0.1`; verify the unit still
  parses.

## 7. Migration and back-compat

- [ ] 7.1 Document the three-phase migration (introduce → deprecate → remove)
  in `docs/` or a new `docs/signal-migration.md`; verify by reading the
  rendered file.
- [ ] 7.2 Implement coordinator proxy mode: when
  `POST /networks/{id}/candidates` is hit and a signal URL is configured, the
  coordinator forwards the request to the signal service and returns the
  result; verify with a test that the legacy daemon path still returns peers.
- [ ] 7.3 Add a startup log in `lanlinkd` that records which URL is used for
  candidate exchange (signal or coordinator fallback); verify by running
  `lanlinkd up` and inspecting logs.

## 8. Integration verification

- [ ] 8.1 Run the full test suite with `cargo test` and ensure no regressions
  in `coordinator`, `lanlink-relay`, `lanlinkd`, and `lanlink-proto`.
- [ ] 8.2 Run a local two-peer smoke test: start `lanlink-coordinator`,
  `lanlink-signald`, and `lanlink-relayd`; run two `lanlinkd up` instances
  against the coordinator and confirm both peers appear in each other's status
  snapshots and packets flow.
- [ ] 8.3 Run `openspec validate --change split-signal-relay-services` and
  confirm all planning artifacts pass validation.

## Workflow follow-up

- Archive the change after the project's review requirements are satisfied.
- Verify the archived result.
