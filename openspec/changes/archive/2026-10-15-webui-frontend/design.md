# LanLink WebUI Plan

## Goal

Add a browser-based management interface for LanLink that mirrors the existing
`lanlink-tui` capabilities (live daemon status, peer list, public directory,
create/join network) but is accessible from any browser on the local machine.

This is a **post-v1.0 / M9+** feature: it reuses the M9 daemon status socket and
coordinator REST API instead of adding new control-plane semantics.

## Architecture

```
┌─────────────┐      HTTP (127.0.0.1:8686)      ┌──────────────┐
│   Browser   │◄────────────────────────────────►│  lanlink-web │
│  (SPA)      │                                   │   (axum)     │
└─────────────┘                                   └──────┬───────┘
                                                         │
                              ┌──────────────────────────┼─────────────┐
                              │                          │             │
                       Unix socket                 HTTP REST      HTTP REST
                    /run/lanlink/*.sock             /directory    /networks
                              │                          │             │
                        ┌─────┴────┐              ┌──────┴────┐      │
                        │ lanlinkd │              │ coordinator│◄─────┘
                        │ (status) │              │            │
                        └──────────┘              └────────────┘
```

## Design decisions

1. **Single Rust binary, no JS build toolchain.** The frontend is a small
   vanilla-JS SPA embedded in the binary with `include_str!`. This avoids
   Node/npm in the Rust workspace and keeps packaging identical to the other
   LanLink binaries.

2. **Read-only daemon proxy.** `lanlink-web` only forwards `StatusRequest::Snapshot`
   from the daemon's Unix socket. It never mutates daemon state, never reads
   secrets, and cannot start/stop `lanlinkd`.

3. **Coordinator operations are write-through.** Create/join are forwarded to the
   coordinator REST API exactly like `lanlinkctl` and `lanlink-tui`. Join only
   returns the assigned IP and peer list; the user still starts `lanlinkd`
   separately to establish the data plane (matching the TUI model).

4. **Local-only binding by default.** Default listen address is `127.0.0.1:8686`
   so the WebUI is only reachable from the same machine. A `--bind` override is
   available for LAN/admin access at the operator's discretion.

5. **Auto-discover daemon socket.** If `--status-socket` is not given, scan
   `/run/lanlink/lanlinkd-*.sock` and `/tmp/lanlinkd-*.sock` (same logic as the
   TUI).

## Files to add

- `crates/lanlink-web/Cargo.toml`
- `crates/lanlink-web/src/main.rs` — CLI, server startup, static routes
- `crates/lanlink-web/src/api.rs` — API handlers and daemon-socket client
- `crates/lanlink-web/src/assets/index.html` — SPA shell
- `crates/lanlink-web/src/assets/style.css` — styling
- `crates/lanlink-web/src/assets/app.js` — frontend logic
- `docs/webui-plan.md` — this plan

No changes to other crates are required, but `README.md` will be updated with
usage instructions.

## API surface

| Method | Path | Description |
|--------|------|-------------|
| GET | `/` | Serve the SPA (`index.html`) |
| GET | `/style.css` | Serve stylesheet |
| GET | `/app.js` | Serve frontend script |
| GET | `/api/health` | Returns `{"ok": true}` |
| GET | `/api/snapshot` | Current daemon snapshot (proxies Unix socket) |
| GET | `/api/directory` | Public directory from coordinator |
| POST | `/api/networks` | Create network (body: `CreateNetworkRequest`) |
| POST | `/api/networks/:id/join` | Join network (body: `JoinNetworkRequest` without `node_id`/`candidates`, which the server fills) |

All JSON endpoints set `Content-Type: application/json` and permissive CORS
headers for local development.

## Frontend screens

1. **Dashboard** — daemon online/offline badge, assigned IP, TAP name, bind
   address, uptime, gateway/firewall flags, total TX/RX bytes.
2. **Peers** — sortable table: virtual IP, latency, path kind (direct/relay/
   punching/waiting), endpoint, last seen, TX/RX bytes, gateway flag.
3. **Networks** — public directory list; forms to create a new network and join an
   existing one. Results show network id, subnet, and assigned IP.
4. **Help/About** — short usage note and links to docs.

The SPA polls `/api/snapshot` every second and `/api/directory` every 10 s,
mirroring the TUI poller cadence.

## Security notes

- The WebUI is unauthenticated and bound to localhost by design; it exposes the
  same operational data as the TUI.
- It does not read `password_file`s, private keys, or any daemon secrets.
- Coordinator create/join operations require the same password the user would type
  into `lanlinkctl`; the WebUI merely forwards them.

## Testing

- `cargo build -p lanlink-web` compiles cleanly.
- `cargo test -p lanlink-web` runs unit tests for the daemon-socket client
  against a fake Unix listener.
- Manual smoke test: start coordinator + daemon, run `cargo run -p lanlink-web`,
  open `http://127.0.0.1:8686`, verify dashboard/peer/directory data.

## Future improvements (out of scope for this PR)

- WebSocket push from daemon instead of polling.
- Authenticated/multi-user WebUI for remote admin.
- Start/stop `lanlinkd` from the browser via a privileged helper.
- Dark/light theme toggle and responsive mobile layout.
