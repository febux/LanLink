# Coordinator role

Use the install wizard to deploy a self-hosted LanLink coordinator. The
coordinator is the only component that needs a public address: it tells peers
where to find each other and advertises relay fallback addresses. It never
sees decrypted traffic.

For a full VPS deployment with TLS, reverse proxy, backups, and sizing, see
[`deployment.md`](./deployment.md). This page covers only the wizard path.

## What the wizard installs

- `/usr/bin/lanlink-coordinator`
- `/etc/systemd/system/lanlink-coordinator.service`
- `/var/lib/lanlink/` for the coordinator SQLite database
- Optionally `/etc/systemd/system/lanlink-relayd.service` when `--with-relay` is
  passed

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `LANLINK_ROLE` | — | Must be `coordinator` |
| `LANLINK_LISTEN` | `0.0.0.0:8080` | REST API listen address |
| `LANLINK_DATA_DIR` | `/var/lib/lanlink` | SQLite database directory |
| `LANLINK_RELAY_SERVER` | — | Relay address advertised to peers; requires `--with-relay` or manual relay setup |
| `LANLINK_PKG` | auto | `deb`, `rpm`, `aur`, `tarball` |
| `LANLINK_VERSION` | latest release | Exact release version |
| `LANLINK_CHANNEL` | `stable` | Release channel |
| `LANLINK_INSTALL_PREFIX` | `/usr/local` | Install prefix for tarballs |
| `LANLINK_YES=1` | prompt | Non-interactive mode |

`--with-relay` is a CLI-only flag at this time (no `LANLINK_WITH_RELAY`
variable).

## Examples

### Basic coordinator

```bash
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  bash -s -- --role coordinator --yes
```

### Coordinator + relay on the same host

```bash
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  bash -s -- --role coordinator --with-relay --yes
```

### Non-interactive with environment variables

```bash
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  LANLINK_ROLE=coordinator \
  LANLINK_LISTEN=0.0.0.0:8080 \
  LANLINK_DATA_DIR=/var/lib/lanlink \
  bash -s -- --yes
```

### Dry-run preview

```bash
bash scripts/install.sh --dry-run --role coordinator --with-relay
```

### Reconfigure an existing coordinator

```bash
bash scripts/install.sh --role coordinator --reconfigure --yes
```

### Uninstall

```bash
# Stop/disable only the coordinator unit and keep the package
bash scripts/install.sh --role coordinator --uninstall --yes

# Also remove the package
bash scripts/install.sh --role coordinator --uninstall --purge --yes
```

## Verification

```bash
sudo systemctl status lanlink-coordinator --no-pager
sudo journalctl -u lanlink-coordinator -n 30 --no-pager
curl -s http://127.0.0.1:8080/api/networks   # path from docs/api.md
```

If `--with-relay` was used, also check:

```bash
sudo systemctl status lanlink-relayd --no-pager
sudo ss -lnp | grep 3478
```

## Advanced / fallback

For manual coordinator installation, TLS with Caddy, database backups, sizing,
and known limitations, read [`docs/deployment.md`](./deployment.md).

The wizard writes a generated unit to `/etc/systemd/system/lanlink-coordinator.service`.
Edit it directly if you need custom coordinator flags, then run:

```bash
sudo systemctl daemon-reload
sudo systemctl restart lanlink-coordinator
```

## Join command for peers

When the coordinator wizard finishes, it prints a peer join command such as:

```bash
LANLINK_PASSWORD='your-password' lanlinkd service up <network-name> \
  --coordinator http://127.0.0.1:8080
```

Point peers at the public HTTPS URL once you have Caddy or another reverse
proxy in front of the coordinator.
