# Tasks

## 1. Release pipeline and script skeleton

- [ ] 1.1 Create `scripts/install.sh` with `set -euo pipefail`, `--help`, `--dry-run`, `--yes`, `--uninstall`, and `--role` argument parsing; verify `bash -n scripts/install.sh` passes and `--help` prints usage
- [ ] 1.2 Update `dist/publish-release.sh` to copy `scripts/install.sh` to the release as `lanlink-install.sh` and generate a `SHA256SUMS` file covering all release artifacts; verify the dry-run release build includes both files
- [ ] 1.3 Update `Makefile` with a `publish-install-script` target that copies `scripts/install.sh` into `dist/` for local testing; verify `make publish-install-script` creates the expected file

## 2. OS/arch detection and package install

- [ ] 2.1 Implement Linux distro detection (`apt-get` → deb, `dnf`/`yum`/`rpm` → rpm, `pacman` → aur, otherwise tarball) and architecture mapping (`x86_64`, `aarch64`, `armv7`); verify each branch by running the detection function on the corresponding distro container or with mocked `PATH`
- [ ] 2.2 Implement GitHub release asset download for `.deb`, `.rpm`, tarball, and `PKGBUILD` paths using `LANLINK_REPO`, `LANLINK_VERSION`, and `LANLINK_CHANNEL`; verify with `--dry-run` that the constructed URLs match the existing release layout under `dist/*/out/`
- [ ] 2.3 Implement checksum verification using the published `SHA256SUMS`; verify that a mismatched checksum aborts and a matching checksum proceeds

## 3. Coordinator role

- [ ] 3.1 Implement coordinator installation branch: install package, create `LANLINK_DATA_DIR` (default `/var/lib/lanlink`), set `lanlink:lanlink` ownership, and write `lanlink-coordinator.service` from `dist/systemd/lanlink-coordinator.service`; verify `systemctl status lanlink-coordinator` reports running after the wizard completes
- [ ] 3.2 Implement optional `lanlink-relayd` enable flag (`--with-relay` / `LANLINK_WITH_RELAY=1`): install `lanlink-relayd`, configure listen address, start `lanlink-relayd.service`, and pass `--relay-server` to the coordinator unit; verify both units are running and `lanlinkctl coord create` succeeds against the coordinator URL
- [ ] 3.3 Implement SQLite initialization idempotency and firewall port opening (HTTP/relay ports via firewalld/ufw/iptables); verify the database file exists, the firewall rule is present, and re-runs do not duplicate rules
- [ ] 3.4 Print peer join instructions at the end of coordinator setup, including `LANLINK_COORD_URL` and an example `lanlinkd service up <network>` command; verify the printed output appears in a test run

## 4. Peer role

- [ ] 4.1 Implement peer package install with optional GUI front-ends controlled by `LANLINK_GUI` (`none`, `tui`, `web`, `desktop`, `all`); verify the correct binaries are present for each GUI selector by checking `which lanlink-tui|lanlink-web|lanlink-desktop|lanlink-gui`
- [ ] 4.2 Implement interactive peer prompts for coordinator URL, network name, and password, with defaults from env vars; verify that env-only provisioning with `--yes` skips all prompts
- [ ] 4.3 Implement peer config writing: create `/etc/lanlink/<name>.toml` using `lanlinkd.toml` format and a `password_file` at `/etc/lanlink/<name>.secret`; verify `lanlinkd run --config /etc/lanlink/<name>.toml --dry-run` parses the generated config successfully
- [ ] 4.4 Implement peer service start via `lanlinkd service up <name>` (or equivalent env-driven path) and status verification through the daemon status socket (`/run/lanlink/lanlinkd-<id>.sock` or `/tmp` fallback); verify `lanlinkctl status` or the status socket returns an online snapshot

## 5. Relay role

- [ ] 5.1 Implement relay installation branch: install package, write `lanlink-relayd.service` from `dist/systemd/lanlink-relayd.service`, configure UDP listen port via `LANLINK_LISTEN`, and start the unit; verify `systemctl status lanlink-relayd` is active
- [ ] 5.2 Implement relay health check: send a `lanlink_relay::encode_bind` probe or connect to the UDP port and confirm the socket is listening; verify the health check passes within 5 seconds of unit start

## 6. Non-interactive, idempotent, and uninstall modes

- [ ] 6.1 Implement `--yes` non-interactive mode and env variable precedence (`LANLINK_ROLE`, `LANLINK_COORD_URL`, `LANLINK_NETWORK`, `LANLINK_PASSWORD_FILE`, etc.); verify a container provisioned only with env vars and `--yes` reaches a running state without `stdin`
- [ ] 6.2 Implement `--dry-run` that prints every command and file write without executing network downloads or system changes; verify no files are written and no systemd commands are invoked when `--dry-run` is used
- [ ] 6.3 Implement idempotent re-runs: detect installed version, upgrade package only when newer, preserve existing `/etc/lanlink/<name>.toml` and secrets unless `--reconfigure` is passed, and restart only running units; verify a second run completes without error and does not overwrite operator-edited config
- [ ] 6.4 Implement `--uninstall`: stop/disable role-specific units, remove role-specific config files, and leave the package installed only when `--purge` is set; verify that after uninstall the relevant `systemctl is-active` checks fail and config files are removed

## 7. Security and documentation

- [ ] 7.1 Audit the script for `eval` or `source` of remote content and ensure downloaded helpers are treated as static data; verify with `grep -E '\beval\b|\bsource\b' scripts/install.sh` returning no matches on remote content paths
- [ ] 7.2 Add shellcheck-clean syntax (`shellcheck scripts/install.sh` passes, or document intentionally ignored rules); verify by running `shellcheck` in CI
- [ ] 7.3 Update `docs/install.md` and `dist/README.md` with the new one-line wizard command, env variables, and role examples; verify the documented commands produce the expected `--help` output when pasted
- [ ] 7.4 Add container/matrix tests for coordinator, peer, and relay roles on Debian, Fedora, and Arch (tarball fallback); verify `openspec validate` is green and all scenarios map to at least one test or manual verification step

## Workflow follow-up

- Archive the change after the implementation passes review and matrix tests.
- Verify the published release includes `lanlink-install.sh` and `SHA256SUMS`.
