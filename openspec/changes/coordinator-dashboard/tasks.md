# Tasks

## 1. Dashboard configuration and CLI

Ticket: t_f043fae9

- [ ] 1.1 Add `[dashboard]` configuration support to `crates/coordinator/src/main.rs`:
  CLI flags `--dashboard-listen` (default `127.0.0.1:8081`), `--dashboard-token`
  (optional), and `--dashboard-enabled`; also read an optional TOML config file
  path from `--dashboard-config`. Verify with `cargo run --bin lanlink-coordinator -- --help`
  that the new flags appear.
- [ ] 1.2 When `dashboard_enabled` is true and a token is present, spawn the
  dashboard Axum app on `dashboard_listen` alongside the main coordinator
  listener in `crates/coordinator/src/main.rs`; verify both listeners accept
  connections when started.
- [ ] 1.3 Update `dist/systemd/lanlink-coordinator.service` to document the new
  dashboard flags in a comment, without changing the default daemon-only
  `ExecStart`; verify the unit still parses with `systemd-analyze verify` if
  available.

## 2. Dashboard router, auth middleware, and static assets

Ticket: t_f043fae9

- [ ] 2.1 Add a new `dashboard` module to `crates/coordinator/src/lib.rs` that
  exposes `dashboard_router(state: Arc<AppState>, token: String) -> Router`, and
  mount the dashboard routes on a separate listener. Verify
  `cargo check -p coordinator` passes.
- [ ] 2.2 Implement bearer-token auth middleware in the dashboard module,
  checking `Authorization: Bearer <token>` and returning HTTP 401 for missing or
  invalid tokens; verify with a unit test that requests without the token are
  rejected.
- [ ] 2.3 Create `crates/coordinator/src/assets/index.html`,
  `crates/coordinator/src/assets/style.css`, and
  `crates/coordinator/src/assets/app.js` matching the `lanlink-web` vanilla-JS
  SPA style, embedded via `include_str!`. Verify the dashboard listener returns
  each asset with the correct `Content-Type` and HTTP 200.

## 3. Read-only admin JSON endpoints

Ticket: t_f043fae9

- [ ] 3.1 Implement `GET /admin/networks` in `crates/coordinator/src/api.rs`
  (or the dashboard module) that returns id, name, subnet, public flag,
  total peer count, and online peer count; verify the response against a test
  database with at least two networks and several members.
- [ ] 3.2 Implement `GET /admin/networks/:id/members` in
  `crates/coordinator/src/api.rs` that returns node id, virtual IP, last-seen,
  gateway/shared-subnet flags, and online status; verify it matches the
  underlying `db::list_peers` results.
- [ ] 3.3 Implement `GET /admin/networks/:id/peer/:node_id` in
  `crates/coordinator/src/api.rs` that returns a single peer's details or HTTP
  404; verify with a test that an existing peer is returned and a missing peer
  yields 404.
- [ ] 3.4 Implement `GET /admin/metrics` and `GET /admin/telemetry` in
  `crates/coordinator/src/api.rs` by reusing `db::aggregate_metrics`; verify
  both endpoints return JSON containing the expected aggregate fields.
- [ ] 3.5 Implement `GET /admin/directory` in `crates/coordinator/src/api.rs` that
  returns the public network directory; verify it matches `GET /directory`.

## 4. Dashboard front-end screens

Ticket: t_f043fae9

- [ ] 4.1 Build the overview screen in `app.js` showing network count,
  total/online peer counts, and cards for relay/signal service health that show
  "not configured" when no URL is set; verify by loading `/ui` in a browser and
  checking the rendered HTML.
- [ ] 4.2 Build the networks table screen with a sortable/filterable table of
  networks and an inline member count; verify by clicking through the UI or
  inspecting the DOM.
- [ ] 4.3 Build the network detail / members table screen with a list of peers
  including virtual IP, last-seen, and gateway flags; verify navigation from
  the networks table works.
- [ ] 4.4 Build the member detail screen and the public directory view; verify
  data matches the corresponding `/admin/*` JSON endpoints.
- [ ] 4.5 Disable or hide any write-action controls in the UI and add comments
  marking them as v2; verify no button triggers a non-existent endpoint.

## 5. Integration, tests, and documentation

Ticket: t_f043fae9

- [ ] 5.1 Add integration tests under `crates/coordinator/tests/api.rs` that
  exercise the dashboard endpoints with a valid token and assert 401 without a
  token; verify `cargo test -p coordinator` passes.
- [ ] 5.2 Add a small helper test in `crates/coordinator/tests/api_test.rs`
  confirming the dashboard assets are served and the admin endpoints require
  auth; verify it compiles and passes.
- [ ] 5.3 Write `docs/dashboard-hosting.md` covering enabling the dashboard,
  token generation, bind address choice, Caddy/nginx reverse-proxy snippets,
  and systemd flag examples; verify by reading the rendered file.
- [ ] 5.4 Update `docs/SUMMARY.md` to include the new dashboard hosting page;
  verify the entry appears.
- [ ] 5.5 Run `cargo check --workspace` and `cargo test --workspace` and confirm
  no regressions in `coordinator` or dependent crates.

## 6. OpenSpec validation and handoff

Ticket: t_f043fae9

- [ ] 6.1 Run `openspec validate --change coordinator-dashboard` and confirm all
  planning artifacts pass validation.
- [ ] 6.2 Review that every task references `crates/coordinator/src/api.rs` or
  `crates/coordinator/src/lib.rs` (directly or through the dashboard module) and
  that the docs task references `docs/`.

## Workflow follow-up

- Apply the change once review is complete, implementing the tasks above in
  dependency order.
- Archive the change after implementation and tests are merged/reviewed.
- Verify the archived result.
