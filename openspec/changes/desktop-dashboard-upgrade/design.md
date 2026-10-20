# Design: Richer Desktop Dashboard

## Context

`lanlink-desktop` is a Tauri v2 shell that bundles the same vanilla-JS SPA used by `lanlink-web`. Today it registers five data/commands invoke handlers (`get_snapshot`, `list_directory`, `create_network`, `join_network`, plus coordinator/service helpers) and renders a simple dashboard + peer table. The SPA polls `get_snapshot` every second and `list_directory` every 10 seconds. The system tray has only Show and Quit.

The daemon's status socket is read-only NDJSON over a Unix domain socket (Linux/macOS) or TCP loopback (Windows). The `StatusSnapshot` already carries daemon status and a peer list with latency, path, endpoint, byte counters, and last-seen. The switch (`lanlink-switch`) tracks per-peer `tx_bytes`, `rx_bytes`, `last_seen`, `rtt`, and `transport`, but not packets, uptime, session state, or address history. Gateway mode is initialized from config and reported in the snapshot, but cannot be toggled at runtime.

See `proposal.md` for the motivation and `specs/` for the behavior contract.

## Goals / Non-Goals

**Goals:**
- Make the desktop dashboard the primary interactive control surface for local network state.
- Surface richer per-peer and network-level metadata without changing public protocols.
- Add safe local control commands (leave network, re-punch peer, toggle gateway) through the existing status socket.
- Adapt polling to window visibility and improve tray integration.
- Keep the SPA vanilla JS; no framework migration.

**Non-Goals:**
- Replacing `lanlinkctl` or the coordinator REST API.
- Adding a persistent history database or long-term metrics store.
- Cross-machine control; all control commands remain local-only.
- Rewriting the web SPA into a framework app.

## Decisions

### 1. Where to add new fields: extend `lanlink-proto` status types

**Decision:** Add new fields to `PeerStatus` and `DaemonStatus` in `crates/lanlink-proto/src/status.rs`.

**Rationale:** Both the daemon, TUI, web fallback, and desktop consume the same types. Extending the shared structs keeps the wire format consistent and avoids ad-hoc JSON in the SPA.

**New fields:**
- `PeerStatus`: `transport: Transport`, `tx_packets: u64`, `rx_packets: u64`, `session_state: SessionState`, `uptime_seconds: u64`, `mac: [u8; 6]` (or a formatted string), `addresses: Vec<SocketAddr>`.
- `DaemonStatus`: `peer_count: u32`, `relay_share_percent: u8`.

### 2. How to compute the new fields

**Decision:** Compute everything inside `StatusState::snapshot()` in `crates/lanlinkd/src/status.rs` from existing daemon state.

**Rationale:** The frontend should receive ready-to-render values so the dashboard stays simple and consistent across web and desktop builds.

**Mapping:**
- `transport` comes from the switch's `PeerStat.transport`.
- `tx_packets`/`rx_packets` require new counters in `PeerState` (incremented where `tx_bytes`/`rx_bytes` are updated).
- `session_state` is derived from `PeerCrypto` in the switch: map `None`/uninitialized → `initial`, in-progress handshake → `handshake`, ready session → `established`, failures → `failed`.
- `uptime_seconds` is `last_seen.elapsed()` from the moment the peer was first seen; we add a `first_seen: Instant` to `PeerState`.
- `mac` is computed deterministically from the virtual IP using the existing `mac_from_virtual_ip` helper in `lanlink-gateway`.
- `addresses` is the switch's stored candidate/endpoint list.
- `peer_count` and `relay_share_percent` are computed from the peer list.

### 3. Sparklines and live rates without a chart library

**Decision:** Implement 60-second sparklines as inline SVG paths in vanilla JS, storing a fixed-size ring buffer of `(tx_rate, rx_rate)` samples in the SPA state.

**Rationale:** No new dependency, consistent with the existing vanilla-JS architecture, and sufficient for a 60-sample mini chart.

**Approach:** On each snapshot, compute `(current_tx - previous_tx) / poll_interval` and push the rate (in bytes/sec) into `state.sparklines[peerId].tx` and `.rx` arrays capped at 60 entries. Render as `<svg><polyline/></svg>` scaled to the array min/max.

### 4. Adaptive polling

**Decision:** Let the frontend control the poll interval based on window focus, with Tauri exposing a small helper if needed.

**Rationale:** The snapshot loop already lives in `app.js`. Using the browser `focus`/`blur` and `visibilitychange` events works in both Tauri and browser dev mode. Tauri-specific window events are a fallback.

**Intervals:**
- Focused/visible: 1000 ms (current behavior).
- Blurred/hidden: 5000 ms.
- Paused: polling stops entirely; manual refresh remains.

Tauri shell can additionally emit a `window-focus-changed` event if we want tray-only state (no web window) to influence polling.

### 5. Toast notifications

**Decision:** Use the existing in-app toast element in `app.js` for all notifications; do not add a native notification dependency.

**Rationale:** The user asked for "toast notifications" in the context of the dashboard UX. The existing toast is already used for service/coordinator results. Native OS notifications would add a new permission and platform complexity.

**Triggers computed by diffing consecutive snapshots:**
- Peer appears → "<ip> joined".
- Peer disappears → "<ip> left".
- Peer path changes from `direct`/`tcp_direct` to `relay`/`tcp_relay` → "<ip> path degraded to relay".

### 6. Tray tooltip

**Decision:** Use Tauri v2's `TrayIcon::set_tooltip` from `main.rs`, updated whenever a snapshot is fetched by the Rust side or by forwarding the latest summary to Rust from the frontend.

**Rationale:** Tooltips are a shell responsibility. Updating from Rust avoids needing a second status fetch path.

**Two options considered:**
- (a) Rust fetches the snapshot independently on a timer and sets the tooltip.
- (b) Frontend sends the summary to Rust via a new `update_tray` invoke.

**Chosen:** (b) is simpler and reuses the existing polling loop. Add a new command `set_tray_summary(summary)` that the SPA calls after each snapshot when the window is hidden.

### 7. New invoke commands and their daemon-side needs

**Decision:** Add three new Tauri commands in `crates/lanlink-desktop/src/commands.rs`: `leave_network`, `repunch_peer`, `set_gateway`. Each writes a control request to the daemon status socket.

**Rationale:** Keeps the frontend command surface small and consistent with the existing `get_snapshot` flow.

**Daemon-side additions (explicitly marked as daemon scope):**
- Extend `StatusRequest` in `lanlink-proto` with `LeaveNetwork`, `RepunchPeer { node_id }`, `SetGateway { enabled }`.
- Extend `StatusResponse` with an `Ack { ok: bool }` or reuse `Error`.
- In `crates/lanlinkd/src/status.rs`, add a control channel (e.g., `tokio::sync::mpsc::UnboundedSender<DaemonCommand>`) to `StatusState` and route the new requests to it.
- In `crates/lanlinkd/src/main.rs`, spawn a task that receives `DaemonCommand` and calls the appropriate runtime action:
  - `LeaveNetwork` → trigger global shutdown/leave flow.
  - `RepunchPeer` → add the node ID to the punching set and signal the puncher.
  - `SetGateway` → call `GatewayManager::enable`/`disable` and update the in-flight `DaemonInfo`.

### 8. Gateway toggle scope and limitations

**Decision:** `set_gateway` is a runtime toggle only when the daemon was built with gateway support and the platform allows it (Linux/Windows). macOS returns an error because the gateway crate reports `GatewayError::Unsupported`.

**Rationale:** Matches existing `GatewayManager` behavior. The UI disables the toggle when the capability is unavailable.

### 9. Keep web SPA read-only

**Decision:** The `lanlink-web` fallback (`crates/lanlink-web/src/assets/app.js`) displays the richer dashboard data but does not implement control commands. The `invoke` map in the desktop SPA will throw for `leave_network`, `repunch_peer`, and `set_gateway` when running in a normal browser.

**Rationale:** The control commands require local daemon access and elevated/network privileges; exposing them through the HTTP web server would be unsafe. The web build remains a dev/status viewer.

## Risks / Trade-offs

- [Risk] Extending `PeerStatus` changes the status wire format and may break the TUI if it assumes a fixed schema.
  → Mitigation: Add only optional/defaulted fields where possible, and update `crates/lanlink-tui/src/app.rs` in the same change to consume the new fields or ignore them safely.
- [Risk] Packet counters require updating hot packet paths in the switch.
  → Mitigation: Increment counters with `Relaxed` ordering and only when a packet is actually forwarded to/from a peer; measure overhead in a benchmark task.
- [Risk] Runtime gateway toggling changes firewall/routing state and could disrupt traffic.
  → Mitigation: Gateway toggle shows a confirmation toast, returns errors from the gateway manager, and logs the result. The UI reflects the actual daemon state on the next snapshot rather than optimistic state.
- [Risk] The status socket accepting write commands increases the local attack surface.
  → Mitigation: Keep the listener on loopback/Unix socket only (already the case); reject non-loopback TCP connections; document that any local user can issue these commands because the data is not secret and the daemon runs with network privileges anyway.
- [Risk] Tauri v2 tray tooltip API may behave differently across platforms.
  → Mitigation: Gate tooltip updates behind `#[cfg(not(target_os = "linux"))]` if Linux tooltips are unsupported, and fall back to a no-op.

## Migration Plan

No external migration is needed. The change is additive:
1. Merge proto/struct extensions first so downstream crates compile.
2. Merge daemon status-socket extensions.
3. Merge desktop SPA and Tauri shell changes.
4. Update `lanlink-tui` display if desired (optional; the new fields are additive).
5. Rollback: revert the commit; older frontends will simply ignore the extra fields or, for the TUI, may need a pinned version.

## Open Questions

- Should the peer detail drawer also show the full node ID (hex) in addition to the virtual IP? The node ID is required for `repunch_peer` but is not currently user-facing.
- Should the desktop app request OS notification permissions for path-downgrade warnings, or are in-app toasts sufficient?
- Should `leave_network` also uninstall the Windows service / stop the daemon, or should it only leave the network and leave the service running? (Recommendation: leave the daemon running so the user can create/join another network from the same app instance.)
