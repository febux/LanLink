# Spec Delta: testing/e2e

## Purpose

The E2E test harness provides repeatable, scenario-based proof that two real LanLink peers, a coordinator, and a relay server can establish a network and carry traffic over direct, relay, and gateway paths on Linux.

## ADDED Requirements

### Requirement: E2E scenarios run real peers on Linux

The E2E harness SHALL spawn two or more real `lanlinkd` processes inside network namespaces, each with its own TAP device, connected to a coordinator and a relay server running on the host.

#### Scenario: Two peers join the same network
- **WHEN** the scenario runner creates a network namespace topology, starts the coordinator and relayd, and launches two `lanlinkd` peers with valid credentials
- **THEN** both peers report joined state and receive unique virtual IPs within the assigned subnet

### Requirement: Scenarios verify the direct UDP path

The E2E harness SHALL verify that two peers on the same virtual network can exchange traffic over a direct UDP path without using the relay.

#### Scenario: Direct UDP path between peers
- **WHEN** two joined peers are on a topology that allows UDP between them
- **THEN** the daemon status snapshots for each peer classify the other peer as `Direct`

### Requirement: Scenarios verify the relay fallback path

The E2E harness SHALL be able to block direct UDP between peers and confirm that encrypted traffic still flows through the relay server.

#### Scenario: Relay fallback when direct UDP is blocked
- **WHEN** the scenario runner installs namespace rules that drop UDP between the peers but leaves the relay path open
- **THEN** the peers remain reachable and their status snapshots classify the path as `Relay`

### Requirement: Scenarios verify MAC learning and switching

The E2E harness SHALL verify that Ethernet frames are learned and switched correctly between virtual adapters by observing ARP resolution and ICMP echo success.

#### Scenario: ARP and ICMP between virtual adapters
- **WHEN** one peer sends an ARP request for the other peer's virtual IP and then sends ICMP echo requests
- **THEN** the ARP is resolved, the ICMP echo requests receive replies, and no unicast frames are flooded after the MAC is learned

### Requirement: Scenarios verify PMTU fragmentation

The E2E harness SHALL verify that large Ethernet frames are fragmented and reassembled end-to-end, so the virtual path MTU is usable.

#### Scenario: Large ping through the virtual tunnel
- **WHEN** one peer sends ICMP echo requests with a payload larger than the inner chunk payload size
- **THEN** the echo requests are delivered and replied to without corruption

### Requirement: Scenarios verify gateway mode

The E2E harness SHALL verify that a peer acting as a gateway forwards traffic between the virtual network and a host in an attached physical-LAN namespace.

#### Scenario: Gateway forwards to a physical-LAN host
- **WHEN** one peer is configured as a gateway sharing a subnet, a third namespace hosts a machine on that subnet, and the other peer routes the shared subnet through the gateway
- **THEN** ICMP echo requests from the non-gateway peer to the physical-LAN host receive replies

### Requirement: Scenarios verify daemon status snapshots

The E2E harness SHALL query each peer's local status socket and verify that the returned NDJSON snapshot contains the expected network, peer list, and path classification.

#### Scenario: Status snapshot reflects joined peers
- **WHEN** a scenario connects to a peer's status socket and requests a snapshot
- **THEN** the snapshot lists the peer's own assigned IP, the other peer, and a non-`Waiting` path for the other peer

### Requirement: Scenarios verify the firewall profile

The E2E harness SHALL verify that enabling the public firewall profile installs ingress rules on the peer's TAP interface and that those rules block the configured ports.

#### Scenario: Firewall profile blocks an inbound port
- **WHEN** a peer joins a public network and the firewall profile is applied to its TAP interface
- **THEN** a connection attempt to a blocked TCP port from another peer is dropped, while ICMP echo requests are still allowed if the profile permits ping

### Requirement: E2E scenarios are opt-in and root-only

The E2E harness SHALL require root to create network namespaces, bridges, veth pairs, and TAP devices, and scenarios SHALL NOT run during a normal `cargo test` invocation.

#### Scenario: E2E targets require root and are skipped by default
- **WHEN** a developer runs `cargo test` without the `e2e` feature or `make e2e` target
- **THEN** no E2E scenario runs and no root privileges are required

### Requirement: E2E output is actionable

Each E2E scenario SHALL return a distinct non-zero exit code on failure and print concise diagnostics that identify which assertion failed and where to find logs.

#### Scenario: Failed scenario reports its exit code
- **WHEN** an E2E scenario fails because a peer did not join within the timeout
- **THEN** the runner exits with the scenario-specific failure code and prints the last lines of the relevant daemon and coordinator logs
