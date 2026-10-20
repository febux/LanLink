# Design: e2e-two-peer-tests

## Context

LanLink already has strong unit-test coverage inside crates and two long-running harnesses in `tests/`:

- `tests/soak.sh` builds a multi-peer topology on a Linux bridge + per-peer network namespaces, runs coordinator/relayd on the host, starts `lanlinkd` peers in each namespace, and checks churn, packet loss, memory growth, and graceful shutdown.
- `tests/memory-audit.sh` runs `miri` and `valgrind` on the parser/switch/crypto crates.
- `crates/coordinator/tests/api.rs` and `api_test.rs` exercise the coordinator Axum app against an in-memory SQLite pool.
- `crates/lanlink-switch/src/lib.rs`, `crates/lanlink-crypto/src/lib.rs`, `crates/lanlink-punch/src/lib.rs`, and `crates/lanlink-relay/src/lib.rs` all contain in-crate integration-style tests against loopback UDP sockets.
- The public APIs we will reuse are: `coordinator::app(pool, relay_servers, tcp_relay_servers)`, `coordinator::db::connect_test()`, `lanlink_crypto::{StaticSession, Identity, Handshake, NoiseSession, derive_network_psk}`, `lanlink_switch::Switch`, `lanlink_punch::Puncher`, `lanlink_relay::RelayServer`, `lanlink_firewall::FirewallProfile`, `lanlink_gateway::GatewayManager`, and the `lanlinkd` status Unix socket protocol (`{"cmd":"snapshot"}` → `lanlink_proto::status::StatusResponse`).

What is missing is a scenario-level suite that runs *two real `lanlinkd` peers* and asserts on externally observable outcomes (join success, path kind, ARP/ICMP, status NDJSON, firewall rules). This design grounds the new `e2e/` and `integration_tests/` directories in those existing scripts and APIs.

## Goals / Non-Goals

**Goals:**

- Provide a top-level `e2e/` directory with small, per-scenario Linux runners that start a coordinator, relay, and two real `lanlinkd` peers in network namespaces and assert on:
  - join success and virtual IP assignment,
  - direct UDP path classification,
  - relay fallback when direct UDP is blocked,
  - MAC learning/ARP/ICMP between TAP interfaces,
  - PMTU fragmentation/reassembly,
  - gateway mode forwarding to a third physical-LAN namespace,
  - daemon status NDJSON snapshots,
  - firewall profile installation.
- Provide a top-level `integration_tests/` directory with crate-level integration tests that exercise multi-crate public APIs without root or netns.
- Wire both suites into `Makefile` with clear targets, per-scenario exit codes, and readable failure output.
- Document a CI plan that runs rootless integration tests on standard runners and runs the root-required e2e suite on a privileged/self-hosted Linux runner.

**Non-Goals:**

- Running E2E scenarios on Windows or macOS in the first iteration (the gateway/firewall code is cross-platform, but the namespace/TAP plumbing is Linux-specific).
- Replacing `tests/soak.sh` or `tests/memory-audit.sh`.
- Adding new product features (this is test infrastructure only).
- Running e2e tests inside a general `cargo test` invocation.

## Decisions

### 1. E2E scenarios are small runner scripts, not only `#[ignore]` Rust tests

**Rationale:** Network namespace creation, veth/TAP plumbing, and `nftables` rule manipulation are easiest to express in shell, and `tests/soak.sh` already proves the pattern. Each scenario can be a standalone bash script that reuses helper functions from a shared `e2e/lib.sh`. A small number of pure-Rust `#[ignore]` tests may wrap the bash runners so developers can discover them with `cargo test -- --ignored` if desired, but the primary interface is `make e2e-join`, `make e2e-relay`, etc.

**Alternative considered:** Pure Rust using the `nix` crate for netns manipulation. Rejected because it duplicates the proven shell plumbing in `tests/soak.sh` and is harder to debug for operators.

### 2. E2E scenarios are `#[ignore]` by default and invoked through `make`

**Rationale:** The scenarios require `CAP_NET_ADMIN` and root. Marking them `#[ignore]` keeps `cargo test` fast and unprivileged. Makefile targets make the root requirement explicit and provide a single entry point.

**Planned targets:**

- `make e2e` — run all e2e scenarios in sequence and report a combined verdict.
- `make e2e-join`, `make e2e-direct`, `make e2e-relay`, `make e2e-mac-switch`, `make e2e-pmtu`, `make e2e-gateway`, `make e2e-status`, `make e2e-firewall` — run individual scenarios.
- `make integration-test` — run `cargo test -p integration_tests` (or `cargo test --workspace --test integration` depending on layout).
- `make test` remains `cargo test --workspace` and therefore runs only the rootless tests.

### 3. Each scenario uses a dedicated work directory and exits with a distinct code

**Rationale:** Makes debugging CI failures straightforward. `tests/soak.sh` already writes PIDs, logs, and TSV samples under `$SOAK_DIR`. Each e2e scenario will write to `target/e2e/<scenario>.$$/` and keep:

- `coordinator.log`, `relayd.log`, `peer1.log`, `peer2.log`,
- per-peer `.toml` configs,
- a `verdict` file and a short markdown report.

Exit codes:

- `10` — setup failure (missing binaries, netns/TAP creation failed),
- `20` — join timeout,
- `30` — direct path not established,
- `40` — relay fallback not observed,
- `50` — MAC/ARP/ICMP failure,
- `60` — PMTU/reassembly failure,
- `70` — gateway forwarding failure,
- `80` — status snapshot assertion failure,
- `90` — firewall profile assertion failure.

### 4. Integration tests live in `integration_tests/` as a normal Cargo crate

**Rationale:** A top-level crate keeps multi-crate tests in one place, lets us depend on the workspace crates explicitly, and runs with `cargo test -p integration_tests`. It mirrors how NetBird's `integration_tests/` directory is organized.

**Planned test modules:**

- `crypto_session.rs` — two `lanlink_crypto::Identity` handshakes, encrypt/decrypt round-trip.
- `switch_framing.rs` — two `lanlink_switch::Switch` instances exchange a frame over a loopback UDP socket and verify reassembly + MAC learning.
- `coordinator_api.rs` — start coordinator `app()` with `connect_test()`, exercise create/join/list/members/candidates.
- `punch_exchange.rs` — two `lanlink_punch::Puncher` instances, a local `RelayServer`, and a small in-process coordinator listener exchange candidates and verify a path.

### 5. Reuse existing public APIs rather than adding test-only interfaces

**Rationale:** Integration tests should look like real callers. We will call `coordinator::app()` and `coordinator::db::connect_test()` directly, construct `Switch` and `Puncher` manually, and use a small Unix-socket client (Python/`socat`) to query the daemon status socket for the e2e status assertions rather than adding test hooks inside `lanlinkd`.

### 6. CI plan: separate jobs for integration and e2e

**Rationale:** Standard GitHub Actions runners cannot create network namespaces or TAP devices. The integration job runs on `ubuntu-24.04` with `cargo test -p integration_tests`. The e2e job is documented as requiring a privileged self-hosted runner (or a container with `--privileged`) and is added to the workflow with a conditional so it does not block normal PRs until such a runner is available.

## Risks / Trade-offs

- **Flaky e2e tests due to timing** → Mitigation: generous join/forwarding timeouts, retry loops for path classification, and deterministic namespace IP assignment. Each scenario retries the readiness check up to a documented deadline before failing.
- **Privileged runner not available in CI immediately** → Mitigation: e2e suite is valuable locally first; CI integration is documented as optional until a runner is provisioned. Integration tests run unprivileged and provide immediate value.
- **Firewall scenario needs `nft` and may conflict with host rules** → Mitigation: each scenario uses a uniquely named table derived from the TAP interface name (`lanlink0` → `lanlink0`) and cleans up in a `trap`/`cleanup` function, following `lanlink_firewall::FirewallProfile::clear()` and `lanlink_gateway::GatewayManager::disable()`.
- **Gateway scenario is complex (proxy-ARP, forwarding, MASQUERADE)** → Mitigation: start with the simplest possible topology: one gateway namespace with a physical-LAN host on a veth, one non-gateway namespace, and route the shared subnet through the gateway's virtual IP.
- **Duplicate helper code between `tests/soak.sh` and e2e scripts** → Mitigation: factor shared netns/TAP/coordinator/relay helpers into `e2e/lib.sh` and consider later making `tests/soak.sh` source it.

## Migration Plan

No migration needed. This change is purely additive test infrastructure. Existing `make test` and the `tests/` harnesses continue to work unchanged.
