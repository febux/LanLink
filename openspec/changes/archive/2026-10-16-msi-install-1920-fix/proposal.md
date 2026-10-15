# Proposal

## Why

The LanLink 0.5.2 Windows MSI fails to install on a clean machine with error 1603. The verbose MSI log shows the real failure is Windows Installer error 1920:

```
Error 1920. Service 'LanLink Virtual LAN Daemon' (LanLinkDaemon) failed to start.
```

The installer rolls back the entire installation because the service is marked `Vital="yes"` and `ServiceControl Start="install"` tells Windows Installer to start the service immediately. The default `C:\ProgramData\LanLink\lanlinkd.toml` shipped by the MSI points `password_file` to `C:\ProgramData\LanLink\network.secret`, which the installer never creates. `lanlinkd service run` therefore exits during service startup, SCM reports the start failure, and the install rolls back.

The existing `DebugNoAutostart` build flag already works around this exact problem, which indicates the current default behavior is unsafe for production installs.

## What Changes

- Stop starting the `LanLinkDaemon` service during MSI installation. The service is still installed, still configured as automatic start, and still stopped/removed on uninstall/reinstall.
- Remove the `DebugNoAutostart` conditional from the WiX source; the no-start-on-install behavior becomes the default.
- Drop the `DEBUG_NO_AUTOSTART` environment variable from `build-msi.sh` (it becomes a no-op to avoid breaking existing CI/scripts).
- Make the MSI build script work on Linux by auto-detecting the unsupported .NET WiX CLI and falling back to the `jkroepke/wixtoolset` Docker image (or any user-supplied image). Also detects `podman`.
- Statically link the Microsoft C runtime on Windows (`-C target-feature=+crt-static`) so the binaries do not depend on `VCRUNTIME140.dll` and work on a clean Windows VM without installing the VC++ Redistributable.
- Update `dist/windows/README.md` to state that the service is registered but not started by the installer, and that the user must configure it (e.g. `lanlinkd service up`) before it will run.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `windows-installer`: completes successfully on a clean machine without a pre-existing network secret or coordinator.
- `windows-msi-build`: builds on Linux via Docker/Podman fallback when native WiX is unsupported.
- `windows-binaries`: no longer require the VC++ Redistributable on the target machine.

## Impact

- `dist/windows/lanlink.wxs`
- `dist/windows/build-msi.sh`
- `dist/windows/README.md`
- `.cargo/config.toml`
- `.gitignore`
