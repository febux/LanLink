# Installation Guide

The fastest way to install LanLink on Linux is the wizard (`scripts/install.sh`,
published as `lanlink-install.sh` in GitHub Releases). It detects your
package manager, downloads the matching artifact, verifies checksums, and
configures the requested role in one command.

For maintainers, see [`dist/publish-release.sh`](../dist/publish-release.sh) or
`make dist-publish` to build and publish all distribution packages to GitHub
Releases in one command.

## What the wizard installs

Regardless of role, the wizard installs the core LanLink binaries and systemd
units:

- `/usr/bin/lanlinkd` — virtual-LAN daemon
- `/usr/bin/lanlinkctl` — command-line control tool
- `/usr/bin/lanlink-coordinator` — self-hosted membership/signaling server
- `/usr/bin/lanlink-relayd` — content-blind UDP relay fallback
- `/etc/lanlink/` — per-network TOML configs and password secrets
- `/var/lib/lanlink/` — runtime data (coordinator SQLite DB, identity keys)
- systemd units:
  - `lanlinkd@.service` — one daemon instance per `/etc/lanlink/<name>.toml`
  - `lanlink-coordinator.service` — coordinator role
  - `lanlink-relayd.service` — relay role

Optional GUI front-ends can be included for the peer role:

| Front-end | Binary | Description |
|---|---|---|
| Native desktop | `lanlink-desktop` | Tauri-based window with system tray |
| Browser WebUI | `lanlink-web` | Serves the UI on `http://127.0.0.1:8686` |
| Terminal UI | `lanlink-tui` | Runs in any terminal |
| Unified launcher | `lanlink-gui` | One binary that can start TUI/Web/Desktop |

## Recommended one-line install

The wizard is safe to pipe from GitHub Releases (`set -euo pipefail`, no `eval`
of remote content, optional `SHA256SUMS` verification). Replace `v0.6.1`
with the latest release if you prefer a pinned version.

```bash
# Quick default (downloads, installs core, prompts for role interactively)
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | bash
```

For a fully non-interactive install, set the role and role-specific options via
environment variables and add `--yes`:

```bash
# Join a network as a peer, non-interactive
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  LANLINK_ROLE=peer \
  LANLINK_COORD_URL=https://coord.example.com \
  LANLINK_NETWORK=gaming-night \
  LANLINK_PASSWORD='s3cret' \
  bash -s -- --yes
```

> Environment variables win over CLI flags, so the same command works both
> interactively and in CI playbooks.

## Wizard options and environment variables

Every CLI flag below has an equivalent environment variable. When both are set,
the environment variable takes precedence.

| CLI flag | Environment variable | Default | Description |
|---|---|---|---|
| `--role ROLE` | `LANLINK_ROLE` | — | `coordinator`, `peer`, or `relay` |
| `--coord-url URL` | `LANLINK_COORD_URL` | — | Peer/coordinator base URL |
| `--listen ADDR` | `LANLINK_LISTEN` | role-specific | Coordinator: `0.0.0.0:8080`; relay: `0.0.0.0:3478` |
| `--relay-server ADDR` | `LANLINK_RELAY_SERVER` | — | Relay address advertised by the coordinator |
| `--network NAME` | `LANLINK_NETWORK` | — | Peer network name |
| `--password PASS` | `LANLINK_PASSWORD` | — | Peer network password |
| `--password-file FILE` | `LANLINK_PASSWORD_FILE` | — | File containing the peer password |
| `--data-dir PATH` | `LANLINK_DATA_DIR` | `/var/lib/lanlink` | Coordinator SQLite directory |
| `--gui SELECTOR` | `LANLINK_GUI` | `none` | `none`, `tui`, `web`, `desktop`, `all` |
| `--pkg TYPE` | `LANLINK_PKG` | auto | `deb`, `rpm`, `aur`, `tarball` |
| `--version VERSION` | `LANLINK_VERSION` | latest release | Exact release version |
| `--channel CHANNEL` | `LANLINK_CHANNEL` | `stable` | Release channel |
| `--prefix PATH` | `LANLINK_INSTALL_PREFIX` | `/usr/local` | Install prefix for tarballs |
| `--yes` | `LANLINK_YES=1` | prompt | Non-interactive mode |
| `--dry-run` | — | — | Print actions without changing the system |
| `--uninstall` | — | — | Remove role-specific config and units |
| `--reconfigure` | — | — | Overwrite existing config files |
| `--with-relay` | — | — | Also install/start `lanlink-relayd` for the coordinator role |
| `--purge` | — | — | Uninstall the package as well as role files |
| — | `LANLINK_REPO` | `febux/LanLink` | Override the GitHub repository used for downloads |

Run the local copy for a dry-run or when testing from the repo:

```bash
bash scripts/install.sh --help
bash scripts/install.sh --dry-run --role coordinator
```

## Role-based install

### Peer (join a network)

The peer role installs the package, writes `/etc/lanlink/<network>.toml` and
`/etc/lanlink/<network>.secret`, then runs `lanlinkd service up <network>` to
install and start `lanlinkd@<network>.service`.

```bash
# Interactive
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | bash -s -- --role peer

# Non-interactive
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  bash -s -- --role peer \
    --coord-url https://coord.example.com \
    --network gaming-night \
    --password 's3cret' \
    --yes

# Use a password file instead of the command line
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  LANLINK_PASSWORD_FILE=/etc/lanlink/gaming-night.secret \
  bash -s -- --role peer --coord-url https://coord.example.com --network gaming-night --yes

# Include the desktop GUI in the install
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  bash -s -- --role peer --gui desktop --coord-url https://coord.example.com \
    --network gaming-night --password 's3cret' --yes
```

After the wizard finishes:

```bash
sudo systemctl status lanlinkd@gaming-night --no-pager
ip addr show lanlink0            # virtual NIC with 26.x.x.x address
```

See [`docs/peer.md`](./peer.md) for the full peer wizard reference.

### Coordinator (self-host)

The coordinator role installs the package, creates `/var/lib/lanlink/`, writes
`/etc/systemd/system/lanlink-coordinator.service`, and starts the unit.

```bash
# Public coordinator with default listen 0.0.0.0:8080
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  bash -s -- --role coordinator --yes

# Also deploy lanlink-relayd on the same host
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  bash -s -- --role coordinator --with-relay --yes

# Custom listen address and data directory
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  bash -s -- --role coordinator --listen 0.0.0.0:18080 \
    --data-dir /srv/lanlink --yes
```

The wizard prints the peer join command at the end:

```bash
LANLINK_PASSWORD='your-password' lanlinkd service up <network-name> --coordinator http://127.0.0.1:8080
```

> For a public VPS you still need a reverse proxy such as Caddy for TLS; the
> wizard only sets up the LanLink side. See [`docs/deployment.md`](./deployment.md)
> for the full VPS guide.

See [`docs/coordinator.md`](./coordinator.md) for the full coordinator wizard reference.

### Relay (self-host)

The relay role installs the package, writes
`/etc/systemd/system/lanlink-relayd.service`, and starts the unit on UDP
`0.0.0.0:3478` by default.

```bash
# Default relay
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  bash -s -- --role relay --yes

# Custom listen address
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  bash -s -- --role relay --listen 0.0.0.0:3478 --yes
```

Then point the coordinator at the relay's public address:

```bash
# passed to the coordinator wizard or edited in the unit
--relay-server 203.0.113.42:3478
```

See [`docs/relay.md`](./relay.md) for the full relay wizard reference.

## Common modes

### `--dry-run`

Prints every download, file write, and systemctl command without executing any
of them. Use it to preview what the wizard will do.

```bash
bash scripts/install.sh --dry-run --role peer \
  --coord-url https://coord.example.com --network gaming-night
```

### `--yes`

Skips all prompts and accepts defaults. Required for fully non-interactive
provisioning. Any value that the wizard cannot default must be supplied via env
or CLI.

```bash
curl -fsSL .../lanlink-install.sh | \
  LANLINK_ROLE=peer \
  LANLINK_COORD_URL=https://coord.example.com \
  LANLINK_NETWORK=gaming-night \
  LANLINK_PASSWORD_FILE=/etc/lanlink/gaming-night.secret \
  bash -s -- --yes
```

### `--reconfigure`

By default the wizard preserves an existing `/etc/lanlink/<network>.toml` and
`/etc/systemd/system/lanlink-coordinator.service`. Pass `--reconfigure` to
overwrite them.

```bash
bash scripts/install.sh --role peer --network gaming-night --reconfigure
```

### `--uninstall` and `--purge`

Stop and disable role-specific units, remove role-specific configs/secrets, and
optionally remove the package.

```bash
# Remove only the peer role files
bash scripts/install.sh --role peer --network gaming-night --uninstall --yes

# Also remove the .deb/.rpm package
bash scripts/install.sh --role peer --network gaming-night --uninstall --purge --yes

# Remove the coordinator role
bash scripts/install.sh --role coordinator --uninstall --yes

# Remove the relay role
bash scripts/install.sh --role relay --uninstall --yes
```

## Upgrades

Run the wizard again with the same role. It installs the newer package only if
the requested version is newer than the installed one, restarts any units that
were already running, and preserves operator-edited configs unless you pass
`--reconfigure`.

```bash
# Upgrade a peer in place
bash scripts/install.sh --role peer --network gaming-night --yes

# Upgrade and force config rewrite
bash scripts/install.sh --role peer --network gaming-night --reconfigure --yes
```

## GUI and package selectors

Peer installs can include a front-end with `--gui` (or `LANLINK_GUI`):

```bash
bash scripts/install.sh --role peer --gui tui   # terminal UI
bash scripts/install.sh --role peer --gui web   # browser UI
bash scripts/install.sh --role peer --gui desktop # Tauri desktop app
bash scripts/install.sh --role peer --gui all   # all front-ends
```

Override package auto-detection with `--pkg` (or `LANLINK_PKG`):

```bash
bash scripts/install.sh --role peer --pkg deb
bash scripts/install.sh --role peer --pkg rpm
bash scripts/install.sh --role peer --pkg aur
bash scripts/install.sh --role peer --pkg tarball
```

## Fallback: manual install

If the wizard cannot run on your system, install the same artifacts manually.
All release artifacts are published to
[GitHub Releases](https://github.com/febux/LanLink/releases):

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

### Debian / Ubuntu (.deb)

```bash
sudo dpkg -i lanlink_*_amd64.deb
sudo apt-get install -f
```

Or build from source:

```bash
make build-deb LL_GUI=all
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

### Windows

Run in an **elevated PowerShell**:

```powershell
irm https://raw.githubusercontent.com/febux/LanLink/master/dist/windows/install.ps1 | iex
lanlinkd service up --name gaming-night --password "s3cret"
```

### macOS

Download the latest `.pkg` from GitHub Releases and run:

```bash
sudo installer -pkg lanlink-*-macos-*.pkg -target /
```

## Service management after install

The wizard starts and enables the relevant systemd units. You can manage them
with `lanlinkd service` or `systemctl` directly:

```bash
# Peer instance for network "gaming-night"
sudo systemctl status lanlinkd@gaming-night --no-pager
sudo systemctl stop lanlinkd@gaming-night
sudo systemctl start lanlinkd@gaming-night

# Coordinator and relay
sudo systemctl status lanlink-coordinator --no-pager
sudo systemctl status lanlink-relayd --no-pager
```

For a manual ad-hoc join without writing a config file:

```bash
LANLINK_PASSWORD='s3cret' sudo -E lanlinkd up --name gaming-night \
  --coordinator https://coord.example.com
```

For persistent production use with a manually-managed config file, see
[`docs/ops.md`](./ops.md).

## Build selector reference

| Selector | Debian / Ubuntu | Arch Linux | What is built |
|---|---|---|---|
| (none) | `make build-deb` | `makepkg -si` | Core only |
| TUI | `make build-deb LL_GUI=tui` | `LL_GUI=tui makepkg -si` | `lanlink-tui` + `lanlink-gui` |
| WebUI | `make build-deb LL_GUI=web` | `LL_GUI=web makepkg -si` | `lanlink-web` + `lanlink-gui` |
| Desktop | `make build-deb LL_GUI=desktop` | `LL_GUI=desktop makepkg -si` | `lanlink-desktop` + `lanlink-gui` |
| All | `make build-deb LL_GUI=all` | `LL_GUI=all makepkg -si` | All front-ends + `lanlink-gui` |

## Troubleshooting

- `bash scripts/install.sh --help` — verify the documented options match the
  running version of the wizard.
- `bash scripts/install.sh --dry-run ...` — preview every command before it runs.
- `dpkg: dependency problems` → run `sudo apt-get install -f` after `dpkg -i`.
- Desktop build fails on Linux → install `webkit2gtk-4.1` headers
  (`libwebkit2gtk-4.1-dev` on Debian/Ubuntu, `webkit2gtk-4.1` on Arch).
- Service fails to start → check `journalctl -u lanlinkd@gaming-night -e` on Linux
  or `sc query LanLinkDaemon` on Windows.
- Permission denied on TAP device → the unit needs `CAP_NET_ADMIN` and
  `CAP_NET_RAW`; the packaged `lanlinkd@.service` already sets these.
- Windows: the Wintun driver is bundled as `wintun.dll`; the virtual adapter
  appears the first time the daemon creates it. Wintun is a layer-3 interface,
  so the Windows data plane carries IP traffic only.
