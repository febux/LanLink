# Windows implementation plan

This document describes the current state of LanLink on Windows and how to test it.

## Backend choice

Windows has two common open-source virtual NIC drivers:

- **Wintun** (WireGuard project) — L3 TUN only. Cannot carry raw Ethernet frames,
  so it is unsuitable for a layer-2 virtual LAN that needs ARP/broadcast/multicast.
- **TAP-Windows6** (OpenVPN project) — L2 TAP. Carries raw Ethernet frames, which
  is what LanLink's switch core expects.

LanLink therefore uses **TAP-Windows6** via the [`tap-windows`](https://crates.io/crates/tap-windows)
crate. The driver must be installed separately by the user; the crate only opens the
existing driver.

## Current implementation

`crates/lanlink-tap/src/lib.rs` contains a `WindowsTap` backend that:

1. Opens an existing TAP-Windows6 adapter by name, or creates a new one and renames it.
2. Sets the IPv4 address and netmask using the driver's built-in `set_ip` call.
3. Sets the MTU via `netsh interface ipv4 set subinterface ... mtu=...`.
4. Brings the adapter up (`Device::up`).
5. Runs the device in a dedicated thread and exposes the `VirtualNic` trait through
   tokio channels.

## Known limitations

- The Windows backend has been cross-compiled (`cargo check --target x86_64-pc-windows-msvc`)
  but not yet tested on a real Windows machine.
- Windows firewall and the TAP adapter may need manual allow rules.
- The firewall/gateway crates are still Linux-only (`nftables`). Windows WFP and
  routing equivalents are not implemented.
- The status socket is currently a Unix domain socket; a Windows named-pipe equivalent
  is needed for the TUI/WebUI to talk to the daemon on Windows.
- The `tap-windows` crate's `Device` is `!Send`, so the backend uses a dedicated
  thread with a small polling timeout. This is functional but slightly less efficient
  than a true async reactor.

## Testing checklist

Before claiming Windows works, verify:

1. Install TAP-Windows6 driver.
2. Build with `cargo build --workspace --release` on Windows.
3. Coordinator runs and is reachable from both PCs.
4. `lanlinkd up --name <net>` creates/joins the network and the TAP adapter appears
   in `ncpa.cpl` with the expected `26.x.x.x` address.
5. Both PCs can ping each other's `26.x.x.x` addresses.
6. A simple LAN game or broadcast discovery tool sees the remote peer.
7. `lanlink-web` / `lanlink-tui` can read daemon status (currently requires a Unix socket,
   so this may fail on Windows until the status transport is ported).

## Next steps for full Windows parity

- Add Windows service support (SCM) for `lanlinkd service ...`.
- Port the daemon status socket to Windows named pipes or TCP loopback.
- Implement Windows firewall helper (WFP) for public/gaming networks.
- Implement Windows gateway mode using Windows routing and NAT.
- Bundle the TAP-Windows6 driver in the installer or detect its absence and prompt.
