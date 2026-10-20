# Spec Delta: testing/integration

## Purpose

The integration test suite exercises the public APIs of multiple LanLink crates together, giving fast, rootless confidence that the crypto, switch, protocol, coordinator, and punch crates compose correctly.

## ADDED Requirements

### Requirement: Integration tests exercise crypto session handshakes

The integration suite SHALL verify that two `lanlink_crypto` identities can complete a Noise handshake and exchange authenticated, encrypted messages.

#### Scenario: Two nodes establish a Noise session
- **WHEN** node A initiates a handshake with node B using valid Ed25519 identities and node B processes the handshake message
- **THEN** both nodes derive the same transport keys and can encrypt and decrypt messages in both directions

### Requirement: Integration tests exercise switch framing round-trips

The integration suite SHALL verify that a `lanlink_switch::Switch` can seal a virtual Ethernet frame into LanLink packets and that a second switch can open them and recover the original frame.

#### Scenario: Ethernet frame survives a switch round-trip
- **WHEN** one switch seals a unicast Ethernet frame addressed to a known peer and the peer's switch receives the resulting packet
- **THEN** the receiving switch returns the original frame, learns the source MAC, and the MAC table maps that MAC to the sender

### Requirement: Integration tests exercise the coordinator API

The integration suite SHALL verify that the coordinator's Axum application, backed by a temporary SQLite pool, exposes the documented network and member endpoints.

#### Scenario: Create, join, and list members through the API
- **WHEN** a test creates a network, two members join with valid credentials, and the members endpoint is queried
- **THEN** the response contains both members with their assigned virtual IPs and gateway flags

### Requirement: Integration tests exercise punch candidate exchange

The integration suite SHALL verify that two `lanlink_punch::Puncher` instances can discover and exchange candidates via the coordinator, or against a local signal instance, and verify a bidirectional path.

#### Scenario: Puncher verifies a local loopback path
- **WHEN** two punchers on the same machine gather host candidates, advertise them through a coordinator, and send authenticated PUNCH_PROBE/PUNCH_REPLY packets
- **THEN** the initiating puncher reports a verified path to the peer at the expected address

### Requirement: Integration tests run without root or network namespaces

The integration suite SHALL run with ordinary user privileges, binding only to loopback addresses, and SHALL be included in the standard `cargo test --workspace` invocation.

#### Scenario: Standard workspace test invocation passes
- **WHEN** a developer runs `cargo test --workspace` on a machine with no special privileges
- **THEN** all integration tests in `integration_tests/` pass without creating TAP devices or network namespaces
