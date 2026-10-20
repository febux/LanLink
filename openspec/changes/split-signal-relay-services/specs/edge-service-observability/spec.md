# Spec Delta

## Purpose

Edge services (the new signal service and the existing relay service) expose a
consistent, small surface for health checks and operational metrics so that
operators can monitor them with standard tools.

## ADDED Requirements

### Requirement: Edge services expose a health endpoint
Both `lanlink-signal` and `lanlink-relayd` SHALL expose `GET /health` that
returns HTTP 200 when the service is ready to accept traffic and HTTP 503 when
it is not.

#### Scenario: Signal service reports healthy after startup
- **WHEN** `lanlink-signald` has finished binding its HTTP listener
- **THEN** `GET /health` returns HTTP 200 with a JSON body indicating the
  service is up

#### Scenario: Relay service reports healthy after startup
- **WHEN** `lanlink-relayd` has finished binding its UDP and optional TCP
  relay sockets
- **THEN** `GET /health` returns HTTP 200 with a JSON body indicating the
  service is up

### Requirement: Edge services expose a metrics endpoint
Both `lanlink-signal` and `lanlink-relayd` SHALL expose `GET /metrics` that
returns operational counters in the same JSON format already used by the
existing coordinator `/metrics` endpoint.

#### Scenario: Signal metrics show peer counts
- **WHEN** peers are registered with the signal service
- **THEN** `GET /metrics` returns JSON including the number of online peers,
  the number of networks with at least one peer, and uptime

#### Scenario: Relay metrics show relayed traffic
- **WHEN** `lanlink-relayd` has forwarded UDP and TCP packets
- **THEN** `GET /metrics` returns JSON including the number of active UDP
  bindings, active TCP sessions, total UDP/TCP packets relayed, and total
  UDP/TCP bytes relayed

### Requirement: Metrics endpoint does not expose secrets or payloads
The `/metrics` endpoint SHALL count only metadata and MUST NOT include node
ids, network ids, network names, IP addresses, or packet contents.

#### Scenario: Metrics response is safe to scrape
- **WHEN** a monitoring system scrapes `GET /metrics`
- **THEN** the response contains only aggregate counters and no PII or traffic
  content

### Requirement: Health and metrics endpoints do not interfere with data-plane sockets
The HTTP listener for `/health` and `/metrics` SHALL be served on a separate
socket from the UDP relay and signal WebSocket/control traffic.

#### Scenario: HTTP scrape under load does not drop relay packets
- **WHEN** the relay is forwarding a high volume of UDP traffic and a scraper
  hits `/metrics`
- **THEN** relay forwarding latency remains unaffected and no UDP packets are
  dropped due to the scrape
