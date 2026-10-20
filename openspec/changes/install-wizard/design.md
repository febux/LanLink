# Design

## Context

LanLink already builds release artifacts for Debian, Fedora/RHEL, Arch (AUR), and a portable Linux tarball under `dist/`. The existing `dist/install.sh` downloads and installs one of those artifacts based on `--pkg` or auto-detection, but it never configures a role, writes network config, or starts services. The new wizard reuses all of those artifacts and the existing CLI flags of `lanlinkd`, `lanlinkctl`, `lanlink-coordinator`, and `lanlink-relayd`. See `proposal.md` for motivation and `specs/install-wizard/spec.md` for the required behavior.

## Goals / Non-Goals

**Goals:**
- Provide a single `curl | bash` entry point that can install any LanLink role.
- Use the existing release artifacts and systemd units without modifying their content.
- Support both interactive wizard and scripted (env + `--yes`) provisioning.
- Make repeated runs safe and useful for upgrades/reconfiguration.
- Keep the script safe to pipe from the network.

**Non-Goals:**
- Windows and macOS are out of scope for v1; those keep their existing installers.
- The wizard does not implement a custom package manager, repository, or signing infrastructure beyond what the GitHub release already provides.
- It does not change the daemon/coordinator/relay wire protocols or systemd unit semantics.

## Decisions

### 1. Source file: `scripts/install.sh`, release name: `lanlink-install.sh`

`scripts/install.sh` is the tracked source copy. The release pipeline copies it to the release root as `lanlink-install.sh` so the public one-liner stays short and stable. This keeps the file under normal source control while giving the published URL a clear name.

*Alternative considered:* keep it as `dist/install.sh` and overwrite the existing file. Rejected because the existing `dist/install.sh` is already referenced by older docs; renaming in releases avoids breaking existing deep links and makes the new behavior obvious.

### 2. Role-driven branch after package install

The script always performs the same package-detection/install phase first, then branches by `LANLINK_ROLE` or interactive choice. This avoids duplicating OS-specific download logic across roles and lets `--uninstall` and `--dry-run` work uniformly.

### 3. Reuse `lanlinkd service up` for peer setup

For the peer role the wizard writes a password file and then invokes `lanlinkd service up <name>` (or its env equivalents) rather than reimplementing systemd unit installation. `lanlinkd service up` already resolves/creates the network, writes `/etc/lanlink/<name>.toml`, installs the template unit, and starts the instance.

*Alternative considered:* write the unit and config directly in bash. Rejected because it duplicates logic that already exists and is tested in `crates/lanlinkd/src/service.rs`.

### 4. Coordinator and relay use shipped units

The coordinator role copies `dist/systemd/lanlink-coordinator.service` and `dist/systemd/lanlink-relayd.service` into `/etc/systemd/system/` (or `/lib/systemd/system/` when run with root privileges) and starts them. Data directories are created with the same ownership model as the `.deb`/`.rpm` packages (`lanlink` user, `root:lanlink` group where applicable).

### 5. Configuration via env vars and CLI flags

Interactive prompts use plain `read -r` with defaults taken from env vars. The same env vars drive non-interactive mode. Recognized variables:

- `LANLINK_ROLE` — `coordinator`, `peer`, `relay`
- `LANLINK_COORD_URL` — coordinator base URL (peer/coordinator)
- `LANLINK_LISTEN` — coordinator/relay listen address (default `0.0.0.0:8080` / `0.0.0.0:3478`)
- `LANLINK_NETWORK` — network name for peer role
- `LANLINK_PASSWORD` / `LANLINK_PASSWORD_FILE` — network password
- `LANLINK_DATA_DIR` — coordinator SQLite directory (default `/var/lib/lanlink`)
- `LANLINK_GUI` — `none`, `tui`, `web`, `desktop`, `all` (peer only)
- `LANLINK_PKG` — override package type (`deb`, `rpm`, `aur`, `tarball`)
- `LANLINK_VERSION` / `LANLINK_CHANNEL` — exact version or release channel
- `LANLINK_INSTALL_PREFIX` — default `/usr/local` for tarball installs

CLI flags mirror the env vars (`--role`, `--coord-url`, `--network`, `--password-file`, `--yes`, `--dry-run`, `--uninstall`, etc.). Env vars win when both are present, to make CI provisioning easier.

### 6. Idempotency strategy

- Package install: use the native package manager's idempotent install command (`dpkg -i` + `apt-get install -f`, `dnf install -y`, `makepkg -si` for AUR, or overwrite tarball binaries).
- Systemd units: `systemctl enable --now` is idempotent; `daemon-reload` is run after unit changes.
- Config files: write only when missing or when a `--reconfigure` flag is passed; otherwise preserve operator edits. Secrets are never overwritten if the password file exists and `--yes` is not set.
- Upgrade path: when the installed version differs from the requested release, the script installs the new package and restarts only running units.

### 7. Checksum verification

The script downloads the release's `SHA256SUMS` file when available. Each artifact is checked before it is installed. If `SHA256SUMS` is missing, the script warns and continues; if a checksum is present and mismatches, the script aborts. No artifact is executed directly from the download directory.

### 8. `curl | bash` safety

- `set -euo pipefail` at the top.
- No `eval` of downloaded content.
- Remote data is either a downloaded file that is then handed to the package manager, or static text parsed with `grep`/`sed`.
- All user-visible prompts are written to stderr so piping stdout is safe.

## Risks / Trade-offs

- **AUR builds are slow and interactive** → The script uses `makepkg -si` and relies on the user having `base-devel`; for headless provisioning the `--pkg tarball` fallback is recommended.
- **Checksum availability depends on the release pipeline** → If `dist/publish-release.sh` is not updated to produce `SHA256SUMS`, the verification step will warn-only. This is acceptable for v1 but should be tightened later.
- **Root requirements differ by package type** → `.deb`/`.rpm` require root; tarball can install to a user-writable prefix. The script checks `id -u` and uses `sudo` only when needed.
- **Firewall automation is distribution-specific** → The script tries `firewalld`, `ufw`, and `iptables`/`nftables` in that order, but may fail silently on minimal systems. A warning is emitted and documented.
- **Re-running against a different role on the same host** could leave stale units running → The script stops/disables units from other roles before enabling the new role when `--role` changes and `--yes` is set; otherwise it warns and asks.

## Migration Plan

1. Implement `scripts/install.sh` and update `dist/publish-release.sh` to publish it as `lanlink-install.sh` alongside `SHA256SUMS`.
2. Update `docs/install.md` and `dist/README.md` to reference the new wizard command and flags.
3. Update `Makefile` to add a `publish-install-script` helper if useful.
4. Run the wizard in a container matrix (Debian, Fedora, Arch) for each role with `--dry-run` and real installs.
5. Archive this change once the script is verified.

## Open Questions

- None for v1; Windows/macOS wizard integration is intentionally deferred.
