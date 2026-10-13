# LanLink — Cross-Platform Virtual LAN

LanLink is a free, cross-platform, peer-to-peer virtual LAN (layer-2 mesh VPN)
inspired by Radmin VPN. It is built for gaming, remote access, and connecting
distributed teams without port forwarding or centralized accounts.

## What makes LanLink different

- **Layer-2 Ethernet emulation** — every peer gets a virtual network adapter and
  a `26.0.0.0/8` address. ARP, broadcast, multicast, SMB, and LAN game server
  discovery work exactly like on a physical LAN.
- **Name + password networks** — create or join a network the same way you would
  with Radmin VPN.
- **Peer-to-peer UDP** — direct connections when possible, with NAT hole punching
  and automatic relay fallback.
- **End-to-end encryption** — the coordinator helps with membership but cannot
  decrypt traffic.
- **Cross-platform** — Linux (TAP), Windows (Wintun), and macOS (utun) from the
  same codebase.

## Components

| Component | Purpose |
|---|---|
| `lanlinkd` | Root/admin daemon that owns the virtual NIC, switch, crypto, and NAT traversal. |
| `lanlinkctl` | User CLI for create/join/list/status and administrative commands. |
| `lanlink-coordinator` | Self-hostable coordination server: network registry, membership, candidate exchange. |
| `lanlink-relayd` | Optional content-blind UDP relay for when direct P2P is impossible. |

## Quick start

1. Start a coordinator:
   ```bash
   cargo run -p coordinator -- --db lanlink.db
   ```
2. Join (or create) a network from each peer with one command (as root or with
   `CAP_NET_ADMIN`):
   ```bash
   LANLINK_PASSWORD=hunter2 \
     sudo -E cargo run -p lanlinkd -- up --name gaming-night --coordinator http://coord:8080
   ```
   The first peer creates the network; later peers join it. Add `--public` to
   enable the public/gaming inbound firewall.

See the [Installation Guide](./install.md) for one-command installs with your
choice of TUI, WebUI, or desktop front-end. See [Operator Runbooks](./ops.md) for
packaging, systemd, gateway mode, and firewall details.
