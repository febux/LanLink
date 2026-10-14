# LanLink — Cross-Platform Virtual LAN

A free, cross-platform, peer-to-peer virtual LAN (layer-2 mesh VPN) inspired by
Radmin VPN. Built for gaming, remote access, and connecting distributed teams without
port forwarding or centralized accounts.

**Platforms**: Linux · Windows · macOS (x86_64 + ARM64)

**Stack**: Rust workspace (daemon, CLI, coordinator) + self-hostable Go/Rust relay
(architecture decision in [`PLAN.md`](./PLAN.md)).

---

## What this is

LanLink creates a software Ethernet switch spanning the public Internet. Every peer
gets a virtual network adapter with a `26.0.0.0/8` address. Because the tunnel operates
at **layer 2** (Ethernet frames), LAN game server discovery, ARP, multicast, SMB, and
legacy IP protocols work the same way they do on a real LAN.

Key design points:

- **Name + password networks**: create or join a network like Radmin VPN.
- **Peer-to-peer UDP** with NAT hole punching; falls back to an encrypted relay if direct
  connection is impossible.
- **End-to-end encryption**: the coordinator only helps with membership; it cannot
  decrypt traffic.
- **Cross-platform**: same codebase supports Linux TAP, Windows Wintun, and macOS utun.

## Project status

Implemented so far:

- **M0–M2** — encrypted two-node ping, MAC-learning switch, broadcast/flooding, and PMTU
  fragmentation/reassembly.
- **M3** — coordinator REST API for create/join/list networks plus a daemon mode that joins
  via the coordinator and floods frames to peers.
- **M4–M5** — STUN-based NAT traversal, UDP hole punching, and fallback to a custom
  content-blind UDP relay (`lanlink-relayd`).
- **M6** — gateway mode (proxy-ARP, ip_forward, MASQUERADE, peer route sync) and a
  public/gaming-network inbound firewall profile (nftables on Linux).
- **M7** — TOML config files + `password_file` secrets, graceful SIGTERM shutdown with
  cleanup, crash-safe firewall/route re-application, `lanlinkd service` management
  (systemd template unit + Windows SCM), sd_notify watchdog, and packaging: deb / rpm /
  AUR / macOS pkg / Windows service (see [`dist/README.md`](./dist/README.md)).
- **M8** — public directory server (`GET /directory`), opt-in anonymized telemetry
  (`POST /telemetry`, `GET /metrics`, `lanlinkctl coord metrics`), and a static documentation
  site under [`docs/`](./docs).
- **M9** — terminal UI (`lanlink-tui`) plus the daemon local status API it is built on:
  a read-only Unix-socket snapshot endpoint (peers, direct/relay path classification,
  per-peer traffic counters, latency probes), with PING/PONG RTT measurement and a
  keepalive liveness fix in the switch core.
- **M9+** — browser WebUI (`lanlink-web`) that proxies the daemon status socket and
  coordinator REST API into a vanilla-JS SPA. Dashboard, peers, public directory,
  and create/join network flows.
- Linux TAP backend; Windows Wintun and macOS utun are stubbed and ready to be wired.

Next up: broader cross-platform CI and public beta soak testing.

See [`PLAN.md`](./PLAN.md) for the full research analysis, architecture, roadmap, and
cross-platform plan.

## Repository layout

```
lanlink/
├── crates/
│   ├── lanlink-proto/     # shared types and control API messages
│   ├── lanlink-crypto/    # Argon2id key derivation, AEAD, Noise channels
│   ├── lanlink-tap/       # virtual NIC abstraction (TAP/Wintun/utun)
│   ├── lanlink-switch/    # MAC-learning switch core
│   ├── lanlink-punch/     # STUN + UDP hole punching
│   ├── lanlink-relay/     # content-blind UDP relay server library + binary
│   ├── lanlink-firewall/  # per-network inbound firewall profile
│   ├── lanlink-gateway/   # gateway mode: proxy-ARP, NAT, route push
│   ├── lanlinkd/          # daemon binary
│   ├── lanlinkctl/        # CLI binary
│   ├── lanlink-tui/       # terminal UI binary (status, directory, create/join)
│   ├── lanlink-web/       # browser UI binary (status, directory, create/join)
│   ├── lanlink-gui/       # unified GUI launcher (TUI, WebUI, or Desktop with one command)
│   ├── lanlink-desktop/   # native desktop UI using Tauri
│   └── coordinator/       # self-hosted coordination server
├── tests/                 # netns-based integration tests
├── dist/                  # systemd / Windows service / launchd / packaging files
├── docs/                  # public documentation site (Markdown + static HTML builder)
├── PLAN.md                # detailed research analysis and build plan
└── README.md              # this file
```

## Quick start (install)

### Linux — one command

```bash
curl -fsSL https://raw.githubusercontent.com/lanlink/lanlink/main/dist/install.sh | sh
LANLINK_PASSWORD=hunter2 sudo -E lanlinkd service up gaming-night
lanlink-desktop
```

### Windows — one command

Run in an **elevated PowerShell**:

```powershell
irm https://raw.githubusercontent.com/lanlink/lanlink/main/dist/windows/install.ps1 | iex
lanlinkd service up --name gaming-night --password hunter2
```

Then open **LanLink** from the Start Menu.

### macOS

Download the latest `.pkg` from [GitHub Releases](https://github.com/lanlink/lanlink/releases)
and run:

```bash
sudo installer -pkg lanlink-*-macos-*.pkg -target /
```

For full install options see [`docs/install.md`](./docs/install.md).

## Quick start (development)

Requires **Rust 1.79+** and root privileges for TAP device management.

```bash
cd lanlink
cargo build --workspace

# Run tests that do not need root
cargo test --workspace

# One command: build everything, start the coordinator, start the daemon
# as root, and open the TUI. Press 'q' or Ctrl-C in the TUI to stop all.
make run-tui

# Same one-command flow, but opens the browser-based WebUI:
make run-web
# Or with flags:
# make web LL_DEV_WEB_BIND=0.0.0.0:8686
# bash scripts/gui-start.sh --mode web --release --no-browser

# Unified launcher: pick TUI, WebUI, or Desktop UI with a single command.
# Defaults to TUI; set MODE=web for the browser UI, MODE=desktop for Tauri.
make gui
make gui MODE=web
make gui MODE=desktop
# Or directly:
# cargo run -p lanlink-gui
# cargo run -p lanlink-gui -- --mode web --bind 127.0.0.1:8686 --no-browser
# cargo run -p lanlink-gui -- --mode desktop
# cargo run -p lanlink-gui -- --dev --mode web

# The same command as a script (pass --release for an optimized build):
# bash scripts/gui-start.sh --mode tui
# bash scripts/gui-start.sh --mode desktop
#
# Customize the dev network name/password:
# LL_DEV_NETWORK_NAME=my-net LL_DEV_PASSWORD=secret make gui

# Manual three-terminal equivalent (kept for debugging):
# 1. cargo run -p coordinator -- --db lanlink.db
# 2. export LANLINK_PASSWORD=hunter2 && sudo -E cargo run -p lanlinkd -- up --name gaming-night
# 3. cargo run -p lanlink-tui -- --coordinator http://localhost:8080

# Production: install as a systemd service (Linux) in one command.
# Resolves/creates the network, writes a config + secret, installs the unit,
# and starts/enables the instance.
LANLINK_PASSWORD=hunter2 sudo -E lanlinkd service up gaming-night

# Or manage the config file manually and use the existing template unit:
sudo lanlinkd service install
sudo lanlinkd service start gaming-night   # uses /etc/lanlink/gaming-night.toml

# Or skip the coordinator entirely with the static two-node mode:
sudo cargo run -p lanlinkd -- \
  static-peer \
  --tap ll0 \
  --local-ip 26.0.0.1 \
  --peer-addr 127.0.0.1:9001 \
  --password secret \
  --bind 0.0.0.0:9000
```

### GUI launcher

A unified GUI entry point, `lanlink-gui`, can start either the terminal UI or
the browser UI with one command. It connects to an already-running daemon and
coordinator by default; add `--dev` to start the full stack in one shot.

```bash
# Terminal UI (default)
cargo run -p lanlink-gui

# Browser UI
cargo run -p lanlink-gui -- --mode web

# Native desktop UI (Tauri)
cargo run -p lanlink-gui -- --mode desktop

# Full dev stack: coordinator + daemon (as root) + WebUI, browser auto-opened
cargo run -p lanlink-gui -- --dev --mode web

# Full dev stack with TUI
cargo run -p lanlink-gui -- --dev --mode tui

# Full dev stack with desktop UI
cargo run -p lanlink-gui -- --dev --mode desktop
```

### WebUI

A browser-based UI is available as `lanlink-web` (or via `lanlink-gui --mode web`).
It proxies the daemon status socket and coordinator REST API, so it can run as an
unprivileged user:

```bash
# One command: start coordinator + daemon (as root) + WebUI and open the browser.
make run-web

# Or start only the WebUI after the coordinator and daemon are already running:
cargo run -p lanlink-web
# or through the unified launcher:
cargo run -p lanlink-gui -- --mode web --no-browser

# Then open http://127.0.0.1:8686 in a browser.
# The UI polls once per second for daemon status and every 10 seconds for the
# public network directory.
```

### Desktop UI

A native desktop app is available as `lanlink-desktop` (or via `lanlink-gui --mode desktop`).
It is built with Tauri and reuses the WebUI assets, but runs as a standalone
window with a system tray icon:

```bash
# Start the desktop UI, connecting to an already-running daemon and coordinator.
cargo run -p lanlink-gui -- --mode desktop

# Full dev stack in one command (coordinator + daemon as root + desktop app).
cargo run -p lanlink-gui -- --dev --mode desktop

# Or use the shell launcher directly:
bash scripts/gui-start.sh --mode desktop
```

One-command launcher flags (accepted by `scripts/gui-start.sh`):

- `--release` / `-r` — optimized build.
- `--mode tui|web|desktop` — choose the GUI frontend (default: `tui`).
- `--bind ADDR` — override the WebUI bind address (web mode only).
- `--no-browser` — do not auto-open the browser (web mode only).
- Environment variables: `LL_DEV_NETWORK_NAME`, `LL_DEV_PASSWORD`, `LL_DEV_COORD_PORT`, `LL_DEV_STATUS_SOCKET`, `LL_DEV_WEB_BIND`.

WebUI-only options:

- `--bind 127.0.0.1:8686` — HTTP listen address (localhost by default).
- `--coordinator http://localhost:8080` — coordinator base URL.
- `--status-socket /run/lanlink/lanlinkd-1.sock` — override daemon socket discovery.

A two-node Linux netns lab is provided in [`tests/m1_ping_lab.sh`](./tests/m1_ping_lab.sh)
for verifying encrypted ICMP between isolated namespaces.

## Building

```bash
# Check all crates
cargo check --workspace

# Run formatting and clippy
make fmt
make clippy

# Run tests
make test
```

## Roadmap snapshot

| Milestone | Goal |
|---|---|
| M0 | Platform NIC spikes (TAP/Wintun/utun) |
| M1 | Encrypted two-node ping over direct UDP |
| M2 | MAC-learning switch, broadcast, fragmentation |
| M3 | Coordinator: create/join by name+password |
| M4 | NAT traversal: STUN, hole punch, relay fallback |
| M5 | Relay server (coturn-compatible) |
| M6 | Gateway mode + gaming-network firewall |
| M7 | Packaging: Linux/Windows/macOS installers |
| M8 | v1.0 public beta |
| M9 | Terminal UI + daemon status API |
| M9+ | Browser WebUI (`lanlink-web`) + unified GUI launcher (`lanlink-gui`) + native desktop UI (`lanlink-desktop` via Tauri) |

Full details and acceptance criteria are in [`PLAN.md`](./PLAN.md) §9.

## Security model

- Traffic is end-to-end encrypted with ChaCha20-Poly1305 (or AES-256-GCM) and authenticated
  per peer.
- The network password is stretched with Argon2id to produce a network PSK; the coordinator
  never sees the password.
- Pairwise sessions will migrate to the Noise protocol framework for identity-based
  authentication and forward secrecy.
- The daemon runs with elevated privileges only for virtual-NIC management; on Linux it can
  be constrained to `CAP_NET_ADMIN` / `CAP_NET_RAW`.

## Contributing

- Keep changes scoped to one crate per PR where possible.
- Add unit tests for crypto and frame parsers; add netns integration tests for networking
  behavior.
- Follow `cargo fmt` and `cargo clippy -- -D warnings`.

## Legal / naming

This project is a clean-room functional equivalent of Radmin VPN, based on its public
marketing material and well-known virtual-LAN architecture. It does **not** use
Radmin/Famatech trademarks, assets, or protocols, and does not attempt to interoperate
with the official Radmin VPN service.

Working name: **LanLink** (verify trademark availability before a public release).

## License

Apache-2.0 — see LICENSE (to be added).

---

*For implementation details and design rationale, read [`PLAN.md`](./PLAN.md).*
