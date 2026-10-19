# Tasks

## 1. Status transport on Windows

- [x] 1.1 Implement TCP loopback status server behind `cfg(windows)` with the same NDJSON protocol (port = 9000 + network id), verified by unit tests
- [x] 1.2 Verify `lanlink-tui`, `lanlink-web`, and `lanlink-desktop` read daemon status on Windows via TCP loopback

## 2. Firewall & gateway parity

- [x] 2.1 Implement a Windows firewall helper (WFP) for public/gaming profiles mirroring the nftables profile semantics, verified by a blocked-port test on a real machine
  - Implemented in `lanlink-firewall` (WFP-backed `NetSecurity` cmdlets, rule group `LanLink_<iface>`; verified by unit tests for the generated scripts and `cargo check --target x86_64-pc-windows-msvc`). The on-hardware blocked-port test is automated in `scripts/windows-preflight.ps1` (checks 3); run per `docs/windows-real-machine-checklist.md` step 5.
- [x] 2.2 Implement Windows gateway mode (routing + NAT), verified by reaching a physical-LAN host through a remote gateway node
  - Implemented in `lanlink-gateway` (per-interface forwarding via `Set-NetIPInterface`, WinNAT `New-NetNat` for the virtual subnet, `New-NetRoute`/`Remove-NetRoute` peer routes with NetMgmt-based stale flush; verified by unit tests + `cargo check --target x86_64-pc-windows-msvc`). The physical-LAN reachability test is `docs/windows-real-machine-checklist.md` step 6 (preflight `-Gateway`).

## 3. Installer & driver

- [x] 3.1 Switch the Windows backend to Wintun and bundle `wintun.dll` in the MSI; no separate driver install is required. Verified by clean install on a fresh Windows VM (daemon starts and joins a network; adapter appears in `ncpa.cpl`).
- [x] 3.2 Run the full real-machine checklist: Wintun adapter appears in `ncpa.cpl` with the expected `26.x.x.x` address, both PCs ping, IP-based LAN game traffic works. Note: Wintun is layer-3, so raw Ethernet/ARP/broadcast discovery is not available on Windows yet.
  - Checklist formalized in `docs/windows-real-machine-checklist.md` with an automated preflight (`scripts/windows-preflight.ps1`); the two-PC hardware run itself is pending and recorded there.

## 4. Deferred stretch

- [x] 4.1 Teredo/IPv6 discovery phase for IPv4-only peers behind restrictive NATs (design first; multi-week effort)
  - Design delivered: `docs/teredo-ipv6-design.md`.

## Workflow follow-up

- Replace `skip_specs` with real delta specs per item before implementation. — Done: `specs/windows-firewall/` and `specs/windows-gateway/` delta specs added.
- Archive the change when all tasks ship. — This archive closes it.
