# Proposal

## Why

LanLink ships a native desktop GUI (`lanlink-desktop.exe`) in the Windows MSI, but on Windows it cannot yet replace the terminal. Two blockers exist:

1. The daemon's read-only status API is a Unix domain socket, so the desktop app (and web/TUI frontends) cannot read daemon status on Windows.
2. The desktop app does not install, start, stop, or configure the `LanLinkDaemon` Windows service, so users still need an elevated PowerShell to run `lanlinkd service up`.

This change makes the Windows desktop app a first-class, terminal-free manager.

## What Changes

- Add a **TCP loopback status transport** for `lanlinkd` on Windows. The daemon binds `127.0.0.1:<port>` where the port is derived from the network id (default `9000 + network_id`, override via `status.socket` config). The protocol stays newline-delimited JSON, identical to the Unix socket.
- Make `lanlinkd service up` on Windows set a TCP status socket in the generated `lanlinkd.toml` and use the network name for the secret file (matching existing Linux behavior).
- Update `lanlink-tui` and `lanlink-web` to detect and use the TCP status socket on Windows.
- Update `lanlink-desktop` (Tauri) to:
  - Detect the daemon status endpoint on Windows (`127.0.0.1:<port>`).
  - Provide Tauri commands to install/start/stop/query the `LanLinkDaemon` service by shelling out to `lanlinkd service ...` (reuses the existing, tested service wrapper).
  - Provide a Tauri command to configure + start a network from the GUI: write `lanlinkd.toml` and the password secret, then run `lanlinkd service up --name <name> --password <password>`.
- Bundle the **Microsoft Visual C++ Redistributable (x64)** in the MSI and install it silently, so a clean Windows VM does not need a separate redist download.
- Update the Windows packaging docs.

## Capabilities

### New Capabilities

- `windows-status-transport`: daemon serves read-only NDJSON status over TCP loopback on Windows.
- `windows-desktop-service-control`: desktop GUI installs/starts/stops the LanLink service and creates networks from the UI.

### Modified Capabilities

- `windows-installer`: now includes the VC++ redist so binaries run on clean Windows.

## Impact

- `crates/lanlinkd/src/status.rs`
- `crates/lanlinkd/src/service.rs`
- `crates/lanlinkd/src/config.rs`
- `crates/lanlink-desktop/src/commands.rs`
- `crates/lanlink-desktop/src/main.rs`
- `crates/lanlink-web/src/api.rs`
- `crates/lanlink-tui/src/daemon.rs`
- `dist/windows/lanlink.wxs`
- `dist/windows/build-msi.sh`
- `dist/windows/README.md`
- `openspec/changes/windows-gui-control/`
