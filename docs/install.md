# Installation Guide

This guide shows how to install LanLink on Linux with **one command**, picking the
front-end you want: terminal UI (TUI), browser WebUI, or native desktop app.

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
| Terminal UI | `lanlink-tui` | Runs in any terminal |
| Browser WebUI | `lanlink-web` | Serves the UI on `http://127.0.0.1:8686` |
| Native desktop | `lanlink-desktop` | Tauri-based window with system tray |
| Unified launcher | `lanlink-gui` | One binary that can start TUI/Web/Desktop |

## One-command summary

```bash
# Debian / Ubuntu — build a .deb with the front-end you want, then install it
make dist-deb LL_GUI=tui   && sudo dpkg -i dist/deb/out/lanlink_*_amd64.deb
make dist-deb LL_GUI=web   && sudo dpkg -i dist/deb/out/lanlink_*_amd64.deb
make dist-deb LL_GUI=all   && sudo dpkg -i dist/deb/out/lanlink_*_amd64.deb

# Arch Linux — build and install from the AUR with the front-end you want
cd dist/aur && LL_GUI=tui   makepkg -si
cd dist/aur && LL_GUI=web   makepkg -si
cd dist/aur && LL_GUI=all   makepkg -si

# Or use systemd units only (no package manager)
sudo cp dist/systemd/*.service /etc/systemd/system/
sudo systemctl daemon-reload
```

## Debian / Ubuntu (.deb)

### 1. Build the package

Run this from the repository root:

```bash
# Default package: daemon + CLI + coordinator + relay
make dist-deb

# Add the terminal UI
make dist-deb LL_GUI=tui

# Add the browser WebUI
make dist-deb LL_GUI=web

# Add the native desktop UI
make dist-deb LL_GUI=desktop

# Install all front-ends at once
make dist-deb LL_GUI=all
```

You can also pass flags directly to the build script instead of using `make`:

```bash
bash dist/deb/build-deb.sh --with-tui
bash dist/deb/build-deb.sh --with-web
bash dist/deb/build-deb.sh --with-desktop
bash dist/deb/build-deb.sh --with-gui
```

The package is written to `dist/deb/out/lanlink_<version>-1_<arch>.deb`.

### 2. Install it

```bash
sudo dpkg -i dist/deb/out/lanlink_*_amd64.deb
sudo apt-get install -f   # install any missing dependencies
```

### 3. Start the service

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

## Arch Linux (AUR)

The repository ships a `PKGBUILD` in `dist/aur/` that can be used directly with
`makepkg` or submitted to the AUR.

### 1. Build and install

```bash
cd dist/aur

# Daemon + CLI + coordinator + relay only
makepkg -si

# With a specific front-end (export the selector first)
LL_GUI=tui   makepkg -si
LL_GUI=web   makepkg -si
LL_GUI=desktop makepkg -si
LL_GUI=all   makepkg -si
```

> **Desktop build note:** `LL_GUI=desktop` requires `webkit2gtk-4.1`
> development headers. Install it first with `sudo pacman -S webkit2gtk-4.1`.

### 2. Start the service

Same commands as Debian:

```bash
LANLINK_PASSWORD='s3cret' sudo -E lanlinkd service up gaming-night

# Or use the systemd template directly
sudo systemctl enable --now lanlinkd@gaming-night
```

## systemd only (no package manager)

If you already built the binaries or downloaded a release archive, you can
install just the systemd units:

```bash
sudo cp dist/systemd/lanlinkd@.service /etc/systemd/system/
sudo cp dist/systemd/lanlink-coordinator.service /etc/systemd/system/
sudo cp dist/systemd/lanlink-relayd.service /etc/systemd/system/
sudo systemctl daemon-reload

# Create the unprivileged runtime user the unit expects
sudo useradd -r -s /usr/sbin/nologin -M lanlink || true

# Start an instance
sudo install -d -m 0750 /etc/lanlink
sudo cp dist/lanlinkd.example.toml /etc/lanlink/gaming-night.toml
sudoedit /etc/lanlink/gaming-night.toml
sudo systemctl enable --now lanlinkd@gaming-night
```

## Running the GUI after install

After installation, start the front-end you built with a single command.
`lanlink-gui` is the unified launcher and is included automatically whenever
you build any front-end.

```bash
# Terminal UI
lanlink-gui --mode tui

# Browser WebUI (auto-opens browser by default)
lanlink-gui --mode web
lanlink-gui --mode web --bind 0.0.0.0:8686

# Native desktop app
lanlink-gui --mode desktop

# Full dev stack: coordinator + daemon + front-end, no manual service setup
lanlink-gui --dev --mode web
lanlink-gui --dev --mode tui
lanlink-gui --dev --mode desktop
```

Or use the Makefile from the source tree:

```bash
make gui
make gui MODE=web
make gui MODE=desktop
```

## Build selector reference

| Selector | Debian / Ubuntu | Arch Linux | What is built |
|---|---|---|---|
| (none) | `make dist-deb` | `makepkg -si` | Core only |
| TUI | `make dist-deb LL_GUI=tui` | `LL_GUI=tui makepkg -si` | `lanlink-tui` + `lanlink-gui` |
| WebUI | `make dist-deb LL_GUI=web` | `LL_GUI=web makepkg -si` | `lanlink-web` + `lanlink-gui` |
| Desktop | `make dist-deb LL_GUI=desktop` | `LL_GUI=desktop makepkg -si` | `lanlink-desktop` + `lanlink-gui` |
| All | `make dist-deb LL_GUI=all` | `LL_GUI=all makepkg -si` | All front-ends + `lanlink-gui` |

### Debian-only script flags

```bash
bash dist/deb/build-deb.sh --with-tui
bash dist/deb/build-deb.sh --with-web
bash dist/deb/build-deb.sh --with-desktop
bash dist/deb/build-deb.sh --with-gui
```

The `LL_GUI` environment variable and the `--with-*` flags produce the same
result for the Debian build.

## Troubleshooting

- `dpkg: dependency problems` → run `sudo apt-get install -f` after `dpkg -i`.
- Desktop build fails on Linux → install `webkit2gtk-4.1` headers
  (`libwebkit2gtk-4.1-dev` on Debian/Ubuntu, `webkit2gtk-4.1` on Arch).
- Service fails to start → check `journalctl -u lanlinkd@gaming-night -e`.
- Permission denied on TAP device → the unit needs `CAP_NET_ADMIN` and
  `CAP_NET_RAW`; the packaged `lanlinkd@.service` already sets these.
