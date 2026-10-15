# Tasks

## 1. Status transport on Windows

- [ ] 1.1 Implement named-pipe (or TCP loopback) status server behind `cfg(windows)` with the same NDJSON protocol, verified by a unit test against a fake pipe listener
- [ ] 1.2 Verify `lanlink-tui` and `lanlink-web` read daemon status on Windows

## 2. Firewall & gateway parity

- [ ] 2.1 Implement a Windows firewall helper (WFP) for public/gaming profiles mirroring the nftables profile semantics, verified by a blocked-port test on a real machine
- [ ] 2.2 Implement Windows gateway mode (routing + NAT), verified by reaching a physical-LAN host through a remote gateway node

## 3. Installer & driver

- [ ] 3.1 Bundle the TAP-Windows6 driver in the MSI or detect its absence and prompt, verified by a clean install on a fresh Windows VM
- [ ] 3.2 Run the full real-machine checklist: TAP adapter appears in `ncpa.cpl` with the expected `26.x.x.x` address, both PCs ping, LAN-game/broadcast discovery sees the remote peer

## 4. Deferred stretch

- [ ] 4.1 Teredo/IPv6 discovery phase for IPv4-only peers behind restrictive NATs (design first; multi-week effort)

## Workflow follow-up

- Replace `skip_specs` with real delta specs per item before implementation.
- Archive the change when all tasks ship.
