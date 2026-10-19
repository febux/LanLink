# Proposal

## Why

Users on Windows report the `LanLinkDaemon` service stopping with exit code 1
and the desktop app showing `cannot connect to adapter` / `No daemon status
available`. The only daemon log lines surfaced by the UI are repeated:

```
ERROR lanlinkd: UDP recv failed error=An existing connection was forcibly closed by the remote host. (os error 10054)
```

`lanlinkd` already tries to disable `SIO_UDP_CONNRESET` when binding the UDP
socket, but the error is still reaching the recv loop and being logged at the
ERROR level. This makes the failure look fatal when it is usually just ICMP
"port unreachable" noise from a stale peer endpoint. The desktop UI then shows
the noisy tail instead of the real startup failure.

## What Changes

- Harden the Windows UDP socket setup in `crates/lanlinkd/src/main.rs`:
  - Add an explicit `windows-sys` dependency with the required WinSock feature.
  - Verify the `SIO_UDP_CONNRESET` ioctl succeeded and log the result.
  - Also disable `SIO_UDP_NETRESET` for the same reason.
- Treat `WSAECONNRESET` / `os error 10054` in the UDP recv loops as a
  recoverable, expected condition: log at `tracing::debug!` and continue.
- Update the desktop app's service-failure hint to point users at the daemon
  log file explicitly and to check for Wintun/TAP driver issues.
- Document that the observed 25-second periodicity matches the relay-keepalive
  interval; surviving resets usually come from an unreachable relay server.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `lanlinkd` UDP data-plane resilience on Windows.
- `lanlink-desktop` Windows service diagnostics.

## Impact

- `crates/lanlinkd/Cargo.toml` and `crates/lanlinkd/src/main.rs`.
- `crates/lanlink-desktop/assets/app.js`.
