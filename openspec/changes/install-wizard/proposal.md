# Proposal

## Why

LanLink currently ships a Linux install script (`dist/install.sh`) that only picks a package format and GUI variant. Setting up a coordinator, joining a peer to a network, or deploying a relay still requires manual commands, config files, and systemd interactions. This change turns the one-line installer into an interactive role wizard (coordinator / peer / relay) so a single `curl | bash` command can produce a working, running node.

## What Changes

- Replace `dist/install.sh` with `lanlink-install.sh` (published in releases and referenced by the same curl command), an interactive installer that asks for a role and then completes the full setup.
- Add a non-interactive/env-driven mode for scripting and provisioning (`LANLINK_ROLE`, `LANLINK_COORD_URL`, `LANLINK_NETWORK`, `LANLINK_PASSWORD_FILE`, `--yes`, `--dry-run`).
- Make the installer idempotent: repeated runs upgrade/reconfigure packages and units instead of failing, and an `--uninstall` path removes installed artifacts.
- Restrict v1 to Linux (Debian, Fedora/RHEL, Arch/AUR, generic tarball). Windows and macOS installers stay separate and unchanged.
- Keep the curl|bash execution model safe: `set -euo pipefail`, no `eval` of remote content, and artifact checksum verification wherever the release provides checksums.

## Capabilities

### New Capabilities

- `install-wizard`: A single Linux entry-point installer that detects OS/package manager, installs the right release artifact, asks (or env-configures) the desired role, writes configuration, installs/enables systemd units, and verifies the result.

### Modified Capabilities

- None.

## Impact

- `dist/install.sh` is rewritten into the new wizard and renamed in release artifacts to `lanlink-install.sh`.
- `scripts/install.sh` is created as the source-of-truth copy for local testing and release inclusion.
- Release publishing (`dist/publish-release.sh`, `Makefile`) gains a new artifact (`lanlink-install.sh`) and a checksum file.
- Existing binaries (`lanlinkd`, `lanlinkctl`, `lanlink-coordinator`, `lanlink-relayd`) and systemd units are consumed by the wizard as-is; no breaking CLI changes required.
