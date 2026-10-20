# Spec Delta

## Purpose

Provide a single Linux entry-point installer that detects the platform, picks the right release artifact, and walks the operator through one of three roles (coordinator, peer, or relay) until the node is installed, configured, and running.

## ADDED Requirements

### Requirement: Role selection and OS/arch/package detection

The wizard SHALL detect the Linux distribution and CPU architecture, choose the matching release artifact from `dist/` (`.deb`, `.rpm`, AUR `PKGBUILD`, or portable tarball), and ask the operator for a role (`coordinator`, `peer`, or `relay`) when no `LANLINK_ROLE` environment variable is set.

#### Scenario: Debian/Ubuntu host
- **WHEN** the wizard runs on a system with `apt-get` and `dpkg`
- **THEN** it installs the `.deb` release artifact for the detected architecture

#### Scenario: Fedora/RHEL host
- **WHEN** the wizard runs on a system with `dnf`, `yum`, or `rpm`
- **THEN** it installs the `.rpm` release artifact for the detected architecture

#### Scenario: Arch Linux host
- **WHEN** the wizard runs on a system with `pacman`
- **THEN** it builds and installs from the AUR `PKGBUILD` in `dist/aur/`

#### Scenario: Unknown distro or missing package manager
- **WHEN** no supported package manager is found
- **THEN** it falls back to the portable tarball from `dist/tarball/`

### Requirement: Coordinator role setup

When the role is `coordinator`, the wizard SHALL install `lanlink-coordinator`, optionally install and enable `lanlink-relayd`, create the coordinator data directory, initialize the SQLite database, configure the listen port and data path, open the required firewall ports, enable and start the `lanlink-coordinator.service`, and print a join instruction for peers.

#### Scenario: Fresh coordinator install
- **WHEN** the operator chooses `coordinator` and no coordinator service is present
- **THEN** the wizard writes `/var/lib/lanlink` ownership for the `lanlink` user, installs the unit, starts it, and prints the peer join command with the coordinator URL

#### Scenario: Coordinator re-run upgrades the package
- **WHEN** the wizard runs again on an existing coordinator
- **THEN** it upgrades the installed package, reloads systemd, and restarts only if the unit was already running

### Requirement: Peer role setup

When the role is `peer`, the wizard SHALL install `lanlinkd` and `lanlinkctl`, optionally install TUI or Web front-ends, prompt for the coordinator URL and network name plus password (or create the network interactively), write `/etc/lanlink/<name>.toml` with `password_file`, install or reuse the `lanlinkd@.service` template, start/enable the instance, and verify that the daemon is reachable through its status socket.

#### Scenario: Peer joins an existing network
- **WHEN** the operator supplies an existing network name and password
- **THEN** `lanlinkctl coord join` resolves the network id, the wizard writes the config and secret, starts `lanlinkd@<name>.service`, and `lanlinkctl status` (or the status socket) reports `online`

#### Scenario: Peer creates a new network interactively
- **WHEN** the operator asks to create a network and supplies a name and password
- **THEN** `lanlinkctl coord create` creates the network, the wizard writes the config, and starts the daemon instance

### Requirement: Relay role setup

When the role is `relay`, the wizard SHALL install `lanlink-relayd`, configure the UDP listen port, enable and start `lanlink-relayd.service`, and verify health by checking that the relay answers a bind/keep-alive probe or reports a listening socket.

#### Scenario: Standalone relay install
- **WHEN** the operator chooses `relay` on a Linux host
- **THEN** the wizard installs the package, starts `lanlink-relayd.service`, and reports the relay listen address as healthy

### Requirement: Non-interactive and dry-run mode

The wizard SHALL support a fully non-interactive mode driven by environment variables (`LANLINK_ROLE`, `LANLINK_COORD_URL`, `LANLINK_NETWORK`, `LANLINK_PASSWORD_FILE`, etc.) and a `--yes` flag, and SHALL support `--dry-run` that prints every planned action without mutating the system.

#### Scenario: Scripted peer provisioning
- **WHEN** the installer is invoked with `LANLINK_ROLE=peer`, `LANLINK_COORD_URL`, `LANLINK_NETWORK`, and `LANLINK_PASSWORD_FILE` set, and `--yes`
- **THEN** it installs the package, writes the config, and starts the service without prompting

#### Scenario: Dry-run coordinator
- **WHEN** the installer is invoked with `--dry-run` and `LANLINK_ROLE=coordinator`
- **THEN** it prints the package URL, config values, and systemd commands but makes no persistent changes

### Requirement: Idempotency and uninstall

The wizard SHALL be idempotent: repeated runs on the same node upgrade or reconfigure packages and units instead of failing. It SHALL also provide an `--uninstall` flag that stops and disables the relevant systemd units and removes the role-specific configuration while preserving the release repository settings.

#### Scenario: Re-run upgrades a peer
- **WHEN** the wizard runs a second time on a peer with the same role and a newer release is available
- **THEN** it upgrades the package, keeps the existing `/etc/lanlink/<name>.toml` and secret, and restarts the daemon instance

#### Scenario: Uninstall a role
- **WHEN** the operator runs the installer with `--uninstall` and the role
- **THEN** it stops and disables the corresponding systemd units and removes role-specific config files after confirmation (or with `--yes`)

### Requirement: Secure curl|bash execution

The install script SHALL start with `set -euo pipefail`, SHALL NOT `eval` content downloaded from the release URL, and SHALL verify release artifact checksums when the GitHub release provides a `SHA256SUMS` file or per-asset checksum.

#### Scenario: Tampered release artifact
- **WHEN** the downloaded artifact checksum does not match the release checksum
- **THEN** the installer aborts before installing the file

#### Scenario: Remote content is never evaluated
- **WHEN** the installer downloads helper data from the release
- **THEN** it is written to disk or parsed as static data; no `eval` or `source` of remote content occurs
