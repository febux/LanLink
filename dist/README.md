# Packaging & distribution

Artifacts for installing LanLink on each platform. Everything here is built
from the workspace root with `make` or the per-platform scripts.

| Platform | Mechanism | Entry point |
|---|---|---|
| Linux (Debian/Ubuntu) | `.deb` + systemd | `dist/deb/build-deb.sh` or `make dist-deb` |
| Linux (Fedora/RHEL) | `.rpm` + systemd | `dist/rpm/build-rpm.sh` or `make dist-rpm` |
| Linux (Arch) | AUR | `dist/aur/PKGBUILD` |
| macOS | `.pkg` + launchd | `dist/macos/build-pkg.sh` or `make dist-pkg` |
| Windows | MSI (WiX) + Windows service | `dist/windows/build-msi.sh` or `make dist-msi` |
| Any | systemd units only | `dist/systemd/` |
| All | GitHub Releases | `dist/publish-release.sh` or `make dist-publish` |

## One-command publish to GitHub Releases

```sh
# Authenticate once with the GitHub CLI
gh auth login

# Build every package that can be built on this host and publish them
make dist-publish VERSION=0.2.0

# Dry-run: build without creating the release
make dist-publish VERSION=0.2.0 PUBLISH_OPTS="--dry-run"

# Publish already-built artifacts only
make dist-publish VERSION=0.2.0 PUBLISH_OPTS="--skip-build"

# Publish with GUI front-ends included in the .deb build
LL_GUI=all make dist-publish VERSION=0.2.0
```

The publish script auto-detects what the current host can build:

- Linux with `dpkg-deb` → `.deb`
- Linux with `rpmbuild` or Docker/Podman → `.rpm`
- Linux with `cargo-xwin` + WiX → `.msi`
- macOS with `pkgbuild` → `.pkg`
- Windows with WiX → `.msi`

Missing platform toolchains are skipped with a warning. Run the script on each
platform (or in CI matrix jobs) and re-run `make dist-publish PUBLISH_OPTS="--skip-build"` to
collect and upload every artifact under one release.

## Building the .deb with GUI front-ends

```sh
# Terminal UI
make dist-deb LL_GUI=tui

# Browser WebUI
make dist-deb LL_GUI=web

# Native desktop UI (requires webkit2gtk headers)
make dist-deb LL_GUI=desktop

# All front-ends
make dist-deb LL_GUI=all
```

## What gets installed (Linux)

- `/usr/bin/lanlinkd`, `/usr/bin/lanlinkctl`, `/usr/bin/lanlink-relayd`,
  `/usr/bin/lanlink-coordinator`
- systemd units:
  - `lanlinkd@.service` — daemon template, one instance per network config
    in `/etc/lanlink/<name>.toml`
  - `lanlink-coordinator.service` — membership server (self-host)
  - `lanlink-relayd.service` — UDP relay fallback server (self-host)
- `/etc/lanlink/lanlinkd.example.toml` — annotated config template
- system user `lanlink` (no login shell)

See [`docs/install.md`](../docs/install.md) for a complete step-by-step guide that
includes one-command installs with TUI, WebUI, or desktop front-ends.

## Quick start after installing the deb/rpm

For a quick ad-hoc network (no config file required):

```sh
# Start the coordinator first (or use an existing one)
sudo systemctl enable --now lanlink-coordinator

# Join or create a network by name and run it as a persistent service
LANLINK_PASSWORD='s3cret' sudo -E lanlinkd service up gaming-night
```

For persistent production use with a manually-managed config file:

```sh
# 1. create a network (or use an existing one)
lanlinkctl coord create --name gaming-night --password 's3cret'
# Network 42 created

# 2. write a config for it
sudo install -d -m 0750 /etc/lanlink
sudo cp /etc/lanlink/lanlinkd.example.toml /etc/lanlink/gaming-night.toml
sudoedit /etc/lanlink/gaming-night.toml    # set network_id, password_file
echo 's3cret' | sudo tee /etc/lanlink/network.secret >/dev/null
sudo chmod 0600 /etc/lanlink/network.secret

# 3. install the unit template once, then start the instance
sudo lanlinkd service install
sudo lanlinkd service start gaming-night

# 4. autostart at boot
sudo lanlinkd service enable gaming-night
```

## Privilege separation model

The daemon needs exactly two Linux capabilities: `CAP_NET_ADMIN` (TAP
device, routes, nftables) and `CAP_NET_RAW` (UDP sockets for STUN/punching).
The shipped unit runs as the unprivileged `lanlink` user with
`AmbientCapabilities=CAP_NET_ADMIN CAP_NET_RAW` and a strict systemd sandbox
(`ProtectSystem=strict`, `ProtectHome`, `SystemCallFilter=@system-service`,
`MemoryDenyWriteExecute`, …). Gateway mode additionally writes
`/proc/sys/net/ipv4/ip_forward`, so `ProtectKernelTunables` is kept off —
that write is still gated by `CAP_NET_ADMIN`.

Secrets (`network.secret`) stay root-owned 0600; the packaged
`/etc/lanlink` is group-readable by `lanlink` via postinst if you prefer the
daemon to read the secret directly — adjust ownership to match your policy.

## Self-hosting the control plane

```sh
sudo systemctl enable --now lanlink-coordinator   # REST API on :8080
sudo systemctl enable --now lanlink-relayd        # UDP relay on :3478
```

Point clients at the coordinator via `coordinator = "https://your-host:8080"`
in their network configs, and configure the coordinator's
`--relay-server` flag to advertise the relay's public address.
