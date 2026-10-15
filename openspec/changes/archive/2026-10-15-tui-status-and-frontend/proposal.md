# Proposal

## Why

LanLink shipped CLI-only: all runtime state (peer table, verified paths, traffic counters) lived inside the root daemon and surfaced only through `tracing` logs. PLAN.md requirements F1/F4/F7 expect create/join, a node list with status/latency, and connection-type display from a UI. The blocking gap was that the daemon exposed no local status surface.

## What Changes

- New daemon status API: read-only NDJSON snapshots over a Unix domain socket (`/run/lanlink/lanlinkd-<id>.sock`, `--status-socket` / `[status] socket` overrides, 0660, graceful degradation, Windows stub).
- Switch extensions: per-peer tx/rx counters, PING/PONG latency probes on a monotonic probe clock, keepalive sender-attribution liveness fix, `UdpOutcome` return type.
- New `lanlink-tui` crate (ratatui 0.29): dashboard / peers / networks screens, directory browsing, create+join parity with `lanlinkctl`, help overlay, pure reducer architecture.

## Capabilities

Implemented as milestone M9 (see PLAN.md §1.1); archived historical change, `skip_specs: true`.

### New Capabilities

- None.

### Modified Capabilities

- None.

## Impact

- `lanlink-proto` (status module), `lanlink-switch` (counters, probes, keepalive fix), `lanlinkd` (status server, ping loop), new `lanlink-tui` crate, docs (protocol, README, PLAN.md M9 row).
