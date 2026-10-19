# Proposal

## Why

LanLink's Windows support is functional but incomplete: the TAP-Windows6 backend exists and cross-compiles, the SCM service wrapper shipped (M7), yet real-machine testing, the status transport, firewall, gateway mode, and installer driver bundling are missing. The deferred Teredo/IPv6 NAT-traversal item from the TCP fallback plan also has no home. These gaps previously lived in `docs/windows-plan.md`; they are migrated here as the single active plan of record.

## What Changes

- Port the daemon status socket to Windows named pipes (or TCP loopback) so `lanlink-tui`/`lanlink-web` work on Windows.
- Implement a Windows firewall helper (WFP) for public/gaming network profiles (currently Linux nftables only).
- Implement Windows gateway mode (routing + NAT equivalents).
- Bundle or detect the TAP-Windows6 driver in the MSI installer.
- Run the real-machine Windows test checklist (adapter creation, ping, LAN-game discovery, UI status read).
- (Deferred stretch) Teredo/IPv6 discovery phase for peers behind restrictive IPv4-only NATs.

## Capabilities

Behavior requirements will be specified once the approach for named-pipe status transport and WFP firewall is prototyped; `skip_specs: true` for now — replace with real delta specs before implementation of each item begins.

### New Capabilities

- None yet (candidates: `windows-status-transport`, `windows-firewall`).

### Modified Capabilities

- None.

## Impact

- `crates/lanlink-tap` (Windows backend testing), `lanlinkd` (status transport, firewall/gateway plumbing), `lanlink-firewall`, `lanlink-gateway`, `dist/windows/` (MSI driver bundling), `lanlink-punch` (Teredo stretch).
