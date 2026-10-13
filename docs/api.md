# Coordinator REST API Reference

Base URL: `https://<coordinator-host>:8080`

## `POST /networks`

Create a new network.

**Request body:**
```json
{
  "name": "gaming-night",
  "password": "hunter2",
  "is_public": true
}
```

**Response `201 Created`:**
```json
{
  "network_id": 1,
  "subnet": "26.0.1.0/24",
  "is_public": true
}
```

## `POST /networks/:id/join`

Join an existing network.

**Request body:**
```json
{
  "password": "hunter2",
  "node_id": "AAAAAAAAAAE=",
  "public_key": "",
  "direct_candidates": ["192.168.1.10:9000"],
  "relay_candidates": [],
  "shared_subnet": ""
}
```

**Response `200 OK`:**
```json
{
  "network_id": 1,
  "assigned_ip": "26.0.1.2",
  "peers": [...],
  "relay_servers": ["203.0.113.50:3478"],
  "is_public": true
}
```

## `GET /networks/:id/members`

List current members. In v1.0 this endpoint is intentionally unauthenticated;
production deployments should gate it with the same password used at join time.

## `POST /networks/:id/candidates`

Submit local candidates and receive the latest peer list.

## `GET /directory`

List public/gaming networks and their approximate online member counts.

## `POST /telemetry`

Submit an opt-in anonymized telemetry report.

**Request body:**
```json
{
  "version": "0.1.0",
  "platform": "x86_64-linux",
  "uptime_seconds": 3600,
  "peer_count": 4,
  "direct_peer_count": 3,
  "relay_peer_count": 1,
  "gateway_enabled": false,
  "firewall_enabled": true,
  "tx_bytes": 1024,
  "rx_bytes": 2048,
  "timestamp": "2026-10-11T00:00:00Z"
}
```

No node id, network id, IP address, or traffic content is included.

## `GET /metrics`

Return aggregated telemetry for the last 24 hours (operator view).
