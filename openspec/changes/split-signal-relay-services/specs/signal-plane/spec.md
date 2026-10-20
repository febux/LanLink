# Spec Delta

## Purpose

The signal plane provides a lightweight, standalone service where LanLink peers
publish their presence and NAT traversal candidates and discover other peers
in the same network, separate from the heavy coordinator control plane.

## ADDED Requirements

### Requirement: Signal service accepts peer registrations per network
The signal service SHALL maintain an in-memory registry of online peers for
each network, keyed by network id, and accept authenticated registrations that
include the peer's node id, public key, virtual IP, gateway role, and current
candidate addresses.

#### Scenario: Peer registers after joining through coordinator
- **WHEN** a peer that has already joined a network via the coordinator POSTs
  a registration to `/networks/{network_id}/peers` with valid credentials and
  candidates
- **THEN** the signal service stores the peer in that network's registry and
  returns the current list of other peers in the network

#### Scenario: Duplicate registration updates candidates
- **WHEN** a peer that is already registered POSTs a new registration with
  changed candidates
- **THEN** the signal service updates the stored peer record, refreshes its
  TTL, and returns the current peer list

### Requirement: Signal service evicts stale peers
The signal service SHALL remove a peer registration if it has not received a
refresh or keepalive within a configurable TTL.

#### Scenario: Peer disappears without refresh
- **WHEN** a peer registers and then sends no refresh for longer than the TTL
- **THEN** the signal service removes that peer from the network registry and
  stops including it in peer lists

### Requirement: Signal service returns peer lists to authenticated members
The signal service SHALL allow any peer that has successfully joined a network
via the coordinator to fetch the current list of peers and their candidates in
that network.

#### Scenario: Registered peer refreshes peer list
- **WHEN** a registered peer GETs `/networks/{network_id}/peers` with valid
  credentials
- **THEN** the signal service returns the current peer list excluding the
  requesting peer

#### Scenario: Unauthenticated peer is rejected
- **WHEN** a request to a signal endpoint provides invalid or missing network
  credentials
- **THEN** the signal service returns HTTP 401 or 403 and no peer data

### Requirement: Signal service advertises relay server URLs
The signal service SHALL learn relay server URLs from the coordinator at
network creation or join time and include them in peer-list responses so that
peers know which relays to bind to.

#### Scenario: Peer list includes relay URLs
- **WHEN** a peer fetches the peer list for a network that has relay servers
  configured on the coordinator
- **THEN** the response includes the UDP and TCP relay server addresses for
  that network

### Requirement: Signal service remains stateless across databases
The signal service SHALL NOT require a persistent database; all peer state MUST
be kept in memory and may be rebuilt from peer registrations after a restart.

#### Scenario: Restart loses no critical persisted state
- **WHEN** the signal service restarts
- **THEN** it resumes normal operation once peers re-register; no database
  migration or recovery is required
