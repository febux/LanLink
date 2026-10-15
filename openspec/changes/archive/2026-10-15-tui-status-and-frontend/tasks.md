# Tasks

Build order from the original plan (`docs/tui-plan.md` §7), all shipped (§10 "What shipped"):

- [x] 1 `lanlink-proto/src/status.rs` types + round-trip tests
- [x] 2 Switch extensions + keepalive fix + tests
- [x] 3 `lanlinkd/src/status.rs` server, `--status-socket` flag, ping loop wiring
- [x] 4 TUI skeleton: terminal lifecycle, event loop, daemon client, dashboard
- [x] 5 Peers table, then Networks screen + forms + action worker
- [x] 6 Help overlay, toasts, color polish
- [x] 7 Tests, docs, fmt/clippy pass (`make fmt clippy test` green)
