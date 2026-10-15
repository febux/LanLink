# Tasks

Implementation order from the original plan (`docs/tcp-teredo-plan.md`); items 1–6 shipped, item 7 deferred:

- [x] 1 Add protocol fields (`tcp_candidates`, `tcp_relay_candidates`, `tcp_relay_servers`; serde defaults keep old clients compatible)
- [x] 2 Implement `TcpRelayServer` in `lanlink-relay`
- [x] 3 Add TCP listener to `lanlinkd` and advertise TCP candidates
- [x] 4 Add TCP connect fallback in `lanlink-punch`
- [x] 5 Wire TCP receive path into the daemon main loop
- [x] 6 Extend the daemon status API with TCP path classification
- [ ] 7 Add a minimal Teredo/IPv6 discovery phase — deferred to future work (tracked in the active `windows-parity` change notes)
