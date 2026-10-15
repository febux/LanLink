# Tasks

## 1. Change WiX service control to not start on install

- [x] 1.1 Remove `Start="install"` from the `LanLinkDaemon` `ServiceControl` in `dist/windows/lanlink.wxs`
- [x] 1.2 Remove the `DebugNoAutostart` conditional since the no-start behavior is now default
- [x] 1.3 Update the XML comment explaining why the service is not started during install

## 2. Update the MSI build script

- [x] 2.1 Remove the `DEBUG_NO_AUTOSTART` option from `dist/windows/build-msi.sh` (or keep it as a no-op with a deprecation note)
- [x] 2.2 Auto-detect unsupported native WiX on Linux and fall back to the `jkroepke/wixtoolset` Docker image
- [x] 2.3 Detect Podman as an alternative to Docker
- [x] 2.4 Only pass `-acceptEula` for WiX 7+; skip it for WiX 5/6 (the Docker image ships WiX 5.0.2)
- [x] 2.5 Ensure the default `wix build` invocation still works

## 3. Update Windows packaging documentation

- [x] 3.1 Update `dist/windows/README.md` to say the service is registered automatic but not started by the installer
- [x] 3.1 Add the post-install step: run `lanlinkd service up --name <network> --password <secret>` (or manually configure `lanlinkd.toml` + `sc start LanLinkDaemon`)

## 4. Verify

- [x] 4.1 Reviewed WiX source and script syntax (XML parses, bash `-n` passes)
- [x] 4.2 Built the MSI successfully on Linux via Docker fallback: `dist/windows/out/lanlink-0.5.2-x64.msi` (45 MB, WiX 5.0.2)
- [x] 4.3 Rebuild the Windows binaries with static CRT and confirm `lanlinkd.exe` no longer depends on `VCRUNTIME140.dll`
- [x] 4.4 On a Windows test machine, install the new MSI and confirm it completes without error 1920/1603 and without rollback
- [x] 4.5 Confirm `LanLinkDaemon` appears in Services as automatic but not running
- [x] 4.6 Confirm `lanlinkd service up --name test --password test` starts the service successfully
