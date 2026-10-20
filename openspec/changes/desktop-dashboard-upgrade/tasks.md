# Tasks

## 1. Extend shared status types

- [ ] 1.1 Add new fields to `PeerStatus` in `crates/lanlink-proto/src/status.rs` (`transport`, `tx_packets`, `rx_packets`, `session_state`, `uptime_seconds`, `mac`, `addresses`) and verify `cargo test -p lanlink-proto` passes.
- [ ] 1.2 Add `SessionState` enum to `crates/lanlink-proto/src/status.rs` (`initial`, `handshake`, `established`, `failed`, `unknown`) and verify serialization round-trips.
- [ ] 1.3 Add network summary fields to `DaemonStatus` (`peer_count`, `relay_share_percent`) and verify the existing snapshot test compiles.

## 2. Extend the switch with packet counters and peer uptime

- [ ] 2.1 Add `tx_packets` and `rx_packets` counters to `PeerState` in `crates/lanlink-switch/src/lib.rs`, increment them where `tx_bytes`/`rx_bytes` are updated, and verify the existing switch tests pass.
- [ ] 2.2 Add `first_seen: Instant` to `PeerState`, copy it through `peer_stats()` into a new `PeerStat.uptime_seconds` field, and verify `cargo test -p lanlink-switch` passes.

## 3. Extend the daemon snapshot and status socket protocol

- [ ] 3.1 Update `StatusState::snapshot()` in `crates/lanlinkd/src/status.rs` to populate the new `PeerStatus` and `DaemonStatus` fields from `PeerStat` and registry state; verify with `cargo test -p lanlinkd snapshot_classifies_paths_and_merges_meta`.
- [ ] 3.2 Add `LeaveNetwork`, `RepunchPeer { node_id }`, and `SetGateway { enabled }` variants to `StatusRequest` in `crates/lanlink-proto/src/status.rs` and add a matching `StatusResponse::Ack` variant.
- [ ] 3.3 Add a control command channel to `StatusState` in `crates/lanlinkd/src/status.rs` and route the new `StatusRequest` variants to it; verify the handler returns `Ack` for recognized commands and `Error` for unknown/malformed requests.

## 4. Wire daemon control commands into the runtime

- [ ] 4.1 In `crates/lanlinkd/src/main.rs`, add a `DaemonCommand` enum and a receiver task that handles `LeaveNetwork` by triggering the global shutdown/leave flow; verify that sending `leave_network` causes the daemon to leave the network.
- [ ] 4.2 In `crates/lanlinkd/src/main.rs`, handle `RepunchPeer` by adding the target node ID to the punching set and signaling the puncher; verify that a re-punch attempt begins after the command.
- [ ] 4.3 In `crates/lanlinkd/src/main.rs`, handle `SetGateway` by calling `GatewayManager::enable`/`disable` and updating `DaemonInfo::gateway_enabled`; verify the next snapshot reflects the new state and that an unsupported platform returns an error response.

## 5. Add new Tauri invoke commands

- [ ] 5.1 Add `leave_network`, `repunch_peer`, and `set_gateway` Tauri commands in `crates/lanlink-desktop/src/commands.rs` that serialize the new `StatusRequest` variants and send them over the resolved status socket.
- [ ] 5.2 Register the new commands in `crates/lanlink-desktop/src/main.rs` and verify the app builds with `cargo build -p lanlink-desktop`.
- [ ] 5.3 Add a `set_tray_summary(summary)` command in `crates/lanlink-desktop/src/commands.rs` that updates the tray icon tooltip, and wire it into the tray builder in `main.rs`.

## 6. Build the richer desktop SPA dashboard

- [ ] 6.1 In `crates/lanlink-desktop/assets/index.html`, add network summary cards, a peer actions column, a pause-polling toggle, a leave-network button with confirmation, a gateway toggle, and a peer detail drawer; verify the page renders in the Tauri window.
- [ ] 6.2 In `crates/lanlink-desktop/assets/style.css`, add styles for summary cards, sparklines, detail drawer, copy buttons, confirmation overlay, and paused state; verify there are no visual regressions on the existing tabs.
- [ ] 6.3 In `crates/lanlink-desktop/assets/app.js`, update `renderDashboard()` to show `peer_count` and `relay_share_percent`; verify the values update with each snapshot.
- [ ] 6.4 In `crates/lanlink-desktop/assets/app.js`, update `renderPeers()` to show the new per-peer fields and action buttons (detail, copy IP, reconnect); verify peer rows render without errors.
- [ ] 6.5 In `crates/lanlink-desktop/assets/app.js`, implement the detail drawer with full counters, address list, and SVG sparklines using a 60-second ring buffer; verify the drawer opens and sparklines scale correctly.
- [ ] 6.6 In `crates/lanlink-desktop/assets/app.js`, implement copy-to-clipboard for peer IP/name using the Tauri clipboard API with a fallback; verify the clipboard contains the expected string.
- [ ] 6.7 In `crates/lanlink-desktop/assets/app.js`, wire `leave_network`, `repunch_peer`, and `set_gateway` invocations behind confirmation/toast flows; verify each command invokes the matching Tauri command and shows the result toast.

## 7. Add adaptive polling and tray UX

- [ ] 7.1 In `crates/lanlink-desktop/assets/app.js`, implement focus-aware polling: 1000 ms when focused/visible, 5000 ms when blurred/hidden, and paused when the user toggles pause; verify via logging that the interval changes.
- [ ] 7.2 In `crates/lanlink-desktop/assets/app.js`, compute peer join/leave/path-downgrade toasts by diffing consecutive snapshots; verify toasts appear when a peer is added, removed, or degrades from direct to relay.
- [ ] 7.3 In `crates/lanlink-desktop/src/main.rs`, forward the latest snapshot summary to the tray tooltip via `set_tray_summary` and update the tooltip when hidden; verify the tooltip shows network ID and peer count on hover.

## 8. Keep the web fallback SPA in sync

- [ ] 8.1 In `crates/lanlink-web/src/assets/app.js`, update `renderDashboard()` and `renderPeers()` to display the new daemon and peer fields in read-only form; verify the web build still serves the dashboard without errors.
- [ ] 8.2 In `crates/lanlink-web/src/assets/app.js`, keep the `invoke` fallback map read-only and ensure new control commands throw a clear error when running in a browser; verify the error message is shown in the UI.

## 9. Verification and integration

- [ ] 9.1 Run `cargo test --workspace` and fix any compile or test failures caused by the new fields or commands.
- [ ] 9.2 Start `lanlinkd` and `lanlink-desktop` locally, join a test network with two peers, and verify the dashboard shows the new fields, sparklines update, path-downgrade toasts appear, and control commands work.
- [ ] 9.3 Verify the tray tooltip updates when the window is hidden and that adaptive polling switches between 1 s and 5 s intervals.

## Workflow follow-up

- Archive the change after the project's review requirements are satisfied.
- Verify the archived spec deltas merged into `openspec/specs/` correctly.
