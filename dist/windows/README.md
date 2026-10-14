# Windows packaging

LanLink on Windows consists of three parts:

1. **`lanlinkd.exe`** — the daemon (this build). It talks to the
   [Wintun](https://www.wintun.net/) driver for the virtual Ethernet adapter.
   The Wintun backend is still a stub; until it lands, the daemon runs but
   cannot create the NIC.
2. **The Wintun driver** — `wintun.dll` (bundled with WireGuard, MIT
   licensed). Ship `wintun.dll` next to `lanlinkd.exe` or install it to
   `C:\Windows\System32\`.
3. **A Windows service** — `lanlinkd service install` registers the service
   with the SCM (automatic start). It runs:

   ```
   lanlinkd.exe service run --config C:\ProgramData\LanLink\lanlinkd.toml
   ```

## Build (from any OS with cross toolchain, or on Windows)

```
cargo build --release -p lanlinkd -p lanlinkctl
```

## Service management (run as Administrator)

```
> lanlinkd service install       # register service (auto-start)
> lanlinkd service start ""      # start
> lanlinkd service status
> lanlinkd service stop ""
> lanlinkd service uninstall
```

The empty string argument is the per-network instance name used on Linux
(`lanlinkd@<name>`); Windows runs a single instance and ignores it.

Config lives at `C:\ProgramData\LanLink\lanlinkd.toml` (same format as
`dist/lanlinkd.example.toml`).

## MSI

The MSI (`dist/windows/lanlink.wxs` + `build-msi.sh`, WiX v4+) installs:

- `C:\Program Files\LanLink\` — `lanlinkd.exe`, `lanlinkctl.exe`, `wintun.dll`,
  README (and adds the dir to the system `PATH`)
- `C:\ProgramData\LanLink\lanlinkd.example.toml` — never overwrites an
  operator-edited `lanlinkd.toml`
- the `LanLinkDaemon` Windows service (auto-start, LocalSystem), started on
  install and fully removed on uninstall

Build from the repo root (on Windows, or on Linux/macOS with `cargo-xwin`):

```
make build-msi                       # or: bash dist/windows/build-msi.sh
```

The script stages the binaries, bundles `wintun.dll` (downloaded from
[wintun.net](https://www.wintun.net/), MIT licensed — pin `WINTUN_SHA256` in
release CI), and runs `wix build`. Requirements: the `wix` CLI
(`dotnet tool install --global wix`), `curl`, `unzip`, and an MSVC target
toolchain when cross-building. See the script header for all knobs
(`VERSION`, `BIN_DIR`, `WINTUN_DLL`, `SKIP_BUILD`).

After installing, copy the example config to `lanlinkd.toml`, fill in your
network, and start the service:

```
> copy "C:\ProgramData\LanLink\lanlinkd.example.toml" "C:\ProgramData\LanLink\lanlinkd.toml"
> notepad "C:\ProgramData\LanLink\lanlinkd.toml"
> sc start LanLinkDaemon
```

Manual service installation (no MSI) remains supported:

```
> lanlinkd service install
> lanlinkd service start ""
```

## Firewall

The per-network inbound firewall profile (blocking SMB/RDP/SSH/NetBIOS on
public networks) uses nftables on Linux. On Windows it maps to Windows
Defender Firewall rules scoped to the Wintun interface (post-v1 task; see
PLAN.md §3.7).
