# Proposal

## Why

LanLink client logs and daemon status snapshots currently contain PII such as IP addresses, MAC addresses, node/device IDs, public keys, hostnames, and network names. Operators and users need a safe way to collect logs for bug reports and share status snapshots without leaking this information. This change adds deterministic, opt-in log and status anonymization, modeled on NetBird's `client/anonymize`, so correlations across log lines survive while sensitive values do not.

## What Changes

- **Pure anonymization module** in `lanlink-proto` (or a new shared utility crate) that provides deterministic salted hashes/masks for IPv4, IPv6, MAC, node/device IDs, public keys, and network names. Passwords are never logged or returned. Invalid inputs pass through unchanged so logging never panics.
- **Tracing integration** in `lanlinkd`: a new `tracing_subscriber` layer/formatter that applies the anonymizer to every log event when `[log] anonymize = true` (default off). Recommended for bug-report bundles.
- **Status snapshot anonymization**: the daemon's NDJSON status endpoint supports a `privacy` flag that returns an anonymized snapshot (masked peer endpoints, virtual IPs, node IDs, relay addresses, shared subnets).
- **`lanlinkctl bugreport` command**: collects recent daemon logs and the active config, forcing anonymization regardless of the daemon's `[log] anonymize` setting, and writes a timestamped bundle ready for attachment.
- **Unit tests** for every mask function covering IPv4, IPv6, localhost, invalid input, and deterministic output.

## Capabilities

### New Capabilities

- `privacy/log-anonymization`: deterministic PII masking for client logs and status snapshots, opt-in via config and mandatory for bug-report bundles.

### Modified Capabilities

- None.

## Impact

- `crates/lanlink-proto` (or new shared util crate): new `anonymize` module and public mask helpers.
- `crates/lanlinkd/src/config.rs`: new `[log]` section with `anonymize` boolean.
- `crates/lanlinkd/src/main.rs` / `crates/lanlinkd/src/service.rs`: tracing subscriber wiring for the anonymizing layer/formatter.
- `crates/lanlinkd/src/status.rs`: optional anonymization of the status snapshot before serialization.
- `crates/lanlinkctl/src/main.rs`: new `bugreport` subcommand.
- `dist/lanlinkd.example.toml` and Windows `lanlinkd.toml`: document the new `[log]` section.
- TUI/WebUI frontends: can request the privacy-enabled status snapshot to display masked addresses.
