# Proposal: Richer Desktop Dashboard

## Why

The current `lanlink-desktop` Tauri shell renders the LanLink web SPA as a read-only dashboard: it polls the daemon status socket once a second and displays a static peer table with IP, latency, path, endpoint, last seen, and byte counters. Operators cannot act on the data (leave a network, reconnect a peer, inspect a peer, copy addresses, toggle gateway mode) and the tray integration is limited to Show/Quit. A richer, interactive dashboard will make the desktop app the primary day-to-day control surface and reduce the need to drop to `lanlinkctl` or service commands.

## What Changes

- Extend the per-peer status snapshot with richer connection metadata: explicit transport/path label, latency, TX/RX byte and packet counters, live rate/sparkline, session/Noise handshake state, peer uptime, virtual MAC, full address list, and last-seen.
- Add network-level summary cards: total peers, relay usage share, firewall profile, and gateway status.
- Add interactivity in the desktop SPA: leave network with confirmation, reconnect/re-punch a peer, toggle gateway mode, open a peer detail panel/drawer, copy peer IP/name, and pause/resume polling.
- Make polling adaptive: faster when the window is focused, slower when tray-only.
- Improve tray UX: tooltip/quick stats and toast notifications for peer join/leave and path downgrade (direct→relay).
- Add new Tauri invoke commands (`leave_network`, `repunch_peer`, `set_gateway`) in `crates/lanlink-desktop/src/commands.rs`.
- Extend the daemon status socket protocol and the switch/registry data model to expose the new fields and to accept the new control commands. This change explicitly marks which work is daemon-side and keeps those additions minimal.
- Keep the vanilla-JS SPA architecture; no framework migration.

## Capabilities

### New Capabilities

- `desktop/dashboard`: The LanLink desktop dashboard presents live daemon status, rich peer and network summaries, and interactive controls for managing the local network membership and individual peers.
- `desktop/tray`: The desktop shell integrates with the system tray, including adaptive status polling, tooltip/quick stats, and toast notifications for membership and path-change events.
- `daemon/status-control`: The daemon's local status socket exposes richer read-only peer/network metadata and accepts a minimal set of write/control commands (leave network, re-punch a peer, toggle gateway mode) from authorized local frontends.

### Modified Capabilities

- None. The existing `windows-gateway` capability defines gateway behavior on Windows; this change only adds UI and a daemon control hook to toggle already-implemented gateway mode, not change the gateway semantics.

## Impact

- `crates/lanlink-desktop/src/commands.rs` — new invoke commands and proxying to the daemon/control surface.
- `crates/lanlink-desktop/src/main.rs` — tray tooltip, focus-aware polling hooks, and new command registration.
- `crates/lanlink-desktop/assets/{index.html,style.css,app.js}` — richer dashboard UI, detail drawer, sparklines, copy actions, confirmation dialogs, toast notifications, and adaptive polling.
- `crates/lanlink-web/src/assets/app.js` — keep the fallback web SPA in sync where applicable (it remains read-only for browser dev mode).
- `crates/lanlink-proto/src/status.rs` — extended `PeerStatus`, `DaemonStatus`, and `StatusRequest` types.
- `crates/lanlink-switch/src/lib.rs` — per-peer packet counters and peer uptime tracking.
- `crates/lanlinkd/src/status.rs` — richer snapshot assembly and new control request handling on the status socket.
- `crates/lanlinkd/src/main.rs` — wiring for leave/re-punch/gateway control into the daemon runtime.
- No public network protocol changes; no coordinator API changes.
