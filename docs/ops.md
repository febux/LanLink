# Operator Runbooks

## Installation

The recommended way to install LanLink on Linux is the wizard:

```bash
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | bash
```

See [`install.md`](./install.md) for role-based examples, environment
variables, and non-interactive usage. The manual package instructions below are
for fallback or unusual distros.

### Linux (deb/rpm/AUR)

Packages install `lanlinkd`, `lanlinkctl`, and `lanlink-coordinator` into
`/usr/bin/`, plus a systemd template unit at `/lib/systemd/system/lanlinkd@.service`.

```bash
sudo dpkg -i lanlink_0.1.0_amd64.deb
# or
sudo rpm -i lanlink-0.1.0-1.x86_64.rpm
```

### Windows

Run the MSI installer, then manage the service from an elevated prompt:

```powershell
lanlinkd service install
lanlinkd service start gaming-night
```

### macOS

Install the `.pkg`, then use `launchctl` to load the launchd plist, or use
`lanlinkd service install/start`.

## Ad-hoc startup

For testing or one-off networks you can skip the config file entirely:

```bash
LANLINK_PASSWORD='s3cret' sudo -E lanlinkd up --name gaming-night \
  --coordinator https://coord.example.com:8080
```

`lanlinkd up` resolves the network by name, creating it if it does not exist,
then runs the daemon. Add `--public` to create a public/gaming network.

## Configuration

Create one TOML file per network under `/etc/lanlink/`:

```toml
[network]
coordinator = "https://coord.example.com:8080"
network_id = 1
password_file = "/etc/lanlink/gaming-night.secret"
bind = "0.0.0.0:9000"
tap = "lanlink0"
mtu = 1400

[telemetry]
enabled = false
```

The password file must contain the password on its first line and be owned by
root with mode `0600`.

## Service management (Linux)

One-command setup for a named network:

```bash
LANLINK_PASSWORD='s3cret' sudo -E lanlinkd service up gaming-night \
  --coordinator https://coord.example.com:8080
```

This resolves (or creates) the network, writes `/etc/lanlink/gaming-night.toml`
and `/etc/lanlink/gaming-night.secret`, installs the systemd unit template,
and starts/enables `lanlinkd@gaming-night.service`.

Traditional manual management:

```bash
sudo lanlinkd service install
sudo lanlinkd service start gaming-night
sudo lanlinkd service stop gaming-night
sudo lanlinkd service status
```

## Gateway mode

A gateway node shares its physical LAN with the virtual network:

```toml
[gateway]
enabled = true
shared_subnet = "192.168.1.0/24"
phys_interface = "eth0"
```

Gateway mode enables `ip_forward`, sets up proxy-ARP on the TAP interface,
configures nftables MASQUERADE, and pushes routes to peers that advertise
shared subnets.

## Public-network firewall

Networks created with `--public` enable an inbound firewall on the TAP interface
that blocks common high-risk ports (SMB 445/139, RDP 3389, SSH 22, NetBIOS). The
firewall is applied idempotently: a fresh daemon run replaces any previous table,
and shutdown removes it.

## Troubleshooting

- `lanlinkctl status` — show joined networks and peer counts (when implemented).
- `lanlinkctl ping <virtual-ip>` — send a latency probe (when implemented).
- `lanlinkctl coord metrics` — view aggregated telemetry on the coordinator.
- Check daemon logs with `journalctl -u lanlinkd@gaming-night -f`.
- Verify TAP device and routes with `ip addr show lanlink0` and `ip route`.
