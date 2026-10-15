# Tasks

## 1. Status transport on Windows

- [x] 1.1 Implement TCP loopback status server behind `cfg(windows)` with the same NDJSON protocol (port = 9000 + network id), verified by unit tests
- [x] 1.2 Verify `lanlink-tui`, `lanlink-web`, and `lanlink-desktop` read daemon status on Windows via TCP loopback

## 2. Firewall & gateway parity

- [ ] 2.1 Implement a Windows firewall helper (WFP) for public/gaming profiles mirroring the nftables profile semantics, verified by a blocked-port test on a real machine
- [ ] 2.2 Implement Windows gateway mode (routing + NAT), verified by reaching a physical-LAN host through a remote gateway node

## 3. Installer & driver

- [x] 3.1 Switch the Windows backend to Wintun and bundle `wintun.dll` in the MSI; no separate driver install is required. Verified by clean install on a fresh Windows VM (daemon starts and joins a network; adapter appears in `ncpa.cpl`).
- [ ] 3.2 Run the full real-machine checklist: Wintun adapter appears in `ncpa.cpl` with the expected `26.x.x.x` address, both PCs ping, IP-based LAN game traffic works. Note: Wintun is layer-3, so raw Ethernet/ARP/broadcast discovery is not available on Windows yet.

## 4. Deferred stretch

- [ ] 4.1 Teredo/IPv6 discovery phase for IPv4-only peers behind restrictive NATs (design first; multi-week effort)

## Workflow follow-up

- Replace `skip_specs` with real delta specs per item before implementation.
- Archive the change when all tasks ship.
