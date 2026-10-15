# TUI Build Plan

Plan for `lanlink-tui`, a terminal frontend for LanLink. Covers the
prerequisite daemon status API, the TUI crate itself, testing, and rollout.

Status: **implemented** (see §10 for what shipped).

## 1. Context

LanLink today ships a privileged daemon (`lanlinkd`), a REST CLI
(`lanlinkctl`, coordinator-facing only), and a self-hostable `coordinator`.
PLAN.md lists the GUI/TUI as the post-v1.0 stretch goal; requirements F1, F4,
and F7 expect network create/join, a node list with status (online/offline,
latency), and connection-type display from a UI.

The blocking gap: **the daemon exposes no local status or control surface.**
All runtime state (peer table, verified paths, traffic counters) lives inside
the process and only surfaces through `tracing` logs. A TUI needs live data
first.

### Locked decisions

| Decision | Choice |
|---|---|
| Binary shape | Standalone `crates/lanlink-tui` → `lanlink-tui` (keeps `lanlinkctl` lean, matches one-binary-per-crate layout) |
| Scope | All-in-one: monitoring **and** create/join flows in v1 |
| Daemon IPC | Unix domain socket (Linux/macOS), named pipe on Windows; newline-delimited JSON |
| Latency probes | Included in the daemon groundwork (required for F4 parity) |

### Goals

- Live view of the joined network: peers, connection type, latency, traffic.
- Create networks, browse the public directory, and join — feature parity
  with `lanlinkctl coord *` and `lanlinkctl directory list`.
- Works over SSH; no daemon restart needed to attach/detach the TUI.

### Non-goals (v1)

- Starting/stopping the daemon or joining *and running the data plane* from
  the TUI (the daemon is root-privileged and managed by systemd/SCM/launchd).
- Editing daemon configuration files.
- Windows named-pipe transport beyond a stub (TAP is stubbed on Windows
  anyway); macOS utun follows the same UDS code path as Linux.

## 2. Architecture

```
┌───────────────────────── lanlink-tui (user) ─────────────────────────┐
│  ratatui + crossterm UI      coordinator REST (reqwest)              │
│  input task ─┐               create / join / directory               │
│  poll tasks ─┼─ mpsc ─▶ App state (pure reducer) ─▶ render loop      │
└──────┬───────────────────────────────┬───────────────────────────────┘
       │ 1 Hz snapshot (NDJSON)        │ REST
       ▼                               ▼
┌── lanlinkd (root) ───┐        ┌── coordinator ──┐
│ status server (UDS)  │        │ axum + sqlite   │
│ Switch / Puncher /   │        └─────────────────┘
│ Metrics / peer meta  │
└──────────────────────┘
```

Two data sources, clearly separated:

1. **Daemon status socket** — live data-plane truth (peers, latency, bytes).
2. **Coordinator REST** — control-plane operations, same endpoints `lanlinkctl`
   already uses (`POST /networks`, `POST /networks/:id/join`,
   `GET /directory`). Reuse `lanlink-proto` request/response types verbatim.

## 3. Phase 0 — Daemon status API (foundation)

### 3.1 Shared types — `lanlink-proto/src/status.rs` (new)

```rust
pub enum StatusRequest { Snapshot }                 // serde tag = "cmd"
pub enum PathKind { Direct, Relay, Punching, Waiting }
pub struct DaemonStatus { version, network_id, assigned_ip, tap_name, bind,
                          relay_addr, gateway_enabled, firewall_enabled,
                          uptime_seconds, tx_bytes, rx_bytes }
pub struct PeerStatus  { node_id, virtual_ip, endpoint, path, latency_ms,
                         last_seen_secs_ago, tx_bytes, rx_bytes, is_gateway,
                         shared_subnet }
pub struct StatusSnapshot { status: DaemonStatus, peers: Vec<PeerStatus> }
pub enum StatusResponse { Snapshot(StatusSnapshot), Error { error } }  // untagged
```

Protocol: client connects, writes one JSON `StatusRequest` per line, daemon
answers each line with one `StatusResponse`. Read-only; carries no passwords
or key material. Connection may stay open across polls.

### 3.2 Switch extensions — `lanlink-switch`

`PeerState` and the UDP ingress need to carry what the status API reports:

- `PeerState` gains `tx_bytes`, `rx_bytes`, `rtt: Option<Duration>`.
- `on_tap_frame` accumulates per-receiver `tx_bytes`; `on_udp_packet`
  accumulates per-sender `rx_bytes` (use sealed packet length).
- New accessor `peer_stats() -> Vec<PeerStat>` (id, addr, last_seen, counters,
  rtt) for the status server.
- **Return-type change:** `on_udp_packet` returns a `UdpOutcome` enum —
  `Frame(Vec<u8>)` (write to TAP), `Reply(Vec<u8>)` (send back to the
  datagram's source; needed for PONGs), `Ignored`. Update the daemon rx loop
  and all existing switch tests.

**Latency probes (PING/PONG).** New sealed control packets reusing the
existing `PING_PACKET_TYPE`/`PONG_PACKET_TYPE` constants:

- Inner payload: `[sender id: 8][tx counter: 8][timestamp ms: 8]` — the same
  counter stream as DATA packets, so replay windows stay effective.
- Timestamps are **milliseconds since `Switch::new`** (a monotonic probe
  clock), *not* wall time: the pinger's clock is echoed verbatim, so RTT is
  valid regardless of clock skew between peers.
- On PING: update liveness/NAT rebind, reply with PONG echoing the timestamp.
- On PONG: `rtt = probe_clock_ms() - ts`.
- New `seal_ping(peer, now_ms)`; daemon pump sends pings every ~5 s alongside
  the existing 20 s keepalive loop.

**Keepalive bug to fix (pre-existing).** `seal_keepalive` seals an *empty*
inner payload, but `on_udp_packet` rejects any inner shorter than
`NODE_ID_LEN + 8` with `Malformed` *before* reaching its `KEEPALIVE` branch —
so received keepalives are currently treated as errors and never refresh
`last_seen`. Fix: keepalives carry `[sender id][counter]` (sender attribution
without replay state), and the `KEEPALIVE` branch runs before the
length/replay checks, updating `last_seen` and NAT rebinds. Wire-format change
is acceptable pre-beta (daemon versions ship together).

**Implementation note (borrow checker).** While accumulating per-peer
`tx_bytes` inside `on_tap_frame`'s destination loop, inline the counter field
access (`self.tx_counter += 1`) instead of calling `self.tx_counter()` —
method calls borrow all of `self` and conflict with the live `self.peers`
borrow; direct field access keeps the borrows disjoint.

**Constraint discovered:** the daemon must seal probes through the *switch's*
`StaticSession` (as above), not a private `session.clone()`. Independent
clone → independent counter stream → counter collisions → peers' replay
windows silently drop valid data packets.

### 3.3 Daemon wiring — `lanlinkd/src/status.rs` (new)

- `StatusRegistry` (shared state, `Arc`):
  - `DaemonInfo` — set once after join: network id, assigned IP, tap name,
    bind addr, gateway/firewall flags, bound relay addr.
  - `peer_meta: HashMap<NodeId, PeerMeta>` — virtual IP / gateway flags /
    shared subnet from the coordinator's `PeerInfo` view; updated by the
    30 s refresh loop and initial join.
- `serve(socket_path, registry, switch, metrics, start_time, shutdown)`:
  tokio UDS listener; one line in → one JSON line out; concurrent
  connections allowed. Snapshot assembly locks the switch briefly
  (`switch.lock().await.peer_stats()` — never hold across awaits).
- **PathKind computation** (daemon-side, since it knows relay addresses and
  punch state): peer in `punching` set → `Punching`; endpoint ∈ relay-server
  set → `Relay`; endpoint set → `Direct`; else `Waiting`.
- **Socket path:** `--status-socket` flag + `[status] socket` config key;
  default `/run/lanlink/lanlinkd-<network_id>.sock`, mode `0660`
  (root:lanlink), directory created at startup; fall back to
  `/tmp/lanlinkd-<network_id>.sock` for unprivileged dev runs. Failure to
  create the socket logs a warning and the daemon continues — status support
  must never take down the data plane.
- `cfg(windows)`: stub returning "unsupported" until the Wintun backend
  lands; `cfg(unix)` carries the implementation.
- Spawn from `run_coordinated` after the NIC is up, alongside the existing
  refresh/telemetry tasks; abort on shutdown.

## 4. Phase 1 — `lanlink-tui` crate skeleton

New crate `crates/lanlink-tui`, binary `lanlink-tui`. Workspace gains
`ratatui = "0.29"` and its `crossterm` in `[workspace.dependencies]`.

Module layout:

```
src/
├── main.rs    args (coordinator URL, status socket path), terminal
│              setup/teardown, panic hook (restore terminal), event loop
├── app.rs     App state + key-handling reducer; pure, unit-testable
├── ui.rs      ratatui widgets per screen; no business logic
├── daemon.rs  status-socket client (tokio UnixStream, NDJSON)
└── coord.rs   coordinator REST client (reqwest + lanlink-proto types)
```

Event loop (all cross-task communication over one mpsc channel):

- **Input task** — crossterm events → `Event::Key`.
- **Snapshot poller** — 1 Hz connect-or-reuse → `Event::Snapshot` /
  `Event::SnapshotError` (drives the "daemon offline" indicator).
- **Directory poller** — 10 s `GET /directory` → `Event::Directory`.
- **Action worker** — receives form submissions; performs create/join REST
  calls; returns `Event::CoordResult` (spawned by the reducer via an
  action channel, keeping the reducer itself pure and sync).
- **Render** — on every event plus a 250 ms tick (for uptime/rate refresh).

Terminal hygiene: raw mode + alternate screen entered in `main`, restored on
exit *and* via panic hook; `q`/`Ctrl-C` quit.

## 5. Phase 2 — Screens

| Screen | Contents | Keys |
|---|---|---|
| **Dashboard** | network id/name, own virtual IP, tap name, uptime, bound relay, gateway/firewall badges, tx/rx totals + rate sparklines (60-sample ring buffer), peer count | `1` / `←→` tabs |
| **Peers** | table: node id (short hex), virtual IP, endpoint, path, latency (colored), last seen, tx/rx; sort by latency | `2`, `s` cycle sort |
| **Networks** | left: public directory list (name, members, public flag); right: create form (name, password, public toggle) and join form (network id, password) — parity with `lanlinkctl` semantics; join results (assigned IP, peers) shown in a result panel | `3`, `c` create, `j` join, form navigation |
| **Help** | overlay with all bindings | `?` toggle |
| Status bar | daemon connection state, coordinator URL, transient toasts/errors | — |

Color coding: `Direct` green, `Relay` yellow, `Punching` cyan, `Waiting` dim;
latency < 50 ms green, < 150 ms yellow, else red. No secrets are ever
rendered; the join form masks the password field.

## 6. Phase 3 — Tests & docs

- **lanlink-switch:** update existing tests to `UdpOutcome`; add round-trip
  tests for ping→pong→rtt, per-peer counters, keepalive liveness fix, replay
  rejection of probes.
- **lanlink-proto:** serde round-trip for `StatusSnapshot`/`StatusRequest`.
- **lanlinkd:** unit-test snapshot assembly from a registry + fake switch
  state; PathKind classification table test.
- **lanlink-tui:** reducer unit tests (tab switching, form editing, sort
  cycling, quit); integration test against a fake UDS server serving canned
  snapshots (`#[tokio::test]` + tempdir socket).
- **Gates:** `make fmt`, `make clippy` (`-D warnings`), `make test`.
- **Docs:** README quick-start section + repo layout entry, PLAN.md stretch
  goal → milestone update, this document linked from `docs/SUMMARY.md`.

## 7. Suggested build order

1. `lanlink-proto/src/status.rs` types + round-trip tests (no deps).
2. Switch extensions + keepalive fix + tests (self-contained).
3. `lanlinkd/src/status.rs` server, `--status-socket` flag, ping loop wiring.
4. TUI skeleton: terminal lifecycle, event loop, daemon client, dashboard.
5. Peers table, then Networks screen + forms + action worker.
6. Help overlay, toasts, color polish.
7. Tests, docs, fmt/clippy pass.

Steps 1–3 (daemon side) and step 4's crate scaffold can proceed in parallel;
the TUI can be developed against a fake status server from day one.

## 8. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Status server blocks the data plane (lock contention) | Snapshot locks the switch only to copy `peer_stats()`; never held across awaits; status failure never aborts the daemon |
| UDS permissions lock out non-root users | Socket mode `0660 root:lanlink`; document adding users to the `lanlink` group; `/tmp` fallback for dev |
| Keepalive/probe wire changes break mixed versions | Pre-beta: acceptable; note in protocol docs (`docs/protocol.md`) alongside the change |
| ratatui API churn (0.x) | Pin exact version in workspace deps; UI code isolated in `ui.rs` |
| TUI leaves terminal broken on panic | Panic hook + alternate screen teardown in `main`; tested via a deliberate-panic debug flag |

## 9. Acceptance checklist

- [x] `lanlinkd run` serves snapshots on its status socket; the NDJSON round trip is covered by `lanlinkd` tests (`status_socket_round_trip`) — live `socat` verification against a root daemon still pending a lab run.
- [x] Peer latency values refresh: PING/PONG RTT covered by switch tests; the daemon probes every 5 s.
- [x] Direct vs Relay vs punching classification covered by unit tests (`snapshot_classifies_paths_and_merges_meta`); netns-lab visual check still pending.
- [x] Create + join from the TUI use the same endpoints and proto types as `lanlinkctl`.
- [x] Terminal survives quit, Ctrl-C; panic hook restores the terminal.
- [x] `make fmt clippy test` green.

## 10. What shipped

- `lanlink-proto`: `status` module (request/response/snapshot types, serde round-trip tests); `JoinNetworkResponse` is now `Clone`.
- `lanlink-switch`: per-peer tx/rx byte counters; `UdpOutcome` (frame/reply/ignored) return type; PING/PONG probes with a monotonic probe clock; keepalives now carry sender id + counter and refresh liveness/NAT rebinds (fixes the empty-inner keepalive bug).
- `lanlinkd`: `status.rs` — registry, snapshot assembly, path classification, UDS server (default `/run/lanlink/lanlinkd-<id>.sock`, `/tmp` fallback, 0660, graceful degradation, Windows stub); `--status-socket` flag + `[status] socket` config key; 5 s ping loop; rx loops handle probe replies.
- `lanlink-tui`: ratatui 0.29 + crossterm binary with dashboard (info, tx/rx sparklines, direct-path gauge), peers table (sort by latency/ip/traffic, path/latency coloring), networks screen (directory list, create/join forms with masked passwords, join results), help overlay, status bar with daemon-online indicator and toasts; pure reducer with 13 unit tests; status client with fake-server integration tests.
- Docs: protocol (control probes + local status API), README (M9 status, layout, quick start), PLAN.md M9 row.
