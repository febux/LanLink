# Tasks: e2e-two-peer-tests

## Anchor files (existing test infrastructure)

This change builds on the existing workspace test layout rather than replacing it:

- `tests/soak.sh` — proven netns/TAP/coordinator/relay topology; reuse its helper patterns in `e2e/lib.sh`.
- `tests/memory-audit.sh` — existing memory-safety harness; remains unchanged.
- `crates/coordinator/tests/api.rs` and `crates/coordinator/tests/api_test.rs` — existing coordinator API tests that `integration_tests/tests/coordinator_api.rs` can extend.
- `Cargo.toml` (workspace members `crates/*`) — `integration_tests/` must be added as an explicit member because it lives at the repository root.
- `Makefile` — new `e2e*` and `integration-test` targets are added next to the existing `test` target.
- `.github/workflows/diagnostics.yml` and `.github/workflows/release.yml` — the new `.github/workflows/e2e.yml` follows the same layout.

## 1. Scaffolding

- [ ] 1.1 Create top-level `integration_tests/` directory with `Cargo.toml`, `src/lib.rs`, and the workspace dependency references needed to compile against `lanlink-crypto`, `lanlink-switch`, `lanlink-proto`, `lanlink-punch`, `lanlink-relay`, `lanlink-firewall`, `lanlink-gateway`, and `coordinator`. Add `integration_tests` to the workspace `Cargo.toml` members list. Verify with `cargo check -p integration_tests`.
- [ ] 1.2 Create top-level `e2e/` directory with `lib.sh`, a `README.md` explaining the root requirement, and empty per-scenario runner stubs. Reuse netns/TAP/coordinator/relay patterns from `tests/soak.sh`. Verify that `ls e2e/*.sh` lists the expected scenario runners and that `bash -n e2e/lib.sh` passes.
- [ ] 1.3 Add an `e2e` feature or `#[ignore]` wrapper crate under `e2e/` if Rust-based test discovery is desired, ensuring it does not run during `cargo test --workspace` without the feature. Verify with `cargo test --workspace` that no e2e code executes.

## 2. Integration test implementation

- [ ] 2.1 Implement `integration_tests/tests/crypto_session.rs`: two `lanlink_crypto::Identity` instances complete a Noise handshake and encrypt/decrypt a round-trip message. Verify with `cargo test -p integration_tests --test crypto_session`.
- [ ] 2.2 Implement `integration_tests/tests/switch_framing.rs`: two `lanlink_switch::Switch` instances exchange an Ethernet frame over loopback UDP, verifying reassembly and MAC-table learning. Verify with `cargo test -p integration_tests --test switch_framing`.
- [ ] 2.3 Implement `integration_tests/tests/coordinator_api.rs`: start `coordinator::app(coordinator::db::connect_test().await?, vec![], vec![])` and exercise create, join, list members, update candidates, resolve, and telemetry/metrics endpoints. Verify with `cargo test -p integration_tests --test coordinator_api`.
- [ ] 2.4 Implement `integration_tests/tests/punch_exchange.rs`: two `lanlink_punch::Puncher` instances and a local `lanlink_relay::RelayServer` exchange candidates through an in-process coordinator listener and verify a PUNCH_REPLY path. Verify with `cargo test -p integration_tests --test punch_exchange`.
- [ ] 2.5 Add a shared helper in `integration_tests/src/lib.rs` for deterministic node IDs, network PSKs, and test timeouts. Verify by running all integration tests and confirming helpers compile.
- [ ] 2.6 Wire `integration_tests` into the workspace `Cargo.toml` if needed and confirm `cargo test --workspace` runs the new tests without root.

## 3. E2E harness shared library

- [ ] 3.1 Implement `e2e/lib.sh` helpers for: namespace creation/cleanup, veth+bridge setup, TAP creation inside namespaces, release binary discovery (`lanlinkd`, `lanlink-coordinator`, `lanlink-relayd`, `lanlinkctl`), process liveness checks, coordinator network creation, and status socket NDJSON queries. Verify by sourcing `e2e/lib.sh` with `bash -n e2e/lib.sh`.
- [ ] 3.2 Implement `e2e/lib.sh` logging/report helpers that write per-scenario work directories under `target/e2e/<scenario>.$$/` and produce a markdown report. Verify by running a stub scenario that creates and tears down its work directory.
- [ ] 3.3 Define per-scenario exit-code constants in `e2e/lib.sh` and a `fail_scenario <code> <message>` function that prints the last 20 lines of relevant logs. Verify with a unit test of `fail_scenario` in a throwaway script.

## 4. E2E scenarios

- [ ] 4.1 Implement `e2e/join.sh`: two peers join a coordinator-created network and receive distinct virtual IPs. Verify by running `sudo make e2e-join` and checking exit code `0`.
- [ ] 4.2 Implement `e2e/direct.sh`: assert status snapshot path is `Direct` after join. Verify by running `sudo make e2e-direct`.
- [ ] 4.3 Implement `e2e/relay.sh`: block direct UDP between peer namespaces while leaving relay path open, then assert status path is `Relay` and ICMP succeeds. Verify by running `sudo make e2e-relay`.
- [ ] 4.4 Implement `e2e/mac-switch.sh`: send ARP + ICMP between peer TAPs and verify ARP resolution and MAC-table-based unicast forwarding. Verify by running `sudo make e2e-mac-switch`.
- [ ] 4.5 Implement `e2e/pmtu.sh`: send ICMP echo requests with payloads larger than the inner chunk size and verify replies are not corrupted. Verify by running `sudo make e2e-pmtu`.
- [ ] 4.6 Implement `e2e/gateway.sh`: one peer advertises a gateway-shared subnet, a third namespace acts as a physical-LAN host, the non-gateway peer routes the subnet through the gateway, and ICMP to the physical-LAN host succeeds. Verify by running `sudo make e2e-gateway`.
- [ ] 4.7 Implement `e2e/status.sh`: query each peer's status socket for `{"cmd":"snapshot"}` and assert the returned NDJSON contains the expected peers and path. Verify by running `sudo make e2e-status`.
- [ ] 4.8 Implement `e2e/firewall.sh`: join a public network, apply the default firewall profile, and verify a blocked TCP port is dropped while ICMP echo is allowed. Verify by running `sudo make e2e-firewall`.

## 5. Makefile and CI wiring

- [ ] 5.1 Add Makefile targets `e2e`, `e2e-join`, `e2e-direct`, `e2e-relay`, `e2e-mac-switch`, `e2e-pmtu`, `e2e-gateway`, `e2e-status`, `e2e-firewall`, and `integration-test` in the existing `Makefile` (group them after the `test` target). Each `e2e-*` target must check for root, build release binaries first, and forward a distinct exit code. Verify by running `make -n e2e-join` and inspecting the planned commands, and `make help` for the new target descriptions.
- [ ] 5.2 Update the workspace `test` target help string in the `Makefile` to clarify that it runs rootless tests only. Verify with `make help`.
- [ ] 5.3 Create `.github/workflows/e2e.yml` alongside `.github/workflows/diagnostics.yml` and `.github/workflows/release.yml`, with two jobs: an `integration` job running on `ubuntu-24.04` with `make integration-test`, and an `e2e` job documented as requiring a privileged self-hosted runner with a conditional `if: github.repository == '...' && vars.E2E_RUNNER != ''`. The e2e job runs `make e2e`. Verify by running `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/e2e.yml'))"`.
- [ ] 5.4 Add a `docs/testing.md` (or extend `README.md`) describing how to run integration tests and e2e scenarios locally, including the root requirement and how to read logs from `target/e2e/<scenario>.$$/`. Verify by reading the rendered markdown.

## 6. Final verification

- [ ] 6.1 Run `cargo test --workspace` and confirm all rootless tests pass (this covers `crates/*/tests` and `integration_tests/` but not `tests/soak.sh`, `tests/memory-audit.sh`, or `e2e/`). Document any failures and fix before proceeding.
- [ ] 6.2 Run `make integration-test` and confirm all `integration_tests/` tests pass.
- [ ] 6.3 Run `sudo make e2e` on a Linux host with `nftables`/`iproute2` and confirm all scenarios pass; if any fail, file follow-up tasks rather than expand scope.
- [ ] 6.4 Run `openspec validate --change e2e-two-peer-tests` (or `openspec validate --changes` if the CLI form is unavailable) and confirm the change passes validation.

## Workflow follow-up

- Review the planning artifacts with the team and address any spec/design feedback.
- Start `/opsx-apply` or the equivalent apply workflow to implement the tasks above.
- Archive the change after implementation and review are complete.
