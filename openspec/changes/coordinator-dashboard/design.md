# Design

## Context

- `crates/coordinator/src/lib.rs` currently builds a single Axum app with the
  public daemon API (`/networks`, `/directory`, `/telemetry`, `/metrics`,
  `/health`) and serves it on one address (default `0.0.0.0:8080`).
- `crates/coordinator/src/db.rs` already provides the queries we need:
  network listing is available through `list_public_networks`; member lists
  and peer details through `list_peers`; aggregate metrics through
  `aggregate_metrics`; telemetry records live in the `telemetry` table.
- `crates/lanlink-web/src/assets/` shows the established vanilla-JS SPA style:
  dark theme, tabbed layout, `include_str!` embedding, and a small `api()` helper.
- The `split-signal-relay-services` change will later add standalone signal and
  relay services with `/health` and `/metrics` endpoints. The dashboard should be
  able to display those aggregates when the coordinator knows their URLs.

See `proposal.md` for motivation and scope.

## Goals / Non-Goals

**Goals:**
- Serve a read-only, operator-facing dashboard from the coordinator binary.
- Add minimal, read-only admin JSON endpoints that reuse existing DB queries.
- Gate the dashboard with a single operator token and a loopback-by-default bind.
- Match the existing `lanlink-web` visual style and asset-embedding pattern.
- Document reverse-proxy hosting and token configuration.

**Non-Goals:**
- User accounts, RBAC, session management, or password recovery.
- Write endpoints that mutate networks or members in v1.
- Replacing `lanlink-web` or changing the per-node daemon UI.
- Prometheus/text metrics format; keep JSON for consistency with the rest of
  the coordinator.
- Native TLS termination; rely on the reverse proxy for HTTPS.

## Decisions

### 1. Separate dashboard listener bound to loopback by default

**Rationale:** The existing coordinator API must remain reachable on a public
address for daemons. Mixing the admin UI on the same listener would expose it to
the same surface. A second Axum app bound to `127.0.0.1:<port>` by default lets
operators put the dashboard behind a reverse proxy without changing the daemon
API surface.

**Approach:**
- Add `--dashboard-listen` / `--dashboard-token` CLI flags and a matching
  `[dashboard]` table in the coordinator config/TOML.
- If `--dashboard-token` is absent, the dashboard listener is not started and
  all dashboard routes are unavailable.
- Default bind: `127.0.0.1:8081`.

### 2. Auth middleware: single `Authorization: Bearer <token>` header

**Rationale:** No user accounts in v1. A single static operator token is the
smallest viable auth model and is easy to configure at the reverse proxy.

**Approach:**
- A `RequireToken` tower layer checks the `Authorization` header against the
  configured token for every request on the dashboard listener.
- The SPA assets (`/`, `/index.html`, `/style.css`, `/app.js`) are protected by
  the same layer so there is no unauthenticated HTML leak.
- Requests without the header or with a mismatched token receive HTTP 401.

### 3. Embed assets with `include_str!`

**Rationale:** Matches `lanlink-web` and avoids any front-end build step,
dependency, or runtime filesystem access.

**Approach:**
- Create `crates/coordinator/src/assets/index.html`,
  `crates/coordinator/src/assets/style.css`, and
  `crates/coordinator/src/assets/app.js`.
- The dashboard router uses `const INDEX_HTML: &str = include_str!(...)` and
  serves the assets with the correct `Content-Type` headers.

### 4. Reuse existing DB queries for admin endpoints

**Rationale:** The coordinator already owns membership and telemetry state.
Adding new tables or caches would duplicate truth.

**Approach:**
- `GET /admin/networks` queries `networks` and counts members with a `last_seen`
  cutoff (e.g., 5 minutes) to compute `online_count`.
- `GET /admin/networks/:id/members` calls `db::list_peers(..., exclude_node_id=[0;8])`
  and enriches each row with `last_seen` and an `online` boolean.
- `GET /admin/networks/:id/peer/:node_id` either reuses `list_peers` and filters
  client-side, or adds a small `db::get_member` helper if filtering is awkward.
- `GET /admin/metrics` calls `db::aggregate_metrics` with the default window.
- `GET /admin/telemetry` returns the same aggregate data in a shape convenient for
  the dashboard, possibly reusing `aggregate_metrics` plus version/platform
  distribution.

### 5. Relay/signal overview cards fetch service health lazily

**Rationale:** The `split-signal-relay-services` change will expose signal and
relay service URLs. The dashboard must not fail when those URLs are absent.

**Approach:**
- The coordinator stores optional `signal_url` and `relay_url` in `AppState`.
- The dashboard front-end reads these URLs from `GET /admin/config` (or from the
  `/admin/networks` response) and only requests `/health`/`/metrics` when they
  are present.
- If a service URL is missing, the UI renders "not configured" with no network
  request.

### 6. Route layout

The dashboard listener uses a dedicated router so the public daemon API is
unaffected:

| Path | Handler |
|---|---|
| `/` | `index.html` |
| `/index.html` | `index.html` |
| `/style.css` | stylesheet |
| `/app.js` | JavaScript |
| `/admin/networks` | JSON: all networks |
| `/admin/networks/:id/members` | JSON: network members |
| `/admin/networks/:id/peer/:node_id` | JSON: single peer |
| `/admin/metrics` | JSON: aggregated metrics |
| `/admin/telemetry` | JSON: telemetry aggregates |
| `/admin/directory` | JSON: public directory |

All paths except the SPA assets require the bearer token.

## Risks / Trade-offs

- **Single static token leaks are hard to rotate without restart** → Mitigation:
  document that the token should be stored in a file or secret manager and passed
  via CLI/config; future changes can add token reload or multiple tokens.
- **Embedding assets bloats the binary** → Mitigation: the dashboard is ~10 KB
  of HTML/CSS/JS; negligible compared to the rest of the coordinator.
- **Separate listener adds an open port** → Mitigation: default `127.0.0.1` only;
  reverse proxy handles TLS and access control; systemd unit uses the same
  sandboxing as the main coordinator.
- **Reading `/admin/networks` can be expensive on large fleets** → Mitigation:
  v1 targets small-to-medium fleets; if needed later, add pagination to the
  admin endpoints without breaking the dashboard.
- **Dashboard front-end needs coordinator version for cache busting** →
  Mitigation: embed the crate version in `app.js` as a global constant so the
  SPA can append a cache-busting query parameter to its API calls.

## Migration Plan

1. Upgrade the coordinator binary to the version that includes the dashboard.
2. Add to `/etc/lanlink/server.env` or the coordinator TOML:
   ```toml
   [dashboard]
   enabled = true
   bind = "127.0.0.1:8081"
   token = "<long-random-token>"
   ```
3. Update the systemd `ExecStart` line to pass `--dashboard-listen` and
   `--dashboard-token` (or a `--dashboard-config` file path).
4. Configure the reverse proxy (Caddy/nginx) to forward a subpath or subdomain
   to `127.0.0.1:8081` with its own access controls.
5. Operators open the proxied URL and supply the token.

Rollback: disable the dashboard by removing the token/setting `enabled = false`
and restarting the coordinator. The daemon API continues unchanged.

## Open Questions

- Should the dashboard also be reachable under the main coordinator listener on
  a separate path prefix for single-node test deployments? The separate listener
  is safer for production; a compatibility flag can be added later if needed.
- Should the admin endpoints be mirrored on the main API listener under
  `/admin/*` for consistency, or only on the dashboard listener? The current plan
  keeps them only on the dashboard listener; if operators want one listener,
  they can set both binds to the same interface.
