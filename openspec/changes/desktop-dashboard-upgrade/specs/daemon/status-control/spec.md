# Spec Delta

## Purpose

The daemon's local status socket exposes read-only operational metadata to authorized frontends and accepts a minimal, safe set of control commands that let those frontends manage the local network session without exposing secrets over the network.

## ADDED Requirements

### Requirement: Status snapshot exposes richer peer metadata
The daemon's `StatusSnapshot` SHALL include, for each peer, the explicit transport/path kind, latency, TX/RX bytes, TX/RX packets, TX/RX rate, session/Noise state, peer uptime, virtual MAC address, and full address candidate list in addition to the existing fields.

#### Scenario: Peer metadata present
- **WHEN** a frontend requests a snapshot and a peer has an active data-plane session
- **THEN** the response includes the peer's `transport`, `tx_packets`, `rx_packets`, `session_state`, `uptime_seconds`, and `mac` fields

#### Scenario: Peer address list present
- **WHEN** a peer was learned through multiple candidate addresses
- **THEN** the snapshot includes a list of known addresses/endpoints for that peer

### Requirement: Status snapshot exposes network summary counters
The daemon's `StatusSnapshot` SHALL include, at the daemon level, total peer count and relay usage share so the dashboard can render summary cards without recomputing from the peer array.

#### Scenario: Network summary included
- **WHEN** a frontend requests a snapshot while the daemon has joined a network
- **THEN** the response includes `peer_count` and `relay_share_percent` in the daemon status

### Requirement: Status socket accepts leave network command
The daemon's local status socket SHALL accept a `leave_network` command that causes the daemon to leave the current network and clear its data-plane state.

#### Scenario: Leave network requested
- **WHEN** an authorized local frontend sends `{"cmd":"leave_network"}`
- **THEN** the daemon leaves the network, returns an acknowledgment, and the next snapshot reflects the empty state

### Requirement: Status socket accepts re-punch command
The daemon's local status socket SHALL accept a `repunch_peer` command that starts a new hole-punch attempt for the specified peer.

#### Scenario: Re-punch peer requested
- **WHEN** an authorized local frontend sends `{"cmd":"repunch_peer","node_id":"..."}`
- **THEN** the daemon attempts a new direct path to that peer and returns an acknowledgment

#### Scenario: Re-punch unknown peer
- **WHEN** the requested node ID is not a known peer
- **THEN** the daemon returns an error response and takes no action

### Requirement: Status socket accepts gateway toggle command
The daemon's local status socket SHALL accept a `set_gateway` command that enables or disables gateway mode at runtime when the platform supports it.

#### Scenario: Enable gateway
- **WHEN** an authorized local frontend sends `{"cmd":"set_gateway","enabled":true}`
- **THEN** the daemon enables gateway mode and the next snapshot shows `gateway_enabled: true`

#### Scenario: Disable gateway
- **WHEN** an authorized local frontend sends `{"cmd":"set_gateway","enabled":false}`
- **THEN** the daemon disables gateway mode and the next snapshot shows `gateway_enabled: false`

#### Scenario: Gateway toggle unsupported
- **WHEN** the platform or current configuration does not support runtime gateway toggling
- **THEN** the daemon returns an error response and leaves gateway state unchanged

### Requirement: Control commands require local access
The daemon SHALL accept write/control commands only over the local status socket (Unix domain socket or TCP loopback), relying on OS-local access control; no additional authentication tokens are required for local frontends.

#### Scenario: Local frontend invokes control
- **WHEN** the desktop app running on the same host sends a control command
- **THEN** the daemon accepts and processes the command

#### Scenario: Remote connection rejected
- **WHEN** a connection to the status endpoint originates from a non-loopback address
- **THEN** the daemon does not accept the connection
