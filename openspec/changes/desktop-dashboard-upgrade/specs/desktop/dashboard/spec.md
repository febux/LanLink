# Spec Delta

## Purpose

The LanLink desktop dashboard is the primary interactive window for monitoring the local daemon and network peers, and for issuing safe local control actions such as leaving the network, reconnecting a peer, or inspecting peer details.

## ADDED Requirements

### Requirement: Dashboard displays rich daemon and network status
The desktop dashboard SHALL display the daemon's assigned virtual IP, TAP device, bind address, uptime, firewall profile, gateway status, relay address, total peer count, and aggregate relay usage share.

#### Scenario: Network is joined
- **WHEN** the daemon has joined a network and the desktop app receives a snapshot
- **THEN** the dashboard shows the network ID, assigned IP, TAP name, bind address, and uptime

#### Scenario: Relay usage computed
- **WHEN** the peer list contains peers on `relay`, `tcp_relay`, or `punching` paths
- **THEN** the dashboard shows the share of peers not on a direct path as a percentage of total peers

### Requirement: Dashboard displays rich per-peer status
The peer table SHALL show, for each peer: virtual IP, virtual MAC, path kind with explicit transport label, measured latency, last-seen age, TX/RX bytes, TX/RX packets, current data rate, session/Noise handshake state, peer uptime, gateway role, and shared subnet.

#### Scenario: Peer is connected
- **WHEN** a peer has an authenticated data-plane session
- **THEN** the table shows its virtual IP, path (`direct`, `relay`, `tcp_direct`, `tcp_relay`, `punching`, or `waiting`), latency when available, and byte/packet counters

#### Scenario: Peer session state shown
- **WHEN** a peer's Noise handshake is incomplete, complete, or has failed
- **THEN** the table shows one of `initial`, `handshake`, `established`, `failed`, or `unknown`

### Requirement: Peer detail drawer shows full metadata
Selecting a peer SHALL open a detail panel showing the full address list, all counters, path history, and a live sparkline of TX/RX rate for the last 60 seconds.

#### Scenario: User opens peer detail
- **WHEN** the user selects a peer row or clicks a detail control
- **THEN** a drawer opens with the peer's virtual IP, name if known, MAC, endpoint, all address candidates, counters, and a 60-second sparkline

### Requirement: Dashboard supports copying peer information
The dashboard SHALL let the user copy a peer's virtual IP and display name to the clipboard.

#### Scenario: Copy peer IP
- **WHEN** the user activates the copy control next to a peer's virtual IP
- **THEN** the virtual IP string is written to the system clipboard and a brief confirmation is shown

### Requirement: Dashboard supports leaving the current network
The dashboard SHALL provide a control to leave the current network with an explicit confirmation step before the action is sent to the daemon.

#### Scenario: Leave network confirmed
- **WHEN** the user chooses to leave the network and confirms the dialog
- **THEN** the desktop app invokes `leave_network`, the daemon leaves the network, and the dashboard returns to the empty state

#### Scenario: Leave network cancelled
- **WHEN** the user chooses to leave the network but cancels the confirmation
- **THEN** no command is sent and the dashboard remains unchanged

### Requirement: Dashboard supports reconnecting a peer
The dashboard SHALL provide a per-peer control to trigger a re-punch (hole-punch retry) for that peer.

#### Scenario: Re-punch peer
- **WHEN** the user activates the reconnect control for a peer
- **THEN** the desktop app invokes `repunch_peer` with the peer's node ID and the daemon starts a new hole-punch attempt

### Requirement: Dashboard supports toggling gateway mode
The dashboard SHALL show the current gateway mode state and provide a control to enable or disable it when the underlying platform supports runtime toggling.

#### Scenario: Toggle gateway on
- **WHEN** gateway mode is disabled and the user enables it
- **THEN** the desktop app invokes `set_gateway` with `enabled: true` and the daemon activates gateway mode

#### Scenario: Toggle gateway unavailable
- **WHEN** the daemon reports that gateway mode cannot be toggled at runtime
- **THEN** the toggle is disabled or hidden and a tooltip explains that the feature is unavailable

### Requirement: Dashboard supports pausing live updates
The dashboard SHALL provide a control to pause and resume automatic snapshot polling without closing the app.

#### Scenario: Pause polling
- **WHEN** the user activates the pause control
- **THEN** automatic snapshot polling stops, the UI shows a paused indicator, and manual refresh remains available

#### Scenario: Resume polling
- **WHEN** the user activates the resume control
- **THEN** automatic snapshot polling resumes at the current focus-aware rate
