# Proposal

## Why

LanLink currently joins a network and runs until the process is killed. There is no clean way for a user to leave a network or stop the daemon without sending SIGKILL or relying on the service manager. This change adds graceful disconnect and stop paths, matching the UX of RadminVPN: the user can disconnect from the current network and reconnect later without restarting the daemon process.

## What Changes

- Extend the daemon's local socket from a read-only status API to a minimal control/status API. The same socket serves NDJSON snapshots and safe local commands (`disconnect`, `stop`, `connect`, `status`).
- Add a persistent daemon runtime for `lanlinkd run`/`up`/`service run`: the process stays alive after a disconnect, the TAP interface, firewall profile, gateway rules, and peer routes are cleaned up, and a later `connect` command can join again.
- Add graceful stop: `lanlinkctl daemon stop` (and a GUI tray action) asks the daemon to clean up and exit with code 0, instead of killing the process.
- Add a coordinator `POST /networks/:id/leave` endpoint and make the daemon notify the coordinator on disconnect (best-effort).
- Add `lanlinkctl daemon` commands (`status`, `disconnect`, `connect`, `stop`).
- Add Disconnect/Stop actions to the native desktop tray/menu and the TUI.
- Add an e2e scenario that verifies disconnect + reconnect without a daemon restart.

## Capabilities

### New Capabilities

- `daemon-control-socket`: local NDJSON control/status socket that accepts disconnect, stop, connect, and status commands.
- `graceful-disconnect`: leave the current mesh network and release TAP/firewall/routes while keeping the daemon alive.
- `graceful-stop`: request daemon shutdown through the control socket so cleanup runs before exit 0.
- `coordinator-leave`: best-effort membership leave notification from daemon to coordinator.
- `e2e-disconnect-reconnect`: automated two-peer test for disconnect + reconnect.

### Modified Capabilities

- `daemon-status`: snapshot now reports `connected: bool` so frontends can show a clear disconnected state.
- `lanlinkctl`: gains a `daemon` subcommand group for local daemon control.
- `lanlink-desktop`/`lanlink-tui`: expose Disconnect and Stop actions.

## Impact

- `crates/lanlink-proto/src/control.rs` (new)
- `crates/lanlink-proto/src/status.rs`
- `crates/lanlinkd/src/main.rs`
- `crates/lanlinkd/src/control.rs` (new)
- `crates/lanlinkd/src/status.rs`
- `crates/lanlinkd/src/session.rs` (new)
- `crates/lanlinkctl/src/main.rs`
- `crates/lanlink-desktop/src/main.rs`
- `crates/lanlink-desktop/src/commands.rs`
- `crates/lanlink-desktop/assets/index.html`, `app.js` (actions only)
- `crates/lanlink-tui/src/main.rs`, `app.rs`, `ui.rs`, `daemon.rs`
- `crates/coordinator/src/lib.rs`
- `crates/coordinator/src/api.rs`
- `crates/coordinator/src/db.rs`
- `e2e/disconnect.sh`
- `Makefile`
- `openspec/changes/disconnect-graceful-stop/`
