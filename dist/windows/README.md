# Windows packaging

LanLink on Windows consists of three parts:

1. **`lanlinkd.exe`** — the daemon. It talks to the
   [Wintun](https://www.wintun.net/) driver for the virtual Ethernet adapter.
2. **The Wintun driver** — `wintun.dll` (bundled with WireGuard, MIT
   licensed). The MSI installs it next to `lanlinkd.exe`.
3. **A Windows service** — `LanLinkDaemon` runs automatically and reads
   `C:\ProgramData\LanLink\lanlinkd.toml`.
4. **GUI front-ends** — the MSI can include the native desktop UI
   (`lanlink-desktop.exe`), the browser WebUI (`lanlink-web.exe`), the terminal
   UI (`lanlink-tui.exe`), and the unified launcher (`lanlink-gui.exe`).

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

Config lives at `C:\ProgramData\LanLink\lanlinkd.toml` (same format as
`dist/lanlinkd.example.toml`).

## MSI

The MSI (`dist/windows/lanlink.wxs` + `build-msi.sh`, WiX v4+) installs:

- `C:\Program Files\LanLink\` — all binaries (`lanlinkd.exe`, `lanlinkctl.exe`,
  optional GUI binaries, `wintun.dll`) and adds the dir to the system `PATH`
- `C:\ProgramData\LanLink\lanlinkd.example.toml` — reference example
- `C:\ProgramData\LanLink\lanlinkd.toml` — default live config the service
  reads; written only if absent, never overwritten on upgrade/repair (edit it
  to set your coordinator, network id and password), removed on uninstall
- the `LanLinkDaemon` Windows service (auto-start, LocalSystem), started on
  install and fully removed on uninstall
- a Start Menu shortcut for `lanlink-desktop.exe` when the desktop GUI is
  included

Build from the repo root (on Windows, or on Linux/macOS with `cargo-xwin`):

```
make build-msi                       # or: bash dist/windows/build-msi.sh
```

The script stages the binaries, bundles `wintun.dll` (downloaded from
[wintun.net](https://www.wintun.net/), MIT licensed — pin `WINTUN_SHA256` in
release CI), and runs `wix build`. Requirements: the `wix` CLI
(`dotnet tool install --global wix`), `curl`, `unzip`, and an MSVC target
toolchain when cross-building. See the script header for all knobs
(`VERSION`, `BIN_DIR`, `WINTUN_DLL`, `SKIP_BUILD`, `LL_GUI`).

After installing, the recommended first step is the one-line network setup:

```powershell
> lanlinkd service up --name gaming-night --password "s3cret"
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
