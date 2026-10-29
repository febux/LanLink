# Spec Delta

## Purpose

Provide safe, local control of the daemon's network session so users can disconnect from a network, reconnect, or stop the daemon cleanly without killing the process.

## ADDED Requirements

### Requirement: Daemon exposes a persistent local control socket

The daemon SHALL bind a local socket that survives network disconnect and serves both NDJSON status snapshots and control commands. On Unix it SHALL be a Unix domain socket; on Windows it SHALL be a TCP loopback socket.

#### Scenario: Socket exists while disconnected
- **WHEN** the daemon is running but not joined to any network
- **THEN** a client can still connect to the control socket and read a disconnected status snapshot

#### Scenario: Reject remote TCP connections
- **WHEN** a TCP connection to the control socket originates from a non-loopback address
- **THEN** the daemon SHALL close the connection without processing any command

### Requirement: Disconnect command leaves the network and keeps the daemon alive

The control socket SHALL accept a `disconnect` command. The daemon SHALL leave the current network, release TAP/IP configuration, clear firewall rules and gateway routes, notify the coordinator best-effort, and remain running.

#### Scenario: Disconnect while connected
- **WHEN** the daemon is joined to a network and receives `{"cmd":"disconnect"}`
- **THEN** the network session stops, network resources are released, and the daemon returns an acknowledgment

#### Scenario: Reconnect after disconnect
- **WHEN** the daemon has previously disconnected and receives `{"cmd":"connect",...}`
- **THEN** it joins the requested network without requiring a process restart

### Requirement: Stop command triggers graceful daemon shutdown

The control socket SHALL accept a `stop` command. The daemon SHALL release all network resources and exit with code 0.

#### Scenario: Stop while connected
- **WHEN** the daemon is joined to a network and receives `{"cmd":"stop"}`
- **THEN** it runs the same cleanup as disconnect and the process terminates with exit code 0

### Requirement: Status snapshot reports connected state

The daemon status snapshot SHALL include a `connected` boolean that is `true` when joined to a network and `false` after a disconnect.

#### Scenario: Snapshot after disconnect
- **WHEN** a client requests a snapshot after a disconnect command
- **THEN** the response contains `connected: false`, an empty peer list, and no TAP/interface name

## MODIFIED Requirements

### Requirement: Coordinator membership API supports leave notification

The coordinator SHALL expose a `POST /networks/:id/leave` endpoint so a daemon can notify it when leaving a network.

#### Scenario: Daemon sends leave
- **WHEN** a daemon sends a valid `LeaveNetworkRequest` to the leave endpoint
- **THEN** the coordinator removes the member from the network

#### Scenario: Invalid leave password
- **WHEN** the leave request password does not match the network password
- **THEN** the coordinator returns HTTP 401 and keeps the member
