# Tasks

## 1. Windows gateway mode (gateway/NAT)

Ticket: t_733b2be1

- [x] 1.1 Enable path: per-interface IP forwarding on the virtual adapter (`Set-NetIPInterface -Forwarding Enabled`) and WinNAT masquerade of the virtual subnet onto the physical LAN (`New-NetNat -InternalIPInterfaceAddressPrefix`), replacing any pre-existing NAT instance of the same name (idempotent)
  - `crates/lanlink-gateway/src/lib.rs` `windows::build_enable_script`; test `enable_script_enables_forwarding_and_replaces_nat`; enabled in `lanlinkd/src/main.rs:529-543`.
- [x] 1.2 Scenario "Remote peer reaches a physical-LAN host": gateway forwards and masquerades virtual-subnet traffic onto the physical LAN (mirror of Linux ip_forward + nftables MASQUERADE)
  - Same enable path; on-hardware verification is `docs/windows-real-machine-checklist.md` step 6 (`scripts/windows-preflight.ps1 -Gateway`).
- [x] 1.3 Scenario "Gateway state is cleaned up": NAT instance removed and forwarding disabled when gateway mode is turned off or the daemon stops
  - `windows::build_disable_script` (test `disable_script_removes_nat_and_forwarding`); called from `lanlinkd/src/main.rs:827-832` on shutdown.

## 2. Windows peer route management

Ticket: t_efe54458

- [x] 2.1 Routes for gateway-shared subnets are installed via the gateway's virtual IP on the virtual adapter (`New-NetRoute -NextHop <gateway virtual IP>`, replacing any existing route first)
  - `windows::build_route_add_script`; tests `route_add_script_replaces_then_adds`, `scripts_escape_interface_names`.
- [x] 2.2 Scenario "Routes follow gateway peers": routes added/removed when the coordinator advertises or withdraws gateway peers (initial sync from join response + resync on peer refresh; all routes removed on shutdown)
  - `RouteManager::sync_routes` (stale-diff removal tested via `gateway_routes_excludes_non_gateways` / sync tests); wired in `lanlinkd/src/main.rs:564-566, 696-698, 832`. Removal script test: `route_del_script_removes_route`.
- [x] 2.3 Scenario "Stale routes are flushed at startup": NetMgmt-protocol routes left by a crashed previous run are removed before the first route synchronization
  - `windows::build_flush_stale_script` + `RouteManager::flush_stale` (test `flush_stale_script_targets_mgmt_routes_only`, `parse_managed_routes_skips_kernel_routes`); called at `lanlinkd/src/main.rs:557-566` before the initial `sync_routes`.

## 3. Tests & build

Ticket: t_286530a5

- [x] 3.1 Spec scenarios covered by unit tests in the project's accepted style (cross-platform script-builder tests under `cfg(windows)` module): 13 tests in `lanlink-gateway`, all scenario-relevant script builders asserted
- [x] 3.2 `cargo check --workspace` green (exit 0)
- [x] 3.3 `cargo test --workspace` green — all packages pass, incl. `lanlink-gateway` 13/13, `lanlinkd` 19/19, `lanlink-firewall` 7/7
