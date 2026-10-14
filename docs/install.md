# Installation Guide

This guide shows how to install LanLink on Linux and Windows with **one command**,
picking the front-end you want: native desktop app, browser WebUI, or terminal UI.

For maintainers, see [`dist/publish-release.sh`](../dist/publish-release.sh) or
`make dist-publish` to build and publish all distribution packages to GitHub
Releases in one command.

## What you get

Every install method below gives you the same core binaries:

- `lanlinkd` — the daemon that creates the virtual LAN interface
- `lanlinkctl` — command-line control tool
- `lanlink-coordinator` — self-hosted membership server
- `lanlink-relayd` — UDP relay fallback server
- systemd units: `lanlinkd@.service`, `lanlink-coordinator.service`,
  `lanlink-relayd.service`

You can optionally include one or more GUI front-ends:

| Front-end | Binary | Description |
|---|---|---|
| Native desktop | `lanlink-desktop` | Tauri-based window with system tray |
| Browser WebUI | `lanlink-web` | Serves the UI on `http://127.0.0.1:8686` |
| Terminal UI | `lanlink-tui` | Runs in any terminal |
| Unified launcher | `lanlink-gui` | One binary that can start TUI/Web/Desktop |

## One-command install

### Linux

```bash
# Native desktop app (default)
curl -fsSL https://raw.githubusercontent.com/lanlink/lanlink/main/dist/install.sh | sh

# Terminal UI
curl -fsSL https://raw.githubusercontent.com/lanlink/lanlink/main/dist/install.sh | sh -s -- --gui tui

# Browser WebUI
curl -fsSL https://raw.githubusercontent.com/lanlink/lanlink/main/dist/install.sh | sh -s -- --gui web

# Pick a different package format
curl -fsSL ... | sh -s -- --pkg deb       # Debian/Ubuntu (default on apt systems)
curl -fsSL ... | sh -s -- --pkg rpm       # Fedora/RHEL
curl -fsSL ... | sh -s -- --pkg tarball  # Any distro, no package manager
curl -fsSL ... | sh -s -- --pkg appimage  # Single-file desktop app
curl -fsSL ... | sh -s -- --pkg flatpak   # Flatpak bundle
curl -fsSL ... | sh -s -- --pkg snap      # Snap package
```

### Windows

Run in an **elevated PowerShell**:

```powershell
irm https://raw.githubusercontent.com/lanlink/lanlink/main/dist/windows/install.ps1 | iex
```

Then create or join a network:

```powershell
lanlinkd service up --name gaming-night --password "s3cret"
# or open LanLink from the Start Menu
```

### macOS

Download the latest `.pkg` from [GitHub Releases](https://github.com/lanlink/lanlink/releases)
and double-click it, or run:

```bash
sudo installer -pkg lanlink-*-macos-*.pkg -target /
```

## Manual package downloads

All release artifacts are published to [GitHub Releases](https://github.com/lanlink/lanlink/releases):

| Format | File | Best for |
|---|---|---|
| Debian/Ubuntu | `lanlink_*_amd64.deb` | apt/dpkg users |
| Fedora/RHEL | `lanlink-*-1.*.rpm` | dnf/yum users |
| Windows | `lanlink-*-x64.msi` | One-click MSI install |
| macOS | `lanlink-*-macos-*.pkg` | Native installer |
| Portable tarball | `lanlink-*-linux-*.tar.gz` | Any Linux distro, manual install |
| AppImage | `lanlink-*-linux-x86_64.AppImage` | Run without installing |
| Flatpak | `lanlink-*-linux-x86_64.flatpak` | Flatpak users |
| Snap | `lanlink_*_amd64.snap` | snap users |

## Package-specific instructions

### Debian / Ubuntu (.deb)

Released `.deb` packages include the native desktop GUI by default.

```bash
# Download from GitHub Releases, then:
sudo dpkg -i lanlink_*_amd64.deb
sudo apt-get install -f   # install any missing dependencies
```

Or build from source:

```bash
make build-deb LL_GUI=all   # include all front-ends
```

### Fedora / RHEL (.rpm)

```bash
sudo dnf install ./lanlink-*-1.x86_64.rpm
```

### Arch Linux (AUR)

```bash
cd dist/aur
makepkg -si
```

### Portable Linux tarball

```bash
tar -xzf lanlink-*-linux-x86_64.tar.gz
sudo ./lanlink-*/install-local.sh
```

This installs to `/usr/local` and registers systemd units.

### AppImage

```bash
chmod +x lanlink-*-linux-x86_64.AppImage
./lanlink-*-linux-x86_64.AppImage
```

### Flatpak

```bash
flatpak install ./lanlink-*-linux-x86_64.flatpak
flatpak run com.lanlink.LanLink
```

### Snap

```bash
sudo snap install --dangerous --devmode ./lanlink_*_amd64.snap
lanlink
```

## Start the service after install

### Linux

```bash
# One-command ad-hoc network (creates config + secret + systemd unit)
LANLINK_PASSWORD='s3cret' sudo -E lanlinkd service up gaming-night

# Or manage the config manually
sudo cp /etc/lanlink/lanlinkd.example.toml /etc/lanlink/gaming-night.toml
sudoedit /etc/lanlink/gaming-night.toml
sudo lanlinkd service install
sudo lanlinkd service start gaming-night
sudo lanlinkd service enable gaming-night
```

### Windows

```powershell
# One command: resolve/create the network, write config, start the service
lanlinkd service up --name gaming-night --password "s3cret"

# Or manage the config manually
copy "C:\ProgramData\LanLink\lanlinkd.example.toml" "C:\ProgramData\LanLink\lanlinkd.toml"
notepad "C:\ProgramData\LanLink\lanlinkd.toml"
sc start LanLinkDaemon
```

### macOS

```bash
sudo launchctl load /Library/LaunchDaemons/com.lanlink.lanlinkd.plist
```

## Running the GUI after install

```bash
# Native desktop app
lanlink-desktop

# Browser WebUI (auto-opens browser by default)
lanlink-web

# Terminal UI
lanlink-tui

# Unified launcher
lanlink-gui --mode desktop
lanlink-gui --mode web
lanlink-gui --mode tui

# Full dev stack: coordinator + daemon + front-end, no manual service setup
lanlink-gui --dev --mode desktop
```

## Build selector reference

| Selector | Debian / Ubuntu | Arch Linux | What is built |
|---|---|---|---|
| (none) | `make build-deb` | `makepkg -si` | Core only |
| TUI | `make build-deb LL_GUI=tui` | `LL_GUI=tui makepkg -si` | `lanlink-tui` + `lanlink-gui` |
| WebUI | `make build-deb LL_GUI=web` | `LL_GUI=web makepkg -si` | `lanlink-web` + `lanlink-gui` |
| Desktop | `make build-deb LL_GUI=desktop` | `LL_GUI=desktop makepkg -si` | `lanlink-desktop` + `lanlink-gui` |
| All | `make build-deb LL_GUI=all` | `LL_GUI=all makepkg -si` | All front-ends + `lanlink-gui` |

## Troubleshooting

- `dpkg: dependency problems` → run `sudo apt-get install -f` after `dpkg -i`.
- Desktop build fails on Linux → install `webkit2gtk-4.1` headers
  (`libwebkit2gtk-4.1-dev` on Debian/Ubuntu, `webkit2gtk-4.1` on Arch).
- Service fails to start → check `journalctl -u lanlinkd@gaming-night -e` on Linux
  or `sc query LanLinkDaemon` on Windows.
- Permission denied on TAP device → the unit needs `CAP_NET_ADMIN` and
  `CAP_NET_RAW`; the packaged `lanlinkd@.service` already sets these.
- Windows: the Wintun driver is bundled as `wintun.dll`; the virtual adapter
  appears the first time the daemon creates it after the Wintun backend is
  wired up.
