# Tasks

## 1. Windows TCP loopback status transport in `lanlinkd`

- [x] 1.1 Add `#[cfg(unix)]` / `#[cfg(windows)]` split to `crates/lanlinkd/src/status.rs` for listener and connection handling
- [x] 1.2 On Windows, bind a TCP listener on `127.0.0.1:<port>`; default port = `9000 + network_id`, allow override via `status.socket` config (e.g. `status.socket = "127.0.0.1:19501"`)
- [x] 1.3 Keep the same NDJSON request/response protocol so frontends work unchanged
- [x] 1.4 Make `lanlinkd service up` on Windows set `status.socket` in the generated `lanlinkd.toml`
- [x] 1.5 Add a unit test that verifies the TCP status server on Windows (or a cross-platform smoke test) — `status_tcp_round_trip` + `parse_status_addr_variants` (Windows); compile-verified via `cargo xwin check --tests`; Unix socket tests still pass

## 2. Frontend status clients on Windows

- [x] 2.1 Update `crates/lanlink-tui/src/daemon.rs` to use TCP loopback on Windows (auto-discover or use default port)
- [x] 2.2 Update `crates/lanlink-web/src/api.rs` to use TCP loopback on Windows
- [x] 2.3 Update `crates/lanlink-desktop/src/commands.rs` to use TCP loopback on Windows

## 3. Desktop GUI service control

- [x] 3.1 Add Tauri commands for `service_install`, `service_uninstall`, `service_start`, `service_stop`, `service_status`
- [x] 3.2 Elevate via UAC (`runas`) when the desktop app runs non-elevated: probe with `net session`, relaunch `lanlinkd service ...` through PowerShell `Start-Process -Verb RunAs` with output captured to a temp log; pass the network password via `LANLINK_PASSWORD` inside the elevated batch instead of the command line
- [x] 3.3 Tauri command `service_up(name, password, coordinator, public)` — implemented as `service_up` rather than `configure_and_start_network`; it shells out to `lanlinkd service up`, which writes `lanlinkd.toml` + the secret and starts the service (single source of truth for the Windows config layout)
- [x] 3.4 Wire new commands into the Tauri `invoke_handler` in `main.rs`
- [x] 3.5 Update the bundled HTML/JS frontend: Windows-only service panel with create/join-to-start form plus install/start/stop/status buttons and an output log

## 4. Bundle VC++ Redistributable in the MSI

- [x] 4.1 Download `vc_redist.x64.exe` in `dist/windows/build-msi.sh` (accept `VCREDIST_EXE` path override and `VCREDIST_SHA256` pin; `SKIP_VCREDIST=1` opt-out)
- [x] 4.2 Custom action to run the redist silently (`/install /quiet /norestart`) before the service install — must be extension-free (no `WixUtilExtension`), so use a Directory+ExeCommand custom action instead of `WixQuietExec`
- [x] 4.3 Skip the custom action on repair/uninstall (`NOT Installed`) and when `vcruntime140.dll` already exists (AppSearch guard) so exit code 1638 cannot fail the install
- [x] 4.4 Fix MSI property plumbing for ALL build-time toggles: in WiX 4+, `-d Var=1` does NOT set MSI properties (verified via `wix msi decompile`), so `Condition="IncludeTui = 1"` etc. were always false and the MSI never installed `lanlink-desktop.exe`/`lanlink-tui.exe`/`lanlink-web.exe`. Add preprocessor defaults + `<Property Value="$(var.X)">` binding

## 5. Build and verify

- [x] 5.1 Build all Windows binaries with `cargo xwin` (desktop rebuilt with the elevation code; sha256 of the redist staged + pinned-check verified)
- [x] 5.2 Build the MSI on Linux via Docker fallback — `dist/windows/out/lanlink-0.5.2-x64.msi` (~72 MB incl. redist); decompiled to confirm `IncludeTui/Web/Desktop=1`, GUI components, `Vcruntime` component, AppSearch guard and `RunVCRedist` CA before `InstallServices`; offline `SKIP_VCREDIST=1` variant also builds
- [x] 5.3 Verify `Dependencies` no longer reports missing `VCRUNTIME140.dll`
- [x] 5.4 On a Windows VM, install the MSI and confirm `lanlink-desktop.exe` opens
- [x] 5.5 Confirm the desktop app can create/join a network and that `LanLinkDaemon` starts
- [x] 5.6 Confirm `lanlink-tui` and `lanlink-web` can read daemon status on Windows

## 6. Documentation

- [x] 6.1 Update `dist/windows/README.md`: GUI-first workflow (desktop app installs/starts the service, no terminal), bundled VC++ redist, and the shipped default config's `[status]` TCP socket

## 7. First-run status UX hardening (Windows VM feedback: "Daemon status unavailable")

- [x] 7.1 `service_status` must never trigger UAC — SCM `QUERY_STATUS` works for standard users; always run it direct (non-elevated)
- [x] 7.2 Status-port discovery: try the `[status] socket` from `C:\ProgramData\LanLink\lanlinkd.toml` first, then scan `127.0.0.1:9001..9032` (network ids 1..=32); do the same in TUI/Web discover; include the probed targets in the "no daemon status socket found" error
- [x] 7.3 `lanlinkd service status` on Windows: print the exit code when the service is Stopped, so a config/secret failure is visible from the GUI
- [x] 7.4 Desktop UI empty state: auto-diagnose (throttled) — show live service state line (not installed / stopped / running-but-unreachable) with matching guidance; no UAC prompt on load

## 8. Windows VM feedback round 2 (zombie Running service, hidden errors, no coordinator)

- [x] 8.1 Fix zombie service state in `crates/lanlinkd/src/service.rs`: `run_service` reports Running before the daemon thread runs and ignores its result — a daemon that fails instantly (missing config/secret) keeps the service "Running" forever. When the runner fails, unblock the control loop and transition to Stopped with a non-zero exit code
- [x] 8.2 Fix hidden errors in the desktop UI: Tauri rejects command errors as plain strings, so `e.message` is undefined — use `e?.message || String(e)` in every catch (create/join/service forms)
- [x] 8.3 Add built-in coordinator control to the desktop GUI: `coordinator_status` (HTTP probe), `coordinator_start` (spawn bundled `lanlink-coordinator.exe` detached with db under the user's data dir), `coordinator_stop` (kill only if app-launched); UI row in the service panel with status + buttons
- [x] 8.4 Diagnose line uses `daemon_config_exists`: when the service is Running but unreachable and the config is missing, say so and point at "create or join a network" instead of the `[status]` socket hint; surface the exit-code detail from `service status` when stopped

## 9. Windows VM feedback round 3 (ProgramData config/secret creation fails)

- [x] 9.1 Path-explicit I/O errors in `lanlinkd service up` (Windows and Linux): each `create_dir_all`/`write` must say which path failed and why
- [x] 9.2 Desktop app pre-creates `C:\ProgramData\LanLink` with a permissive ACL before invoking `service_up`, so the elevated `lanlinkd` child can write even when the parent ProgramData ACL is restrictive
- [x] 9.3 Same `service_up` call writes a fallback single-file config if `lanlinkd` fails; the GUI can then retry with `service_start`
- [x] 9.4 If the directory/file write still fails, the UI error message must include the exact path and the recommended manual workaround (run `lanlinkd service up` from an elevated PowerShell / check AV/Controlled Folder Access)

## 10. Windows VM feedback round 4 (move daemon config out of ProgramData)

- [x] 10.1 Move `windows_impl::CONFIG_PATH` and the `service up` config dir from `C:\ProgramData\LanLink` to `C:\Program Files\LanLink` (explicit user request: ProgramData is blocked by ACL/environment)
- [x] 10.2 Update the shipped default `lanlinkd.toml` (`password_file` path) and the MSI LiveConfig location to `C:\Program Files\LanLink`
- [x] 10.3 Update desktop `DAEMON_CONFIG`, diagnose strings, and the elevated batch ACL prep to the new path
- [x] 10.4 Update `dist/windows/README.md` with the new config location

## 11. Windows VM feedback round 5 (TOML secret path / batch directory bug)

- [x] 11.1 Fix generated `password_file` in `lanlinkd service up` (Windows): write the Windows path with forward slashes so TOML basic strings do not treat backslashes as invalid escape sequences; unescaped backslashes caused `configuration error: TOML parse failed`, so the daemon could not read the network secret and the service stopped immediately
- [x] 11.2 Fix desktop elevation batch: `mkdir`/`icacls` were using the full config file path (`C:\Program Files\LanLink\lanlinkd.toml`) instead of the directory, which created a directory named `lanlinkd.toml` and blocked the real config file write; derive the directory from `DAEMON_CONFIG`
- [x] 11.3 Surface the elevated `lanlinkd service up` log content in the desktop UI error when the batch exits non-zero, so config/secret failures are visible instead of swallowed
- [x] 11.4 Verify `cargo xwin check -p lanlinkd -p lanlink-desktop` passes; add regression tests in `crates/lanlinkd/src/config.rs` for forward-slash `password_file` and for backslash-in-basic-string TOML rejection
- [x] 11.5 Rebuild the Windows MSI with the fixes so the VM tests the updated binaries

## 12. Windows VM feedback round 6 (no adapter / missing TAP driver)

- [x] 12.1 Add persistent file logging to the Windows SCM service entry point (`lanlinkd service run`) so daemon startup failures are captured in `C:\Program Files\LanLink\lanlinkd.log`
- [x] 12.2 Surface the last daemon log lines in the desktop UI service diagnosis panel
- [x] 12.3 Replace the OpenVPN TAP-Windows6 backend with Wintun (`wintun-bindings`) so no separate driver install is required; load the bundled `wintun.dll` from the binary directory or `C:\Program Files\LanLink`
- [x] 12.4 Update README/docs to reflect Wintun usage and note the layer-3 (IP only) limitation on Windows
- [x] 12.5 Rebuild the Windows MSI with the Wintun backend and verify `cargo xwin check --workspace` passes

## 13. Windows VM feedback round 7 (Wintun netmask invalid)

- [x] 13.1 Fix `prefix_to_netmask` byte order bug: remove `.to_be()` so `Ipv4Addr::from(u32)` produces the correct netmask (255.255.255.0 for /24) on little-endian hosts
- [x] 13.2 Add unit test for `prefix_to_netmask` covering /0, /8, /16, /24, /30, /32, and invalid /33
- [x] 13.3 Rebuild the Windows MSI with the netmask fix
