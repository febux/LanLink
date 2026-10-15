# Windows packaging

LanLink on Windows consists of three parts:

1. **`lanlinkd.exe`** — the daemon. It talks to the
   [Wintun](https://www.wintun.net/) driver for the virtual IP adapter.
2. **The Wintun driver** — `wintun.dll` (bundled with WireGuard, MIT
   licensed). The MSI installs it next to `lanlinkd.exe`. Wintun is a
   layer-3 interface, so the Windows data plane carries IP traffic only.
3. **A Windows service** — `LanLinkDaemon` runs automatically and reads
   `C:\Program Files\LanLink\lanlinkd.toml`.
4. **GUI front-ends** — the MSI can include the native desktop UI
   (`lanlink-desktop.exe`), the browser WebUI (`lanlink-web.exe`), the terminal
   UI (`lanlink-tui.exe`), and the unified launcher (`lanlink-gui.exe`).

## GUI-first setup (no terminal)

After installing the MSI, everything can be managed from the **LanLink**
desktop app (Start Menu → LanLink):

1. Open the app. On the dashboard, enter a **network name** and **password**
   (plus the coordinator URL, if not self-hosting on this machine) and click
   **Start network**.
2. Accept the UAC prompt — the app runs `lanlinkd service up` elevated for
   you: it writes `C:\Program Files\LanLink\lanlinkd.toml` + the secret file,
   installs the `LanLinkDaemon` service if missing, and starts it.
3. The dashboard then shows the daemon status, peers, and connection paths,
   read live from the daemon over `127.0.0.1:9001` (TCP loopback status
   socket configured in the default config; port is `9000 + network id`).
4. **Install/Start/Stop service** buttons and the output log in the same
   panel cover the remaining service operations. Each shows a UAC prompt on
   demand; no terminal required.

The terminal commands below remain equivalent and fully supported.

## One-line install

Run in an **elevated PowerShell**:

```powershell
irm https://raw.githubusercontent.com/lanlink/lanlink/main/dist/windows/install.ps1 | iex
```

Then create or join a network:

```powershell
lanlinkd service up --name gaming-night --password "s3cret"
```

Or open **LanLink** from the Start Menu.

## Build (from any OS with cross toolchain, or on Windows)

```
cargo build --release -p lanlinkd -p lanlinkctl -p lanlink-desktop -p lanlink-web -p lanlink-tui -p lanlink-gui -p lanlink-relay -p coordinator
```

To build the MSI:

```
make build-msi                # builds with all GUI front-ends (default)
make build-msi LL_GUI=desktop # desktop GUI only
make build-msi LL_GUI=web     # web UI only
```

## Service management (run as Administrator)

```powershell
> lanlinkd service install       # register service (auto-start)
> lanlinkd service up --name gaming-night --password "s3cret"  # create/join + start
> lanlinkd service status
> lanlinkd service stop
> lanlinkd service uninstall
```

Config lives at `C:\Program Files\LanLink\lanlinkd.toml` (same format as
`dist/lanlinkd.example.toml`).

## MSI

The MSI (`dist/windows/lanlink.wxs` + `build-msi.sh`, WiX v4+) installs:

- `C:\Program Files\LanLink\` — all binaries (`lanlinkd.exe`, `lanlinkctl.exe`,
  optional GUI binaries, `wintun.dll`), the default live config
  `lanlinkd.toml`, the example config, and adds the dir to the system `PATH`
- the `LanLinkDaemon` Windows service (auto-start, LocalSystem), registered but
  **not** started during install and fully removed on uninstall
- the Microsoft Visual C++ Redistributable (x64), installed silently before
  the service is registered, so the Rust binaries run on a clean Windows
  machine without a separate redist download. Skipped automatically when
  `vcruntime140.dll` is already present; build with `SKIP_VCREDIST=1` to
  leave it out (the exe also remains in `C:\Program Files\LanLink\` for
  manual re-runs)
- a Start Menu shortcut for `lanlink-desktop.exe` when the desktop GUI is
  included

Build from the repo root (on Windows, or on Linux/macOS with `cargo-xwin`):

```
make build-msi                       # or: bash dist/windows/build-msi.sh
```

The script stages the binaries, bundles `wintun.dll` (downloaded from
[wintun.net](https://www.wintun.net/), MIT licensed — pin `WINTUN_SHA256` in
release CI), bundles the VC++ redistributable (downloaded from
`aka.ms` — pin `VCREDIST_SHA256` in release CI, or pass `VCREDIST_EXE` /
`SKIP_VCREDIST=1`), and builds the MSI.

Requirements:

- `curl`, `unzip`, and an MSVC target toolchain when cross-building.
- A working WiX toolchain **or** a container runtime:
  - **Windows:** install the WiX CLI with `dotnet tool install --global wix`.
  - **Linux/macOS:** the .NET WiX CLI installs but is unsupported and produces
    invalid MSIs. The script auto-detects this and falls back to the
    `jkroepke/wixtoolset` Docker image (requires Docker or Podman). Override
    the image with `WIX_DOCKER_IMAGE=...`.

See the script header for all knobs (`VERSION`, `BIN_DIR`, `WINTUN_DLL`,
`VCREDIST_EXE`, `SKIP_BUILD`, `SKIP_VCREDIST`, `LL_GUI`, `WIX_DOCKER_IMAGE`,
`WIX_CONTAINER_RUNTIME`).

After installing, the service is registered but not running. Configure a
network and start the service with the one-line setup:

```powershell
> lanlinkd service up --name gaming-night --password "s3cret"
```

Alternatively, edit `C:\Program Files\LanLink\lanlinkd.toml` and the referenced
secret file, then run:

```powershell
> sc start LanLinkDaemon
```

Manual service installation (no MSI) remains supported:

```powershell
> lanlinkd service install
> lanlinkd service start ""
```

## Firewall

The per-network inbound firewall profile (blocking SMB/RDP/SSH/NetBIOS on
public networks) uses nftables on Linux. On Windows it maps to Windows
Defender Firewall rules scoped to the Wintun interface (post-v1 task; see
PLAN.md §3.7).
