# Tasks

## 1. Harden Windows UDP socket setup

- [x] 1.1 Add explicit `windows-sys` dependency with `Win32_Networking_WinSock`
      feature to `crates/lanlinkd/Cargo.toml`.
- [x] 1.2 In `bind_udp_socket`, log success/failure of the
      `SIO_UDP_CONNRESET` ioctl at `debug`/`warn` level.
- [x] 1.3 Also disable `SIO_UDP_NETRESET` on the bound UDP socket.

## 2. Make UDP recv loops resilient to 10054

- [x] 2.1 In `start_coordinated_pump` and `start_packet_pump`, detect
      `WSAECONNRESET` / `os error 10054` and log at `debug`/`trace` while
      continuing the loop.

## 3. Improve desktop diagnostics

- [x] 3.1 Update `crates/lanlink-desktop/assets/app.js` to tell users to check
      `C:\Program Files\LanLink\lanlinkd.log` when the service stopped with an
      unexpected exit code.

## 4. Verify

- [x] 4.1 `cargo check -p lanlinkd` and `cargo check -p lanlink-desktop` pass.
- [x] 4.2 Existing tests still pass.
- [x] 4.3 Windows smoke test: daemon starts, joins a network, and does not
      produce `ERROR UDP recv failed 10054` under normal peer churn or relay
      keepalive traffic.
