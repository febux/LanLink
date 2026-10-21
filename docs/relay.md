# Relay role

Use the install wizard to deploy a self-hosted `lanlink-relayd` instance. The
relay is a content-blind UDP (and TCP) fallback used when direct peer-to-peer
connections cannot be established. Traffic is end-to-end encrypted by peers.

## What the wizard installs

- `/usr/bin/lanlink-relayd`
- `/etc/systemd/system/lanlink-relayd.service`
- `lanlink-relayd.service` — started and enabled
- A firewall rule for the relay UDP port (if `firewalld`, `ufw`, or `iptables`
  is available)

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `LANLINK_ROLE` | — | Must be `relay` |
| `LANLINK_LISTEN` | `0.0.0.0:3478` | UDP relay listen address |
| `LANLINK_PKG` | auto | `deb`, `rpm`, `aur`, `tarball` |
| `LANLINK_VERSION` | latest release | Exact release version |
| `LANLINK_CHANNEL` | `stable` | Release channel |
| `LANLINK_INSTALL_PREFIX` | `/usr/local` | Install prefix for tarballs |
| `LANLINK_YES=1` | prompt | Non-interactive mode |

## Examples

### Default relay

```bash
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  bash -s -- --role relay --yes
```

### Custom listen address

```bash
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  bash -s -- --role relay --listen 0.0.0.0:3478 --yes
```

### Non-interactive with environment variables

```bash
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  LANLINK_ROLE=relay \
  LANLINK_LISTEN=0.0.0.0:3478 \
  bash -s -- --yes
```

### Dry-run preview

```bash
bash scripts/install.sh --dry-run --role relay
```

### Reconfigure

```bash
bash scripts/install.sh --role relay --reconfigure --yes
```

### Uninstall

```bash
bash scripts/install.sh --role relay --uninstall --yes
```

## Pointing the coordinator at the relay

The relay's public address must be advertised by the coordinator so peers know
to fall back to it. When you run the coordinator wizard with `--with-relay` on
the same host, this is done automatically. Otherwise pass the relay address to
the coordinator install or edit the coordinator unit:

```bash
--relay-server 203.0.113.42:3478
```

## Verification

```bash
sudo systemctl status lanlink-relayd --no-pager
sudo ss -lnp | grep 3478   # UDP listening socket
```

From an external host, check that the relay port is reachable:

```bash
nc -u coord.example.com 3478    # should not return ICMP unreachable
nc -vz coord.example.com 3479   # TCP fallback, if enabled in the unit
```

## Advanced / fallback

For a full VPS deployment that combines the coordinator and relay with Caddy
TLS, firewall rules, and systemd hardening, see [`docs/deployment.md`](./deployment.md).

The wizard writes a generated unit to `/etc/systemd/system/lanlink-relayd.service`.
Edit it directly for additional flags, then run:

```bash
sudo systemctl daemon-reload
sudo systemctl restart lanlink-relayd
```
