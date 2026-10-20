# Spec Delta

## Purpose

The coordinator is the persistent, authenticated control plane for LanLink. It
manages network membership, directory listings, telemetry collection, and
aggregate metrics, while real-time peer presence and candidate exchange are
moved to the separate signal service.

## ADDED Requirements

### Requirement: Coordinator keeps network CRUD and directory
The coordinator SHALL continue to support creating networks, resolving
networks by name, joining networks, listing public networks on `/directory`,
and returning relay server URLs as part of join/create responses.

#### Scenario: Existing join endpoint still assigns IP and returns relay URLs
- **WHEN** a peer POSTs a valid `JoinNetworkRequest` to
  `/networks/{network_id}/join`
- **THEN** the coordinator assigns or re-assigns a virtual IP, persists the
  member record, and returns relay server addresses in the response, but the
  peer list may be empty or only contain coordinator metadata

### Requirement: Coordinator removes candidate exchange from join
The coordinator SHALL no longer return a full peer list with candidates in the
`JoinNetworkResponse` and SHALL no longer accept `POST
/networks/{network_id}/candidates` for candidate refresh.

#### Scenario: Join response omits peer candidates
- **WHEN** a peer joins a network with other members already present
- **THEN** the `JoinNetworkResponse` from the coordinator does not include
  candidate addresses of other peers

#### Scenario: Candidate endpoint returns deprecated response during migration
- **WHEN** an old daemon POSTs candidates to
  `/networks/{network_id}/candidates` during the migration window
- **THEN** the coordinator either returns a 404/410 with a message pointing to
  the signal service, or proxies the request to the configured signal service

### Requirement: Coordinator continues telemetry and metrics aggregation
The coordinator SHALL keep `POST /telemetry` and `GET /metrics` unchanged,
aggregating anonymized daemon reports into the existing metrics window.

#### Scenario: Telemetry submission unchanged
- **WHEN** a daemon POSTs a `TelemetryReport` to `/telemetry`
- **THEN** the coordinator records the report and returns HTTP 201

### Requirement: Coordinator advertises signal service URL
The coordinator SHALL expose the signal service base URL, either in join
responses, directory entries, or a dedicated endpoint, so that daemons can
reach the signal plane after joining.

#### Scenario: Daemon learns signal URL from coordinator
- **WHEN** a peer successfully joins or resolves a network
- **THEN** the response includes a `signal_url` field pointing to the signal
  service for that network
