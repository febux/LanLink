# Proposal: e2e-two-peer-tests

## Why

LanLink today has extensive unit tests inside individual crates and two long-running harness scripts (`tests/soak.sh`, `tests/memory-audit.sh`), but there is no scenario-level suite that exercises two real `lanlinkd` peers end-to-end. That gap is the strongest remaining signal that the project is not yet "real" for users and contributors who want confidence in the control plane, data plane, NAT traversal, relay fallback, MAC learning, PMTU, gateway mode, and daemon observability working together. NetBird (netbirdio/netbird) uses its `e2e/` and `integration_tests/` directories as its primary legitimacy signal; LanLink needs the same kind of owned, repeatable, scenario-based proof.

## What Changes

- Add a top-level `e2e/` directory containing full-stack, two-node (and multi-netns) Linux scenarios that run real `lanlinkd` peers against a real `coordinator` and `lanlink-relayd`.
  - Each scenario is a small runner script or an `#[ignore]` Rust test.
  - Scenarios are opt-in via `make e2e` / `make e2e-<scenario>` and require root because they create network namespaces, veth pairs, bridges, and TAP devices.
  - Covered scenarios: join, direct UDP path, relay fallback (force relay by blocking UDP), MAC learning/switching via ARP+ICMP between virtual adapters, PMTU fragmentation, gateway mode with a third netns as physical-LAN host, daemon status NDJSON snapshots, and firewall profile installation.
- Add a top-level `integration_tests/` directory containing crate-level integration tests that exercise public APIs of multiple crates together without root/netns where possible.
  - crypto session handshake between two nodes,
  - switch + proto framing round-trip,
  - coordinator API against a temp SQLite,
  - punch candidate exchange against a local signal/coordinator instance.
- Wire the new suites into `Makefile` with clear targets, per-scenario exit codes, and readable failure output.
- Add a CI plan (documented in `tasks.md`) for when and how to run the rootless integration tests versus the root-required e2e suite.

## Capabilities

### New Capabilities

- `testing/e2e`: Full-stack Linux scenario test harness that runs real `lanlinkd` peers in network namespaces and verifies control-plane join, direct and relay data-plane paths, MAC learning/switching, PMTU, gateway mode, daemon status snapshots, and firewall profiles.
- `testing/integration`: Crate-level integration test suite that exercises the public APIs of multiple crates together (crypto sessions, switch framing, coordinator API, punch candidate exchange) without requiring root or network namespaces.

### Modified Capabilities

- None. This change introduces test infrastructure only; it does not change product behavior or existing spec requirements.

## Impact

- New top-level directories: `e2e/` and `integration_tests/`.
- New `Makefile` targets: `e2e`, `e2e-<scenario>`, `integration-test` (or equivalent), and updates to existing `test` target if needed.
- New dependencies in test-only code: `nix`, `tokio::process`, and standard Linux networking tools (`ip`, `nft`, `iptables`) invoked by scenario runners.
- CI: integration tests can run on standard runners; e2e suite requires a root-capable Linux runner (self-hosted or container with `--privileged`).
- No breaking changes to public product APIs or CLI behavior.
