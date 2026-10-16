# Coordinator Deployment Guide

This guide describes how to deploy the **LanLink coordinator** on a VPS with a
stable domain name. The coordinator is the only piece of the architecture that
*must* be hosted on a public address: it tells peers where to find each other and
advertises relay fallback addresses. Relay traffic itself is peer-to-end
encrypted; the coordinator cannot decrypt it.

This guide uses **Caddy** for automatic HTTPS because the coordinator has no
native TLS support. The same machine can also run `lanlink-relayd` so the
advertised relay fallback addresses actually exist. Client-side setup
(systemd requirements, port conflicts, frontend flags) is covered in §7, and
current deployment-relevant code limitations are listed in §12.

Replace `coord.example.com` with your domain, and `203.0.113.42` with your
VPS public IP, throughout.

---

## Prerequisites

- A VPS with a static public IP.
- A domain name and an **A record** pointing `coord.example.com` to the VPS IP
  **before** you start Caddy (ACME/Let's Encrypt validates the domain).
- Rust toolchain on the VPS, or a local build environment to cross-compile and
  copy the binary.
- Root access on the VPS.

### Install Rust on the VPS (if building there)

```bash
apt update && apt install -y curl build-essential pkg-config libssl-dev sqlite3 git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustc --version
```

Log out and back in so `cargo` is available in the shell, or use `source
$HOME/.cargo/env` before each cargo command.

---

## 1. Firewall

Open only what the public needs. The coordinator listens on loopback; Caddy is
its only public HTTP/HTTPS entry point.

```bash
ufw allow 443/tcp   # coordinator HTTPS via Caddy
ufw allow 80/tcp    # Caddy ACME HTTP-01 challenge + redirect
ufw allow 3478/udp  # UDP relay fallback (used by lanlink-relayd)
ufw allow 3479/tcp  # TCP relay fallback (used by lanlink-relayd)
ufw enable
```

---

## 2. Build and install the coordinator

Build directly on the VPS:

```bash
apt update && apt install -y curl build-essential pkg-config libssl-dev sqlite3 git

# Install rustup if it is not already present
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# in the LanLink repo root
cargo build --release -p coordinator
install -Dm755 target/release/lanlink-coordinator /usr/local/bin/lanlink-coordinator
```

Or build locally and copy the binary:

```bash
# local machine (assumes rustup/cargo is already installed)
cargo build --release -p coordinator
scp target/release/lanlink-coordinator root@coord.example.com:/usr/local/bin/lanlink-coordinator
```

If you also deploy the relay from the same machine, copy it too:

```bash
cargo build --release -p lanlink-relay
scp target/release/lanlink-relayd root@coord.example.com:/usr/local/bin/lanlink-relayd
```

---

## 3. Create service user and directories

```bash
useradd --system --home /var/lib/lanlink --shell /usr/sbin/nologin lanlink
install -d -o lanlink -g lanlink /var/lib/lanlink /var/lib/lanlink/backups
install -Dm644 /dev/null /etc/lanlink/server.env
echo 'RUST_LOG=info' > /etc/lanlink/server.env
```

- `/var/lib/lanlink/lanlink.db` — coordinator SQLite database
- `/var/lib/lanlink/backups/` — daily DB backups
- `/etc/lanlink/server.env` — shared environment (log level, future secrets)

---

## 4. systemd unit

`/etc/systemd/system/lanlink-coordinator.service`:

```ini
[Unit]
Description=LanLink coordinator (membership/signaling)
After=network-online.target
Wants=network-online.target

[Service]
User=lanlink
Group=lanlink
EnvironmentFile=/etc/lanlink/server.env
ExecStart=/usr/local/bin/lanlink-coordinator \
  --listen 127.0.0.1:8080 \
  --db /var/lib/lanlink/lanlink.db \
  --relay-server 203.0.113.42:3478 \
  --tcp-relay-server 203.0.113.42:3479
Restart=on-failure
RestartSec=3

# hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/lanlink
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX

[Install]
WantedBy=multi-user.target
```

Important notes:

- `--relay-server` and `--tcp-relay-server` must be the **public**
  addresses advertised to joining peers. If you omit them, the coordinator
  defaults to `127.0.0.1`, which is wrong for a public deployment.
- These flags accept **IP addresses only, not hostnames** (`SocketAddr`
  parsing). Replace `203.0.113.42` with your VPS's public IP (find it with
  `curl -s ifconfig.me`). A hostname like `coord.example.com:3478` fails with
  `invalid socket address syntax`. Clients still connect to the relay by IP,
  which is fine; the domain is only needed for the coordinator HTTPS endpoint.
- The `--listen` port stays on loopback (`127.0.0.1:8080`); Caddy is the only
  public HTTP entry point.

Enable the service:

```bash
systemctl daemon-reload
systemctl enable --now lanlink-coordinator
```

---

## 5. Caddy (automatic HTTPS)

Install Caddy and proxy coordinator traffic.

```bash
apt install -y caddy
```

`/etc/caddy/Caddyfile`:

```
coord.example.com {
    reverse_proxy 127.0.0.1:8080
}
```

Reload:

```bash
systemctl reload caddy
```

Caddy obtains and renews the certificate automatically and redirects HTTP to
HTTPS.

---

## 6. Deploy lanlink-relayd on the same host

If you advertised relay addresses above, run the relay server too. It is a
single process that handles both UDP and TCP fallback.

```bash
# Rust toolchain is already installed from section 2
source "$HOME/.cargo/env"
cargo build --release -p lanlink-relay
install -Dm755 target/release/lanlink-relayd /usr/local/bin/lanlink-relayd
```

`/etc/systemd/system/lanlink-relayd.service`:

```ini
[Unit]
Description=LanLink content-blind relay (UDP + TCP fallback)
After=network-online.target
Wants=network-online.target

[Service]
User=lanlink
Group=lanlink
EnvironmentFile=/etc/lanlink/server.env
ExecStart=/usr/local/bin/lanlink-relayd \
  --listen 0.0.0.0:3478 \
  --tcp-listen 0.0.0.0:3479
Restart=on-failure
RestartSec=3
AmbientCapabilities=CAP_NET_BIND_SERVICE
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
RestrictAddressFamilies=AF_INET AF_INET6

[Install]
WantedBy=multi-user.target
```

Start it:

```bash
systemctl daemon-reload
systemctl enable --now lanlink-relayd
```

Relay traffic is content-blind and end-to-end encrypted by peers.

---

## 7. Client bootstrap

Each peer needs the daemon (`lanlinkd`) and, optionally, a frontend. The only
server-side value clients need is the coordinator HTTPS URL — relay addresses
are returned in the coordinator's join response.

### 7.1 Start the daemon as a systemd service

```bash
LANLINK_PASSWORD='...' sudo -E lanlinkd service up gaming-night \
  --coordinator https://coord.example.com
```

This resolves (or creates) the network, writes `/etc/lanlink/gaming-night.toml`
and the password secret, installs the `lanlinkd@.service` template, and starts
`lanlinkd@gaming-night.service`.

### 7.2 systemd unit requirements (client)

The daemon stores a persistent Ed25519 identity key in `/var/lib/lanlink/`.
The service unit must allow writing there, and the directory must be owned by
the `lanlink` service user:

```bash
sudo mkdir -p /var/lib/lanlink
sudo chown -R lanlink:lanlink /var/lib/lanlink
sudo chmod 750 /var/lib/lanlink
```

The unit template (`dist/systemd/lanlinkd@.service` in the repo) must contain:

```ini
RuntimeDirectory=lanlink
RuntimeDirectoryMode=0755
ReadWritePaths=/var/lib/lanlink
```

Without `ReadWritePaths=/var/lib/lanlink`, the daemon fails under
`ProtectSystem=strict` with `Read-only file system (os error 30)`. If you
installed from an older package, update the template at
`/etc/systemd/system/lanlinkd@.service`, then
`sudo systemctl daemon-reload && sudo systemctl restart lanlinkd@gaming-night`.

### 7.3 Port conflicts on the data socket

The generated config binds UDP `0.0.0.0:9000` by default. If another service
already uses that port (Docker's `docker-proxy` commonly does), the daemon
fails with `Address already in use (os error 98)`. Check with:

```bash
sudo ss -lnp | grep 9000
```

If it is taken, edit `/etc/lanlink/<name>.toml` and change the `bind` port:

```toml
[network]
bind = "0.0.0.0:19000"
```

Then `sudo systemctl restart lanlinkd@<name>`. Any high UDP port works — peers
discover each other via the coordinator and STUN, so the exact port does not
need to be coordinated between machines.

### 7.4 Frontends need the coordinator URL explicitly

Desktop/TUI/Web/ctl frontends do **not** auto-discover the daemon's configured
coordinator and default to `http://localhost:8080`. Join/create actions from a
frontend will fail with `error sending request for url
(http://localhost:8080/...)` unless you pass the flag:

```bash
lanlink-desktop --coordinator https://coord.example.com
lanlink-tui     --coordinator https://coord.example.com
lanlink-web     --coordinator https://coord.example.com
lanlinkctl      --coordinator https://coord.example.com status
```

### 7.5 Verify the client

```bash
sudo systemctl status lanlinkd@gaming-night --no-pager
sudo journalctl -u lanlinkd@gaming-night -n 30 --no-pager
ip addr show lanlink0     # virtual NIC with a 26.x.x.x address
```

A successful join logs `joined network via coordinator ... assigned_ip=26.x.y.z`
followed by `created Linux TAP device name=lanlink0`.

---

## 8. Backup the coordinator database

The SQLite database is small but hosts network membership and telemetry. Use
`sqlite3 .backup` (safe on a live database) rather than a raw file copy.

`/usr/local/bin/lanlink-backup.sh`:

```bash
#!/bin/sh
set -eu
ts=$(date -u +%Y%m%dT%H%M%SZ)
sqlite3 /var/lib/lanlink/lanlink.db ".backup '/var/lib/lanlink/backups/lanlink-${ts}.db'"
gzip -f "/var/lib/lanlink/backups/lanlink-${ts}.db"
find /var/lib/lanlink/backups -name 'lanlink-*.db.gz' -mtime +14 -delete
```

```bash
chmod +x /usr/local/bin/lanlink-backup.sh
```

`/etc/systemd/system/lanlink-backup.service`:

```ini
[Unit]
Description=Backup LanLink coordinator database

[Service]
Type=oneshot
ExecStart=/usr/local/bin/lanlink-backup.sh
```

`/etc/systemd/system/lanlink-backup.timer`:

```ini
[Unit]
Description=Daily LanLink database backup

[Timer]
OnCalendar=*-*-* 04:30:00
Persistent=true

[Install]
WantedBy=timers.target
```

Enable:

```bash
systemctl daemon-reload
systemctl enable --now lanlink-backup.timer
```

Optional: sync `/var/lib/lanlink/backups/` to off-site storage with rclone,
rsync, or your provider's object store.

---

## 9. Sizing

| Component | Role | Minimum VPS | Notes |
|---|---|---|---|
| Coordinator | Signaling only | 1 vCPU, 512 MB RAM | Trivial CPU/RAM; DB is tiny |
| Relay | Traffic only when hole punching fails | 1 vCPU, 1 GB RAM, 1–2 TB transfer | Assumes small groups; most traffic is direct peer-to-peer |

A 1 TB/month egress cap supports roughly 3 Mbit/s sustained. For the project's
stated use case (small groups, gaming + telemetry + remote control), a 1 vCPU /
1 GB / 1–2 TB instance is comfortable. Add additional relay hosts later if
needed; the coordinator's `--relay-server` flag accepts a comma-separated list,
so no client changes are required.

---

## 10. Verification

1. Check services are running:

   ```bash
   systemctl status lanlink-coordinator lanlink-relayd caddy --no-pager
   ```

2. Read coordinator logs:

   ```bash
   journalctl -u lanlink-coordinator -n 50 --no-pager
   ```

3. Verify HTTPS works against a coordinator endpoint:

   ```bash
   curl -s https://coord.example.com/api/networks   # path from docs/api.md
   ```

4. From an external host, check relay ports:

   ```bash
   nc -u coord.example.com 3478   # should not return ICMP unreachable
   nc -vz coord.example.com 3479  # TCP should accept connection
   ```

5. Join two peers from different NATs and confirm virtual ping works:

   ```bash
   LANLINK_PASSWORD='...' sudo -E lanlinkd up --name test \
     --coordinator https://coord.example.com
   # then from peer A: ping <peer B virtual IP>
   ```

6. Confirm the backup script works:

   ```bash
   /usr/local/bin/lanlink-backup.sh
   ls -l /var/lib/lanlink/backups/
   sqlite3 /var/lib/lanlink/backups/lanlink-*.db.gz ".tables"   # after zcat/gunzip
   ```

---

## 11. Optional hardening

- **Rate limiting**: add Caddy `rate_limit` for the join endpoint to slow down
  password guessing on public networks.
- **Public directory**: the coordinator exposes a public network directory. If
  you don't want this, adjust the coordinator API or hide it at the reverse
  proxy.
- **Log retention**: the coordinator logs via journald. On small disks, cap
  journal size in `/etc/systemd/journald.conf`:

  ```ini
  SystemMaxUse=200M
  ```

- **Monitoring**: the coordinator's telemetry endpoint can be scraped for active
  networks and peer counts (see `docs/api.md`).

---

## 12. Known limitations

These are current code limitations discovered in real deployments; all are
candidate fixes in the repo:

- **Relay addresses are IP-only.** The coordinator's `--relay-server` /
  `--tcp-relay-server` flags parse as `SocketAddr`, so hostnames are rejected.
  Use the VPS public IP; if the IP ever changes, update the flags and restart
  the coordinator.
- **Default STUN servers fail to resolve.** `lanlinkd` logs
  `failed to parse default STUN server server="stun.l.google.com:19302"` for
  the same `SocketAddr` reason. Until hostnames are supported, NAT hole
  punching is effectively disabled and peers fall back to the relay. The relay
  path works but adds latency; configure STUN servers by IP in the daemon
  config (`[network] stun_servers`) to restore direct connections.
- **Frontends default to `http://localhost:8080`.** GUI/TUI/Web/ctl do not yet
  read the coordinator URL from the running daemon; always pass
  `--coordinator` (see §7.4).
