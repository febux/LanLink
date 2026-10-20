# Tasks

## 1. Pure anonymization module

- [ ] 1.1 Add `crates/lanlink-proto/src/anonymize.rs` with `Anonymizer` struct, per-process salt resolution, and `mask_text` entry point. Verify `cargo check -p lanlink-proto` passes.
- [ ] 1.2 Implement `mask_ipv4` (default /24 prefix, configurable) and unit tests covering valid, invalid, localhost, broadcast, and `0.0.0.0` inputs. Verify tests pass.
- [ ] 1.3 Implement `mask_ipv6` (default /64 prefix, configurable) and unit tests covering `::1`, `fe80::1`, full address, and invalid input. Verify tests pass.
- [ ] 1.4 Implement `mask_mac` (preserve OUI) and unit tests covering colon, hyphen, uppercase, lowercase, and invalid input. Verify tests pass.
- [ ] 1.5 Implement `mask_id` for node/device IDs and public keys, returning a stable short token, with collision and stability tests. Verify tests pass.
- [ ] 1.6 Implement `mask_network_name` returning a stable short token, with stability and distinctness tests. Verify tests pass.
- [ ] 1.7 Add `redact_passwords` helper that replaces known password patterns/substrings with `<redacted>`; unit-test redaction of literal passwords and password-file paths. Verify tests pass.

## 2. Tracing integration

- [ ] 2.1 Add `[log]` section to `crates/lanlinkd/src/config.rs` with `anonymize` boolean defaulting to `false`, plus resolved `RunConfig` field and test that parsing works. Verify `cargo test -p lanlinkd config` passes.
- [ ] 2.2 Implement `AnonymizeLayer<S>` in a new `crates/lanlinkd/src/log_anonymizer.rs` module that wraps an inner layer and applies `mask_text` to each rendered event. Verify it compiles with the existing `tracing-subscriber` setup.
- [ ] 2.3 Wire the anonymizing layer into the non-Windows console subscriber in `crates/lanlinkd/src/main.rs` when `cfg.log_anonymize` is true; default path remains unchanged. Verify `cargo check -p lanlinkd` passes.
- [ ] 2.4 Wire the anonymizing layer into the Windows service file appender in `crates/lanlinkd/src/service.rs` when `[log] anonymize` is true. Verify `cargo check -p lanlinkd` passes on a Windows target (or `cargo check --workspace` on the current host if Windows deps are cfg-gated).

## 3. Status snapshot anonymization

- [ ] 3.1 Add optional `privacy: bool` field to `lanlink_proto::status::StatusRequest::Snapshot` with `#[serde(default)]`. Verify existing NDJSON round-trip tests still pass.
- [ ] 3.2 Add `anonymize_snapshot` method in `crates/lanlinkd/src/status.rs` that masks all peer endpoints, virtual IPs, assigned IP, node IDs, relay address, and shared subnets while keeping counters and path kinds intact. Verify `cargo check -p lanlinkd` passes.
- [ ] 3.3 Update the NDJSON handlers (`handle_conn` / `handle_tcp_conn`) to call the anonymized path when `privacy` is true. Verify `cargo test -p lanlinkd status_socket_round_trip` and Windows equivalent still pass.
- [ ] 3.4 Add a unit test in `crates/lanlinkd/src/status.rs` asserting that a privacy-enabled snapshot contains masked addresses and unchanged peer count/path kinds. Verify the test passes.

## 4. `lanlinkctl bugreport`

- [ ] 4.1 Add `Bugreport` subcommand and arguments (`--config`, `--log-file`, `--output`, `--status-socket`) to `crates/lanlinkctl/src/main.rs`. Verify `cargo check -p lanlinkctl` passes.
- [ ] 4.2 Implement log-file anonymization: read the log file, apply `mask_text` with a freshly generated salt, and write `logs.txt` into a timestamped bundle directory. Verify with a temporary log file that the output contains no original IPs or node IDs.
- [ ] 4.3 Implement config sanitization: copy the config file, redact `password`, `password_file`, and any network name, and write `config.toml` into the bundle. Verify with a temporary config that secrets are removed.
- [ ] 4.4 Implement optional status snapshot collection: connect to the daemon status socket, request `Snapshot { privacy: true }`, and write `snapshot.jsonl`. Verify with a mock status server that the line is a valid anonymized snapshot.

## 5. Documentation, config examples, and final verification

- [ ] 5.1 Update `dist/lanlinkd.example.toml` with a documented `[log]` section showing `anonymize = false`. Verify the file parses as valid TOML.
- [ ] 5.2 Update `dist/windows/lanlinkd.toml` with the same `[log]` documentation. Verify the file parses as valid TOML.
- [ ] 5.3 Run `cargo check --workspace` and confirm exit code 0.
- [ ] 5.4 Run `cargo test --workspace` and confirm all tests pass.
- [ ] 5.5 Run `openspec validate --change log-pii-anonymization` and confirm it reports green.

## Workflow follow-up

- Archive the change after implementation review and required tests pass.
- Verify the archived spec appears under `openspec/specs/privacy/log-anonymization/spec.md`.
