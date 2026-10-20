# Spec Delta

## Purpose

Provide an operator-facing admin web dashboard served directly by the LanLink
coordinator so operators can inspect networks, peers, aggregate telemetry,
and the public directory through a browser without relying on per-node UI.

## ADDED Requirements

### Requirement: Coordinator serves a static admin SPA
The coordinator SHALL serve a self-contained, vanilla-JS single-page admin
application on a dashboard path prefix, using the same visual style as the
existing `lanlink-web` interface. The assets SHALL be embedded into the
`coordinator` binary so no extra build step or on-disk files are required at
runtime.

#### Scenario: Operator opens the dashboard
- **WHEN** an operator navigates a browser to the dashboard base path
- **THEN** the coordinator returns the SPA HTML, CSS, and JavaScript assets with
the correct content types

#### Scenario: No external build toolchain is required
- **WHEN** the coordinator binary is built with `cargo build -p coordinator`
- **THEN** the dashboard assets are included at compile time and served without
any external bundler

### Requirement: Admin JSON endpoints expose network and peer metadata
The coordinator SHALL expose read-only JSON endpoints under an `/admin/*` path
prefix that return network membership and per-peer metadata. These endpoints
MUST reuse the existing SQLite-backed membership data and not introduce a second
source of truth.

#### Scenario: List all networks for the admin view
- **WHEN** an authorized client sends `GET /admin/networks`
- **THEN** the coordinator returns a JSON array where each entry contains the
network id, name, subnet, public/private flag, total peer count, and online peer
count

#### Scenario: List members of a network
- **WHEN** an authorized client sends `GET /admin/networks/{network_id}/members`
- **THEN** the coordinator returns a JSON array of members with node id,
virtual IP, last-seen timestamp, gateway/shared-subnet flags, and online status

#### Scenario: Read details of a single peer
- **WHEN** an authorized client sends `GET /admin/networks/{network_id}/peer/{node_id}`
- **THEN** the coordinator returns the peer's virtual IP, last-seen timestamp,
candidate addresses, and gateway/shared-subnet flags

### Requirement: Admin JSON endpoints expose aggregate metrics and telemetry
The coordinator SHALL expose read-only admin endpoints that surface the same
aggregate metrics and telemetry data already collected by the coordinator,
formatted for dashboard consumption.

#### Scenario: Admin view of fleet metrics
- **WHEN** an authorized client sends `GET /admin/metrics`
- **THEN** the coordinator returns the same aggregated metrics as `GET /metrics`

#### Scenario: Admin view of telemetry aggregates
- **WHEN** an authorized client sends `GET /admin/telemetry`
- **THEN** the coordinator returns telemetry window statistics including report
count, average peer counts, version distribution, and platform distribution

### Requirement: Dashboard and admin endpoints require operator token
Access to the dashboard SPA and every `/admin/*` JSON endpoint SHALL be gated
by a single operator token configured at coordinator startup. The dashboard
SHALL default to a loopback bind address so it is safe to place behind a
reverse proxy.

#### Scenario: Missing token is rejected
- **WHEN** a request arrives at the dashboard or any `/admin/*` endpoint without
the configured operator token
- **THEN** the coordinator returns HTTP 401 Unauthorized

#### Scenario: Valid token grants access
- **WHEN** a request includes the configured operator token in the
`Authorization: Bearer <token>` header
- **THEN** the coordinator serves the requested dashboard asset or JSON response

#### Scenario: Dashboard listens on loopback by default
- **WHEN** the coordinator starts with the dashboard enabled and no explicit bind
address
- **THEN** the dashboard HTTP listener binds to `127.0.0.1` on a default port

### Requirement: Dashboard v1 is read-only
The dashboard and its JSON endpoints in v1 SHALL be read-only. Any action that
would mutate coordinator state, such as creating or deleting networks or
removing members, SHALL require new endpoints that are explicitly documented as
out of scope for v1.

#### Scenario: Admin endpoint does not mutate state
- **WHEN** any request under `/admin/*` is issued
- **THEN** only read operations against the existing database are performed

#### Scenario: Write actions are explicitly deferred
- **WHEN** the dashboard UI contains buttons or links for write actions
- **THEN** those controls are disabled or hidden and the implementation tasks
list them as requiring new endpoints in a future change

### Requirement: Overview gracefully degrades when relay or signal URLs are absent
The dashboard overview screen SHALL display relay and signal service statistics
when the corresponding service URLs are configured, and SHALL show a neutral
"not configured" state instead of errors when they are absent.

#### Scenario: Relay/signal URLs are not configured
- **WHEN** the overview loads and no signal or relay service URL is configured
- **THEN** the relay and signal cards show a placeholder value and no failed
network requests are issued

#### Scenario: Relay/signal URLs are configured
- **WHEN** the overview loads with signal and/or relay service URLs configured
- **THEN** the dashboard requests the services' `/health` or `/metrics` endpoints
and displays available counters

### Requirement: Dashboard hosting is documented
The project SHALL include operator documentation that explains how to enable the
dashboard, configure the token, choose a bind address, and host the dashboard
behind a reverse proxy.

#### Scenario: New operator enables the dashboard
- **WHEN** an operator reads the dashboard hosting documentation
- **THEN** they can start the coordinator with dashboard enabled, set a secure
token, and reach the dashboard through a reverse proxy without reading source
code
