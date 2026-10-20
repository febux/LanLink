# Spec Delta

## Purpose

Provides deterministic, opt-in anonymization of personally identifiable information in client logs and daemon status snapshots so operators can safely collect diagnostics and share status views without leaking IP addresses, MAC addresses, keys, hostnames, or network names.

## ADDED Requirements

### Requirement: Anonymize IPv4 addresses deterministically
The anonymization module SHALL accept an IPv4 address and return a masked address that preserves the configured prefix bits while replacing the remaining bits with a deterministic, salted hash of the original address.

#### Scenario: IPv4 masking preserves prefix
- **WHEN** the mask function receives `198.51.100.123`
- **THEN** the output keeps the first 24 bits (`198.51.100`) and replaces the host bits with a stable hash-derived value

#### Scenario: Invalid IPv4 passes through
- **WHEN** the mask function receives a string that is not a valid IPv4 address
- **THEN** the output equals the input unchanged

### Requirement: Anonymize IPv6 addresses deterministically
The anonymization module SHALL accept an IPv6 address and return a masked address that preserves the configured prefix bits while replacing the remaining bits with a deterministic, salted hash of the original address.

#### Scenario: IPv6 masking preserves prefix
- **WHEN** the mask function receives `2001:db8::abcd`
- **THEN** the output keeps the first 64 bits and replaces the interface identifier with a stable hash-derived value

#### Scenario: Localhost passes through or is masked consistently
- **WHEN** the mask function receives `::1`
- **THEN** the output is either `::1` or a deterministically masked `::1` that remains stable across calls

### Requirement: Anonymize MAC addresses deterministically
The anonymization module SHALL accept a MAC address and return a value that preserves the OUI while replacing the device-specific bytes with a deterministic, salted hash.

#### Scenario: MAC masking preserves OUI
- **WHEN** the mask function receives `aa:bb:cc:dd:ee:ff`
- **THEN** the output keeps `aa:bb:cc` and replaces the last three octets with a stable hash-derived value

### Requirement: Anonymize node/device IDs and public keys deterministically
The anonymization module SHALL accept node IDs and public-key byte strings and return a short, deterministic, salted hash that is stable across log lines and status snapshots.

#### Scenario: Same node ID yields same anonymized token
- **WHEN** the mask function receives the same 8-byte node ID twice
- **THEN** both outputs are identical short alphanumeric tokens

#### Scenario: Different node IDs produce different tokens
- **WHEN** the mask function receives two distinct node IDs
- **THEN** the probability of a hash collision is no higher than the underlying hash function

### Requirement: Anonymize network names deterministically
The anonymization module SHALL accept a network name and return a deterministic, salted hash token that is stable across log lines for the same network.

#### Scenario: Network name token is stable
- **WHEN** the mask function receives `"gaming-night"`
- **THEN** repeated calls return the same token, and different names return different tokens

### Requirement: Passwords are never logged
The system SHALL ensure that network passwords, password files, and password-derived key material are never emitted in log messages or status snapshots.

#### Scenario: Join request password is not logged
- **WHEN** a join request containing a password is processed
- **THEN** no log line or status field contains the literal password or any reversible derivation of it

### Requirement: Tracing layer applies anonymization when enabled
`lanlinkd` SHALL provide a tracing layer/formatter that applies the anonymization module to every log event's formatted message and fields when `[log] anonymize = true`.

#### Scenario: Anonymized log output
- **WHEN** `[log] anonymize` is `true` and a log event contains `198.51.100.7:5000`
- **THEN** the emitted log line shows the masked IP address, not the original

#### Scenario: Default log output is unchanged
- **WHEN** `[log] anonymize` is `false` (the default)
- **THEN** log lines are emitted without any PII masking

### Requirement: Status snapshot can be returned anonymized
`lanlinkd` SHALL support a privacy flag on its NDJSON status request that causes the returned snapshot to anonymize all peer and daemon addresses, node IDs, and network names before serialization.

#### Scenario: Privacy-enabled status snapshot
- **WHEN** a frontend requests a snapshot with privacy enabled
- **THEN** peer endpoints, virtual IPs, assigned IPs, node IDs, relay addresses, and shared subnets are masked, but peer count, path kinds, byte counters, and uptime remain accurate

### Requirement: Bug report forces anonymization
`lanlinkctl bugreport` SHALL collect recent daemon logs and the active configuration into a bundle, forcing anonymization regardless of the daemon's `[log] anonymize` setting.

#### Scenario: Bug report bundle contains no PII
- **WHEN** `lanlinkctl bugreport` is run against a daemon whose logs contain peer IP addresses and node IDs
- **THEN** the produced bundle contains only masked values and no passwords or password file paths

### Requirement: Anonymization functions have unit tests
Every mask function in the anonymization module SHALL have unit tests covering valid input, invalid input, edge cases, and deterministic output.

#### Scenario: IPv4 edge cases
- **WHEN** tests exercise `0.0.0.0`, `255.255.255.255`, and a malformed string
- **THEN** all cases are accepted without panics and invalid input is returned unchanged

#### Scenario: IPv6 edge cases
- **WHEN** tests exercise `::1`, `fe80::1`, a full `2001:db8:85a3::8a2e:370:7334`, and a malformed string
- **THEN** all cases are accepted without panics and invalid input is returned unchanged

#### Scenario: MAC edge cases
- **WHEN** tests exercise lowercase, uppercase, hyphen-separated, and malformed MAC strings
- **THEN** valid formats are masked and invalid formats are returned unchanged
