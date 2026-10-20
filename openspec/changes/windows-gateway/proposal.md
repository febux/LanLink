# Proposal

## Why

`openspec/specs/windows-gateway/spec.md` (seeded when the windows-parity change was archived) carries two Windows requirements without a dedicated change of their own: gateway mode (IP forwarding + WinNAT masquerade) and peer route management (advertise/withdraw sync + stale-route flush). This change is the tracking shell that verifies those requirements against the implementation shipped in the windows-parity change (`crates/lanlink-gateway`, commits 9050383/6233d7d), closes remaining gaps if any are found, and records board-ticket IDs per task.

## What Changes

- Nothing new is expected in code: the Windows gateway implementation reuses `crates/lanlink-gateway` (script builders under `cfg(windows)`, mirroring the Linux ip_forward + nftables MASQUERADE path) and the `lanlinkd` startup/shutdown wiring.
- Each requirement/scenario is verified against code and unit tests; gaps found during verification are fixed here.
- Tracking is mirrored on the `lanlink` board as sub-tickets (IDs listed in tasks.md).

## Capabilities

### New Capabilities

- None (spec already exists at `specs/windows-gateway/`).

### Modified Capabilities

- None.

## Impact

- `crates/lanlink-gateway` (verification only unless gaps found), `crates/lanlinkd/src/main.rs` (gateway enable/disable + route sync wiring, verification only).
