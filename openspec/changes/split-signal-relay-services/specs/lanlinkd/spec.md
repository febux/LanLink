# Spec Delta

## Purpose

`lanlinkd` is the LanLink peer daemon. It joins networks through the
persistent coordinator control plane, then discovers and refreshes peers
through the lightweight signal plane, falling back gracefully when only legacy
coordinator endpoints are available.

## ADDED Requirements

### Requirement: Daemon accepts a signal service URL in config and CLI
`lanlinkd` SHALL accept a `signal` URL in its TOML config, as a `--signal`
CLI override for `lanlinkd run`, and as a `--signal` override for
`lanlinkd up`. If the signal URL is not provided, it SHALL default to the
configured coordinator URL for backward compatibility during the migration
window.

#### Scenario: Config file supplies signal URL
- **WHEN** the TOML config contains `network.signal = "https://signal.example.com"`
- **THEN** the daemon uses that URL for candidate exchange and peer discovery

#### Scenario: CLI overrides signal URL
- **WHEN** `lanlinkd run --signal https://signal.example.com` is invoked
- **THEN** the daemon uses the CLI value regardless of the config file value

### Requirement: Daemon registers with signal service after coordinator join
After a successful coordinator join, `lanlinkd` SHALL POST its initial
presence and candidates to the signal service and receive the current peer list
in return.

#### Scenario: Successful join uses signal for first peer list
- **WHEN** `lanlinkd` completes a coordinator join and has a signal URL
- **THEN** it registers with the signal service and uses the returned peer list
  to populate the switch and start hole punching

### Requirement: Daemon refreshes candidates through signal service
`lanlinkd` SHALL periodically post presence/candidate updates to the signal
service and receive updated peer lists, using the same refresh interval it
previously used against the coordinator.

#### Scenario: Periodic refresh targets signal service
- **WHEN** the refresh interval fires
- **THEN** `lanlinkd` posts its candidates to the signal service, not the
  coordinator, and syncs the returned peers into the switch

### Requirement: Daemon falls back to coordinator candidate endpoints during migration
When configured with only a coordinator URL and no signal URL, or when the
signal service returns 404/410, `lanlinkd` SHALL fall back to the legacy
`POST /networks/{network_id}/candidates` coordinator endpoint until that
endpoint is removed.

#### Scenario: Old config without signal URL still works
- **WHEN** `lanlinkd` is started with a config that has no `signal` setting
- **THEN** it uses the coordinator URL for both join and candidate refresh
  and continues to operate as before

### Requirement: Daemon continues local status API unchanged
The local status socket served by `lanlinkd` SHALL continue to expose the same
snapshot shape, including relay address, peer list, and path classification,
regardless of whether peers were discovered through the signal service or the
legacy coordinator endpoint.

#### Scenario: Status snapshot works after signal migration
- **WHEN** a frontend requests a status snapshot from `lanlinkd`
- **THEN** the response includes the joined network, virtual IP, peer list, and
  per-peer paths with the same fields as before the signal split
