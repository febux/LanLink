# Proposal

## Why

The `coordinator` crate exposes a REST control plane for network membership,
directory, telemetry, and metrics, but operators have no built-in, operator-facing
view into the fleet. Today the only browser UI is `lanlink-web`, which is a
per-node, local daemon proxy on `127.0.0.1:8686`. An admin dashboard served
_directly_ by the coordinator gives operators a single place to inspect
networks, peers, virtual IPs, last-seen times, aggregate telemetry, and the
public directory without touching the data-plane socket.

## What Changes

- **Static admin SPA served by the coordinator** on `/ui/*` (or `/admin/*`):
  vanilla-JS single-page application, embedded with `include_str!` and served
  from the existing Axum router in `crates/coordinator`. Style matches the
  existing `lanlink-web` dark, minimal aesthetic; no front-end build toolchain.
- **Read-only JSON admin endpoints** added to the same Axum router:
  - `GET /admin/networks` — list all networks with member counts and online/total
    peer counts.
  - `GET /admin/networks/:id/members` — detailed member list (virtual IP,
    last-seen, gateway/shared-subnet flags).
  - `GET /admin/networks/:id/peer/:node_id` — individual peer details (virtual
    IP, candidates, last-seen).
  - `GET /admin/metrics` and `GET /admin/telemetry` — same aggregate data as
    the existing `/metrics` and `/telemetry` endpoints, but exposed under the
    admin namespace.
  - `GET /admin/directory` — admin view of the public directory.
- **Operator-token authentication** for the dashboard and all `/admin/*` JSON
  endpoints: a single `[dashboard] token` in the coordinator TOML/CLI, default
  disabled, bind address defaulting to `127.0.0.1` so the dashboard is safe
  behind a reverse proxy. No user accounts in v1.
- **Dashboard screens**: overview (networks, peers online/total; relay/signal
  statistics degrade gracefully when `split-signal-relay-services` service URLs
  are not configured), network table with members, member detail, and directory
  management.
- **Documentation** in `docs/` covering dashboard hosting, reverse-proxy setup,
  and token configuration.

v1 is intentionally read-only. Any write surface (creating networks, removing
members, etc.) is out of scope and must be explicitly marked as requiring new
endpoints in later work.

## Capabilities

### New Capabilities

- `coordinator-dashboard`: an operator-facing admin web dashboard served by the
  `coordinator` crate, including a vanilla-JS SPA, read-only admin JSON
  endpoints, operator-token auth, and fleet/network/peer/directory views.

### Modified Capabilities

None. The existing `windows-gateway` spec is unrelated to this change.

## Impact

- Modified crate: `crates/coordinator` (`src/lib.rs` for new routes and auth
  middleware, `src/api.rs` for admin JSON handlers, `src/main.rs` for dashboard
  config flags, and new embedded assets under `src/assets/`).
- New documentation: `docs/dashboard-hosting.md`.
- No changes to `lanlink-web`, the daemon, relay, signal services, or the
  public coordinator API contract used by daemons.
