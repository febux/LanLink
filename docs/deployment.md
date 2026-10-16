# Coordinator Deployment Guide

This guide describes how to deploy the **LanLink coordinator** on a VPS with a
stable domain name. The coordinator is the only piece of the architecture that
*must* be hosted on a public address: it tells peers where to find each other and
advertises relay fallback addresses. Relay traffic itself is peer-to-end
encrypted; the coordinator cannot decrypt it.

This guide uses **Caddy** for automatic HTTPS because the coordinator has no
native TLS support. The same machine can also run `lanlink-relayd`; relay
deployment is included at the end so the advertised fallback addresses actually
exist.

Replace `coord.example.com` with your domain throughout.

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
  --relay-server coord.example.com:3478 \
  --tcp-relay-server coord.example.com:3479
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

Important: `--relay-server` and `--tcp-relay-server` must be the **public**
addresses advertised to joining peers. If you omit them, the coordinator
defaults to `127.0.0.1`, which is wrong for a public deployment.

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

Peers only need the coordinator URL:

```bash
LANLINK_PASSWORD='...' sudo -E lanlinkd up --name gaming-night \
  --coordinator https://coord.example.com
```

Relay addresses are returned in the coordinator's join response.

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
