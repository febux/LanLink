# Tasks

Tracking: disconnect + graceful stop — `t_0cb9346f`.

## 1. Control protocol and persistent daemon socket

- [x] 1.1 Add `crates/lanlink-proto/src/control.rs` with `ControlRequest` variants `status`, `connect`, `disconnect`, `stop` and matching `ControlResponse`. Verify `cargo test -p lanlink-proto` passes. (t_2cc38533)
- [x] 1.2 Add a `connected: bool` field to `DaemonStatus` in `crates/lanlink-proto/src/status.rs` and update all snapshot tests that construct it. (t_2cc38533)
- [x] 1.3 Create `crates/lanlinkd/src/control.rs` that binds a persistent Unix-domain/TCP-loopback socket (`/run/lanlink/lanlinkd-control.sock`, `/tmp/lanlinkd-control.sock` fallback; Windows `127.0.0.1:19000` or configured `[status] socket`). The server parses each incoming line as `ControlRequest`; if it fails, falls back to `StatusRequest::Snapshot`. Control commands are forwarded to the daemon runtime and answered with `ControlResponse`. (t_2cc38533)
- [x] 1.4 Reject non-loopback TCP connections on Windows; on Unix rely on filesystem permissions. (t_2cc38533)

## 2. Persistent runtime: disconnect keeps daemon alive

- [x] 2.1 Refactor `crates/lanlinkd/src/main.rs` so `Run`/`Up`/`Service(Run)` start a `DaemonRuntime` that owns the control/status socket and an optional network session. `run_coordinated` is updated to take a cancellation token and a `watch::Sender<StatusState>`, performing the same setup and cleanup as today. (t_706f8760)
- [x] 2.2 On `ControlRequest::Connect` with a `RunConfig`, start a new network session only when none is active; return `ControlResponse::Error` if already connected. (t_706f8760)
- [x] 2.3 On `ControlRequest::Disconnect`, cancel the session token, await cleanup, clear the status registry, and return an ack. The daemon process stays alive. (t_706f8760)
- [x] 2.4 On `ControlRequest::Stop`, cancel the session, trigger the global shutdown signal, await cleanup, and exit the process with code 0. (t_706f8760)
- [x] 2.5 Ensure the initial config from `run`/`up` auto-connects on startup so existing CLI behavior is preserved. (t_706f8760)

## 3. Cleanup on disconnect/stop

- [x] 3.1 In `run_coordinated`, after the session task returns (cancelled or failed), clear the firewall profile, disable gateway mode, remove synced gateway routes, and drop the TAP device so the interface/IP go away. (t_706f8760)
- [x] 3.2 Add `lanlink_tap::destroy_nic(name)` for Linux (`ip link del`) and a no-op/stub for Windows/macOS. (t_706f8760)
- [x] 3.3 Send a best-effort `POST /networks/:id/leave` to the coordinator during disconnect; log and ignore failures. (t_a36ca95b)
- [x] 3.4 Update the status registry to report `connected: false` and empty peers/TAP after disconnect. (t_706f8760)

## 4. Coordinator leave endpoint

- [x] 4.1 Add `lanlink_proto::LeaveNetworkRequest { password, node_id }`. (t_a36ca95b)
- [x] 4.2 Add `POST /networks/:id/leave` route in `crates/coordinator/src/lib.rs` and handler in `api.rs` that verifies the password and deletes the member row. (t_a36ca95b)
- [x] 4.3 Add `delete_member` helper in `crates/coordinator/src/db.rs`. (t_a36ca95b)

## 5. `lanlinkctl` daemon commands

- [x] 5.1 Add `DaemonCmd` subcommands in `crates/lanlinkctl/src/main.rs`: `status`, `disconnect`, `connect`, `stop`. (t_c79fb487)
- [x] 5.2 Implement control socket discovery: first `--socket`/configured path, then `/run/lanlink/lanlinkd-control.sock`, `/tmp/lanlinkd-control.sock`, and on Windows `127.0.0.1:19000`. (t_c79fb487)
- [x] 5.3 Implement request/response over the socket (send line, read first response line). (t_c79fb487)
- [x] 5.4 For `stop`, send `stop`, then wait for the control connection to close, which only happens after the daemon exits cleanly. (t_c79fb487)
- [x] 5.5 For `connect`, accept `--coordinator/--network-id/--password` flags plus TAP/bind/mtu options; build a `ControlRequest::Connect` and send it. (t_c79fb487)

## 6. GUI frontends

- [x] 6.1 In `lanlink-desktop`, add tray menu items `Disconnect` and `Stop daemon`; add Tauri commands `disconnect_network` and `stop_daemon` that send the matching control commands to the discovered control socket. (t_ebf81f34)
- [ ] 6.2 In `lanlink-desktop/assets/app.js`, add dashboard buttons for Disconnect/Stop and wire them to the new Tauri commands. (t_ebf81f34)
- [x] 6.3 In `lanlink-tui`, add a key action (`d` for disconnect, `x` for stop) and/or a menu row; send the same control commands. (t_ebf81f34)
- [x] 6.4 Update control-socket discovery in `lanlink-tui/src/daemon.rs` to use the persistent `lanlinkd-control.sock` path. (t_ebf81f34)

## 7. E2E test

- [x] 7.1 Add `e2e/disconnect.sh` using helpers from `e2e/lib.sh`: create a network, start two peers, verify connected, send `disconnect` on peer 1, assert `connected: false`. (t_db304eb7)
- [x] 7.2 Send `connect` on peer 1 with the same parameters and verify it reconnects without restarting the daemon process. (t_db304eb7)
- [x] 7.3 Send `stop` on peer 1 and assert the process exits and the control socket is removed. (t_db304eb7)
- [x] 7.4 Add `e2e-disconnect` Makefile target and include it in the `e2e` aggregate. (t_db304eb7)

## 8. Verification

- [x] 8.1 Run `cargo check --workspace` and fix any compile errors. (t_0cb9346f)
- [x] 8.2 Run `cargo test --workspace` and fix any test failures. (t_0cb9346f)
- [x] 8.3 Run `make e2e-build` and `bash -n e2e/*.sh`. (t_db304eb7)
- [x] 8.4 Run `openspec validate --changes` and confirm the new change passes. (t_0cb9346f)

## Workflow follow-up

- Review the implementation and spec deltas.
- Archive the change after review and tests pass.
- Remaining work: `lanlink-desktop` tray/dashboard integration (t_ebf81f34), Makefile target for `e2e/disconnect.sh` and full e2e run.
