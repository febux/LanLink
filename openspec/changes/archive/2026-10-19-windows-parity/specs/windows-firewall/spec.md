## ADDED Requirements

### Requirement: Windows inbound firewall profile

The daemon SHALL apply the public/gaming inbound firewall profile on Windows
using Windows Filtering Platform rules scoped to the virtual adapter, with the
same blocked-port semantics as the Linux nftables implementation (drop inbound
TCP 139/445/3389/22 and UDP 137/138 on the virtual interface, allow ICMPv4
echo requests).

#### Scenario: Public network joins get block rules
- **WHEN** a node joins a network marked public/gaming on Windows
- **THEN** the daemon installs firewall rules that block inbound TCP 139, 445, 3389 and 22 and UDP 137 and 138 on the virtual adapter
- **AND** inbound ICMPv4 echo requests on the adapter are allowed

#### Scenario: Idempotent re-application and clean teardown
- **WHEN** the firewall profile is applied twice, or the daemon leaves the network or stops
- **THEN** no duplicate rules remain, and all daemon-installed rules are removed

### Requirement: Windows status transport

The daemon SHALL expose its status NDJSON stream on Windows via a TCP loopback
listener on port 9000 + network id, so `lanlink-tui`, `lanlink-web`, and
`lanlink-desktop` read daemon status identically across platforms.

#### Scenario: Frontends read status on Windows
- **WHEN** a frontend connects to 127.0.0.1:(9000 + network id) on Windows
- **THEN** it receives the same NDJSON status events as the Linux Unix-socket transport
