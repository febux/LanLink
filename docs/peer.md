# Peer role

Use the install wizard to join a LanLink network from a Linux host. The peer
role installs the daemon, writes a network config and password secret, and
starts a systemd service instance.

## What the wizard installs

- `/usr/bin/lanlinkd`, `/usr/bin/lanlinkctl`
- `/etc/lanlink/<network>.toml` — daemon config
- `/etc/lanlink/<network>.secret` — password secret, mode `0600`
- `/etc/systemd/system/lanlinkd@.service` (from the package)
- `lanlinkd@<network>.service` — started and enabled
- Optional GUI binaries (`lanlink-tui`, `lanlink-web`, `lanlink-desktop`,
  `lanlink-gui`) when `--gui` is set

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `LANLINK_ROLE` | — | Must be `peer` |
| `LANLINK_COORD_URL` | — | Coordinator base URL, e.g. `http://coord.example.com:8080` |
| `LANLINK_NETWORK` | — | Network name to join or create |
| `LANLINK_PASSWORD` | — | Network password |
| `LANLINK_PASSWORD_FILE` | — | Path to a file containing the password |
| `LANLINK_GUI` | `none` | `none`, `tui`, `web`, `desktop`, `all` |
| `LANLINK_PKG` | auto | `deb`, `rpm`, `aur`, `tarball` |
| `LANLINK_VERSION` | latest release | Exact release version |
| `LANLINK_CHANNEL` | `stable` | Release channel |
| `LANLINK_INSTALL_PREFIX` | `/usr/local` | Install prefix for tarballs |
| `LANLINK_YES=1` | prompt | Non-interactive mode |

Either `LANLINK_PASSWORD` or `LANLINK_PASSWORD_FILE` must be supplied when
`--yes` is used.

## Examples

### Interactive join

```bash
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  bash -s -- --role peer
```

### Non-interactive join

```bash
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  bash -s -- --role peer \
    --coord-url https://coord.example.com \
    --network gaming-night \
    --password 's3cret' \
    --yes
```

### Use a password file

```bash
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  LANLINK_PASSWORD_FILE=/etc/lanlink/gaming-night.secret \
  bash -s -- --role peer \
    --coord-url https://coord.example.com \
    --network gaming-night \
    --yes
```

### Include a GUI

```bash
curl -fsSL https://github.com/febux/LanLink/releases/download/v0.6.1/lanlink-install.sh | \
  bash -s -- --role peer --gui desktop \
    --coord-url https://coord.example.com \
    --network gaming-night \
    --password 's3cret' \
    --yes
```

### Dry-run preview

```bash
bash scripts/install.sh --dry-run --role peer \
  --coord-url https://coord.example.com --network gaming-night
```

### Reconfigure

```bash
bash scripts/install.sh --role peer --network gaming-night --reconfigure --yes
```

### Uninstall

```bash
bash scripts/install.sh --role peer --network gaming-night --uninstall --yes
```

## Verification

```bash
sudo systemctl status lanlinkd@gaming-night --no-pager
sudo journalctl -u lanlinkd@gaming-night -n 30 --no-pager
ip addr show lanlink0            # virtual NIC with a 26.x.x.x address
```

A successful join logs `joined network via coordinator ... assigned_ip=26.x.y.z`
followed by `created Linux TAP device name=lanlink0`.

## Management commands

```bash
lanlinkd service status
lanlinkd service stop gaming-night
lanlinkd service start gaming-night
```

## Advanced / fallback

For manual peer configuration, ad-hoc daemon startup, gateway mode, and
firewall details, see [`docs/ops.md`](./ops.md).

The wizard preserves an existing `/etc/lanlink/<network>.toml` unless you pass
`--reconfigure`. Edit the config manually and restart the service to change
daemon-level options such as `bind`, `tap`, `mtu`, or STUN servers:

```bash
sudoedit /etc/lanlink/gaming-night.toml
sudo systemctl restart lanlinkd@gaming-night
```
