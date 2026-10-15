# Radmin VPN-style Virtual LAN — Cross-Platform Research Analysis & Build Plan

> Goal: a functional cross-platform replica of Radmin VPN — a free, easy-to-use
> layer-2 mesh VPN aimed at gaming, remote access, and connecting distributed teams.
> Primary platforms: **Linux, Windows, macOS** (x86_64 + ARM64).
> Stack decision: Rust or Go (analyzed in §7; recommendation: Rust for the daemon,
> with Go as a fully valid alternative — criteria given).

---

## 1. Executive summary

Radmin VPN is a **Windows-only layer-2 (Ethernet) mesh VPN** by Famatech. Users create or
join named networks protected by a password; every peer gets a virtual NIC
("Famatech Radmin VPN Ethernet Adapter") with an IP from `26.0.0.0/8`. Traffic flows
peer-to-peer over UDP with AES-256 encryption; when a direct connection is impossible,
traffic falls back to a **relay over TCP**. Because it emulates a real Ethernet segment
(ARP, broadcast, multicast), LAN games, SMB, and legacy protocols "just work".

Our replica is a **cross-platform green-field project** with three major subsystems,
plus a thin platform abstraction for the virtual NIC:

1. **Virtual NIC abstraction** — TAP on Linux, Wintun on Windows, utun on macOS;
   all feed raw Ethernet frames into the same switch core.
2. **Data plane** — Ethernet frame encryption/encapsulation, MAC-learning switch
   mesh, broadcast/multicast forwarding.
3. **NAT traversal** — STUN, UDP hole punching, port mapping, relay fallback.
4. **Control plane** — a coordination service (create/join network by name+password,
   membership list, key distribution, node status).

None of this requires reverse-engineering Radmin itself; the architecture is well
understood and there are open reference implementations to study (ZeroTier, n2n, tinc).
Estimated effort for a solid v1.0: **~4–6 engineer-months** (see §9).

---

## 1.1 Current implementation status (last updated 2026-10-11)

| Milestone | Status | Notes |
|---|---|---|
| **M0** | ✅ Done | Stack chosen: Rust workspace; crates scaffolded. |
| **M1** | ✅ Done | Encrypted two-node ping over direct UDP. `lanlinkd static-peer` works; Linux TAP backend implemented; Windows Wintun / macOS utun stubbed. |
| **M2** | ✅ Done | Switch engine integrated into `lanlinkd`: MAC learning, flooding, PMTU chunking/reassembly, per-peer replay window. Unit tests pass. |
| **M3** | ✅ Done | Coordinator REST API (`POST /networks`, `POST /networks/:id/join`, `GET /networks/:id/members`) with SQLite + Argon2id; `lanlinkd run` joins via coordinator and floods frames to discovered peers; `lanlinkctl coord create/join/list` CLI. |
| **M4** | ✅ Done | STUN client, host/server-reflexive candidate gathering, UDP hole punching, coordinator candidate exchange, and relay fallback all implemented. UPnP/NAT-PMP deferred post-v1. |
| **M5** | ✅ Done | Custom content-blind UDP relay server (`lanlink-relayd`) with E2E-encrypted relay traffic. |
| **M6** | ✅ Done | Gateway mode + per-network firewall profile. Linux-only v1: nftables firewall, proxy-ARP, ip_forward, MASQUERADE, route push to peers. |
| **M7** | ✅ Done | TOML config files (+`password_file` secrets), graceful SIGTERM shutdown with firewall/route/NAT cleanup, crash-safe idempotent nftables re-apply + stale-route flush, `lanlinkd service` management (systemd template unit w/ sandboxing + privilege separation, Windows SCM wrapper), sd_notify READY/watchdog, deb/rpm/AUR/macOS-pkg packaging, MSI + WiX script (`dist/windows/build-msi.sh` + `lanlink.wxs`), 72 h soak harness (`tests/soak.sh`), memory-audit harness (`tests/memory-audit.sh`: miri pass on `lanlink-proto`/`lanlink-switch`/crypto replay-window; valgrind gate when installed), and opt-in auto-update channel (`lanlinkctl update check`, daemon config) all implemented. Runtime soak/valgrind require root/CI. |
| **M8** | ✅ Done | v1.0 public beta polish: public directory server (`GET /directory`, `lanlinkctl directory list`), opt-in anonymized telemetry (`POST /telemetry`, `GET /metrics`, `lanlinkctl coord metrics`), and static docs site under `docs/`. GUI/TUI remains a post-v1.0 stretch goal. |
| **M9** | ✅ Done | TUI: `lanlink-tui` (ratatui) with dashboard / peers / networks screens, directory browsing, create+join parity with `lanlinkctl`; daemon local status API (read-only Unix-socket NDJSON snapshots at `/run/lanlink/lanlinkd-<id>.sock`, `--status-socket`/`[status] socket` overrides); switch extensions: per-peer tx/rx counters, PING/PONG latency probes (monotonic probe clock), keepalive sender-attribution liveness fix. Plan: `openspec/changes/archive/2026-10-15-tui-status-and-frontend/`. |
| **M9+** | ✅ Done | WebUI: `lanlink-web` (axum + vanilla-JS SPA) with dashboard / peers / networks screens, public directory browsing, create+join parity with `lanlinkctl`; proxies the daemon status socket and coordinator REST API; binds to `127.0.0.1:8686` by default. Desktop UI: `lanlink-desktop` (Tauri v2) bundles the same SPA as a native window with system tray and Rust invoke commands for `get_snapshot`, `list_directory`, `create_network`, `join_network`. Unified launcher: `lanlink-gui --mode tui|web|desktop` starts any frontend, and `make gui MODE=desktop` starts the full dev stack with one command. Plan: `openspec/changes/archive/2026-10-15-webui-frontend/`.

**Next priority for future sessions**: cross-platform CI hardening and full 72 h
soak / valgrind runtime validation. M8 non-GUI items, the M9 TUI stretch
goal, the M9+ WebUI, and the unified `lanlink-gui` launcher are complete.

**Key files to know when resuming**:
- `crates/lanlink-tap/src/lib.rs` — virtual NIC abstraction (Linux TAP implemented, Windows/macOS stubs).
- `crates/lanlink-crypto/src/lib.rs` — `StaticSession` (symmetric AEAD) used today; Noise upgrade planned.
- `crates/lanlink-switch/src/lib.rs` — MAC learning, flooding, chunking/reassembly, peer address updates, relay-candidate fallback in `sync_peers`.
- `crates/lanlink-punch/src/lib.rs` — STUN client, host/reflexive candidate gathering, hole-punch engine.
- `crates/lanlink-relay/src/lib.rs` + `src/main.rs` — content-blind UDP relay library and `lanlink-relayd` binary.
- `crates/lanlink-firewall/src/lib.rs` — per-network inbound firewall profile (nftables on Linux; public/gaming default blocks SMB/RDP/SSH/NetBIOS); `apply` is crash-safe (replaces any previous table).
- `crates/lanlink-gateway/src/lib.rs` — gateway mode: proxy-ARP, ip_forward, MASQUERADE; `RouteManager` is stateful (removes stale routes on shutdown) and `flush_stale` clears crash leftovers at startup.
- `crates/lanlinkd/src/main.rs` — daemon main; `static-peer` and `run` subcommands (now `--config`-driven); global shutdown signal; cleanup on exit.
- `crates/lanlinkd/src/config.rs` — TOML config + `password_file` + CLI-override resolution.
- `crates/lanlinkd/src/service.rs` + `src/sdnotify.rs` — `lanlinkd service install/start/stop/...` (systemd template + Windows SCM) and systemd notify/watchdog.
- `crates/coordinator/src/api.rs` + `db.rs` — coordinator REST API and SQLite persistence; binary renamed to `lanlink-coordinator`.
- `crates/lanlinkctl/src/main.rs` — CLI for create/join/list; network create supports `--public`.
- `dist/` — packaging: systemd units (sandboxed `lanlinkd@.service` etc.), `deb/` build script, `rpm/` spec, `aur/` PKGBUILD, `macos/` launchd + pkg, `windows/` service docs; see `dist/README.md`.
- `tests/m1_ping_lab.sh` — root-required netns smoke test.
- `tests/soak.sh` — 72 h churn/leak soak harness (root required).
- `tests/memory-audit.sh` — miri + optional valgrind memory-safety audit.
- `GET /directory` + `crates/coordinator/src/db.rs::list_public_networks` — public directory server.
- `crates/lanlink-proto/src/telemetry.rs` + `POST /telemetry` / `GET /metrics` + `lanlinkctl coord metrics` — opt-in anonymized telemetry.
- `docs/` — static documentation site (Markdown + stdlib-only `build.sh`).

---

## 2. Product research: what Radmin VPN is

### 2.1 Positioning

| Attribute | Value |
|---|---|
| Vendor | Famatech Corp. (BVI jurisdiction), makers of Radmin remote-control |
| Price | 100% free, no ads, no paid tiers (funded by the commercial Radmin product) |
| Claimed user base | 60 million users |
| Platforms (Radmin) | Windows 11/10/8/7, x86/x64/ARM64 — **no Linux/macOS** |
| Platforms (our replica) | **Linux, Windows, macOS** — x86_64 + ARM64 (the gaps we fill) |
| Footprint | ~100 MB disk, 1 GHz CPU |
| Privacy claims | No-log policy; servers in BVI; no traffic logging |
| Crypto (claimed) | End-to-end AES-256 |
| Distribution | Own installer with auto-update |
| Community | Discord server + radmin-club.com hosting public "gaming networks" |

### 2.2 Core user flows

- **Create network**: name + password → network appears in main window.
- **Join network**: enter name + password → network + its node list appear.
- **Node list UI**: every network shows connected peers; right-click a peer →
  **ping**, copy IP, etc. Connection type per peer (Direct / Relay-TCP) is surfaced.
- **Public gaming networks**: curated large networks anyone can join for specific games.
- **Gateway mode**: a node can share its physical LAN with the virtual network
  (configured per node).
- **Gaming-network firewall**: always-on firewall in "Gaming Networks" blocking common
  Windows services (SMB shares etc.) to protect players from each other.

### 2.3 Technical characteristics (confirmed + inferred)

| Fact | Source | Confidence |
|---|---|---|
| Virtual adapter is an **Ethernet adapter** (L2 VPN) | Kaspersky firewall doc names "Famatech Radmin VPN Ethernet Adapter" | Confirmed |
| Virtual subnet is **26.0.0.0/8** | Kaspersky doc references `26.0.0.0/8` row | Confirmed |
| Connections are **direct P2P when possible**, **Relay/TCP fallback** | Official troubleshooting FAQ | Confirmed |
| AES-256 end-to-end encryption | Official security page | Confirmed |
| Works behind firewalls/NAT without port forwarding | Marketing + FAQ | Confirmed |
| Runs as a Windows **service** + tray GUI; service can be started separately ("Service not started" state) | FAQ | Confirmed |
| LAN games & broadcasts work (game server discovery) | Help/gaming guides | Confirmed |
| Network size limit ~1000 nodes, networks per user unlimited | Community reports | Inferred (hedge) |
| Throughput "LAN-like" (~100 Mbps–1 Gbps typical) | Community benchmarks | Inferred |
| Identity = per-install keypair; network password gates membership | Standard design of such systems | Inferred (design choice) |

**Key insight**: the product's differentiation is *simplicity* (name+password, zero config)
plus *L2 emulation* (LAN games work) plus *free*. Performance is "good enough", not
WireGuard-class. Our Linux replica should preserve those three properties.

---

## 3. Technical analysis: how such a system works

### 3.1 Why L2 matters

An L3 VPN (WireGuard, Tailscale) routes IP packets — ARP, DHCP, NetBIOS name resolution,
IPX-era game protocols and UDP-broadcast server discovery do **not** cross it. Radmin VPN
instead **bridges Ethernet frames**: each peer's virtual NIC behaves like a switch port on
a shared virtual switch. That is why LAN games see each other's hosted servers.

The platform-specific adapters all present raw Ethernet frames to the same switch code:

- **Linux** — kernel **TAP device** (`/dev/net/tun` in TAP mode).
- **Windows** — **Wintun** (the MIT-licensed driver from the WireGuard project) creates a
  virtual Ethernet adapter that exposes a ring buffer of raw frames.
- **macOS** — **utun** (or a custom NetworkExtension in a future v2) gives raw Ethernet
  frames; for v1.0 we target the `utun` API available to root daemons.

On all three, the daemon is simply another Ethernet switch port on a shared virtual switch.

### 3.2 Architecture (reconstructed)

```
┌────────────────────────── Famatech cloud (our: coordinator) ─────────────────────────┐
│  Control server: network registry (name→id), auth, membership, key distribution      │
│  STUN servers: public address:port discovery                                        │
│  Relay servers: TCP/UDP packet forwarders when P2P fails                            │
└───────────▲───────────────────────────────▲──────────────────────────────────────────┘
            │ control channel (TLS)        │
┌───────────┴──────────┐          ┌────────┴─────────┐
│  Node A (peer)       │  UDP     │  Node B (peer)   │
│ ┌──────────────────┐ │ ◄──────► │ ┌──────────────┐ │
│ │ TAP adapter      │ │ encrypted│ │ TAP adapter  │ │
│ │ 26.x.y.z         │ │ Ethernet │ │ 26.x.y.w     │ │
│ └────────▲─────────┘ │ frames   │ └──────▲───────┘ │
│ ┌────────┴─────────┐ │          │ ┌──────┴───────┐ │
│ │ daemon: switch,  │ │          │ │ daemon       │ │
│ │ MAC learning,    │ │          │ └──────────────┘ │
│ │ crypto, puncher  │ │          └──────────────────┘
│ └──────────────────┘ │
└──────────────────────┘
```

Per-node components:

- **Virtual switch**: reads frames from TAP, looks up destination MAC → peer (with
  flooding of unknown/broadcast/multicast destinations), encrypts, sends over UDP.
- **MAC learning table**: src-MAC → peer-id, built from observed frames, aged out.
- **Puncher**: STUN binding to discover reflexive addresses; exchanges candidate pairs
  through the coordinator; fires simultaneous UDP packets to punch; verifies via
  authenticated ping; falls back to relay.
- **Session manager**: per-peer Noise-encrypted channels, rekeying, keepalives.

### 3.3 NAT traversal strategy (the hard part)

Ordered escalation (same ladder Radmin-class products use):

1. **STUN discovery** of each peer's public mapping (full-cone, restricted, port-restricted
   cone NATs usually punch fine with simultaneous open).
2. **UDP hole punching** with multiple candidate ports (birthday-attack style port
   guessing for hard NATs).
3. **Port mapping**: UPnP-IGD / PCP / NAT-PMP to pin a forwarding on the router.
4. **Relay fallback**: DTLS/encrypted TCP or UDP through relay servers (TURN-like).
   Marker in UI: connection type "Relay" (Radmin surfaces exactly this).
5. Keepalive packets to keep mappings alive (e.g., every 15–25 s, adaptive).

### 3.4 Crypto design (modern equivalent)

Radmin claims AES-256 E2E. For our replica, use a **WireGuard/Noise-style** design:

- **Identity**: each node generates an Ed25519 identity keypair (device identity),
  and an X25519 static key for channel establishment.
- **Network key**: derived from the user password with **Argon2id** (network-ID salted).
  Coordinator never stores the password (stores only a verifier / PAKE output).
- **Pairwise channel**: Noise protocol (`Noise_IKpsk2` or `Noise_XXpsk3`) with the
  network PSK bound in — gives mutual identity hiding + network membership proof.
- **Data AEAD**: ChaCha20-Poly1305 or AES-256-GCM, 64-bit packet counter nonce,
  replay window (like IPsec/WireGuard anti-replay, e.g., 1024-bit bitmap).
- **Rekey** every N minutes / N bytes; erase old keys.
- **Relay traffic is still E2E encrypted** — relays never see plaintext (this is what
  "end-to-end" means in Radmin's marketing; preserve the property).

### 3.5 Broadcast / multicast handling (needed for games)

- Pure design: flood broadcasts to all peers (fine ≤ ~50 peers).
- Optimization (ZeroTier-style): multicast group membership tracking — peers announce
  interest in IPv4 multicast groups (224.0.0.x, 239.255.x.x for SSDP/game discovery);
  frames to uninterested groups are dropped. **Defer to post-v1.0**; flood first.

### 3.6 Gateway mode (node shares its LAN)

A node flags itself "gateway" (like Radmin). Then:

- The gateway answers ARP (proxy-ARP) on the TAP for the physical LAN subnet,
  ip_forward=1, and nftables MASQUERADE (or static routes pushed to peers) for
  traffic toward the physical LAN.
- Peers get routes `physical-subnet via gateway-virtual-IP` (for L3 traffic) and
  proxy-ARP handles L2 semantics.

### 3.7 Gaming-network firewall

Radmin's always-on firewall for public networks blocks Windows services (SMB 445, etc.).
Linux equivalent: per-network **nftables/iptables chain on the TAP interface** with a
default-deny inbound rule set for dangerous ports (139/445/3389/22 optional), enabled by
default for networks flagged "public/gaming", configurable.

### 3.8 What is explicitly out of scope for v1.0

- Radmin remote-control integration (separate product) — we integrate with SSH/VNC/RDP instead.
- Legacy non-IP protocols such as IPX/AppleTalk (modern LAN games are IP-only).
- Kernel-side fast path (Linux eBPF/XDP, Windows NDIS filter driver, macOS NetworkExtension).
- No-log claims/jurisdiction — a policy matter for whoever hosts the coordinator.

---

## 4. Reference open-source implementations (study, don't necessarily copy)

| Project | Model | Layer | License | Notes |
|---|---|---|---|---|
| **ZeroTier** | controller + P2P mesh | L2 (emulated Ethernet) | BSL 1.1 | Closest analog. BSL is source-available, not FOSS — study architecture, don't copy code casually |
| **n2n** (ntop) | supernode + P2P edge | L2 over TAP | GPL-3 | Literally "open-source Hamachi/Radmin"; simplest architecture; GPL-3 is viral if copied |
| **tinc** | full mesh | L2 (switch mode) + L3 | GPL-2+ | Old but robust; good MAC-learning/flooding code |
| **Tailscale/Headscale** | coordination + WireGuard | L3 | BSD-3/MIT(Headscale) | Best control-plane UX reference (auth, ACL, key rotation) |
| **Nebula** | overlay L3 | L3 | MIT | Clean certificate/CA design for overlay identity |
| **NetBird** | management + WireGuard | L3 | Apache-2.0 | Modern Go stack incl. Signal/Relay servers — good server-side reference |
| **coturn** | STUN/TURN relay | — | BSD-style | Battle-tested relay we can reuse as-is for fallback |
| **WireGuard** (kernel) | L3 tunnel | L3 | GPL-2 / wg-go Apache | We can't reuse the kernel impl for L2, but its Noise design + anti-replay + timer wheel are the gold standard to imitate |

**Licensing conclusion**: write our own daemon (clean-room, informed by published
protocol designs); reuse only MIT/Apache/BSD components (coturn, crates/libs). Avoid
GPL/BSL code unless we accept the obligations.

---

## 5. Requirements for the cross-platform replica ("LanLink" — working name)

### 5.1 Functional requirements (v1.0 = parity)

| # | Requirement | Radmin parity |
|---|---|---|
| F1 | Create network (name + password) via CLI/TUI | ✔ |
| F2 | Join existing network by name + password | ✔ |
| F3 | Virtual TAP NIC per network, IPs from `26.0.0.0/8` (configurable range) | ✔ |
| F4 | Node list with status (online/offline, latency, Direct/Relay) | ✔ |
| F5 | Peer ping (ICMP handled via normal OS ping + our latency probe) | ✔ |
| F6 | P2P UDP connections with NAT traversal, no manual port forwarding | ✔ |
| F7 | Relay fallback; UI/CLI shows connection type | ✔ |
| F8 | E2E encryption (Noise + AEAD), relay blind to content | ✔ |
| F9 | Ethernet-level emulation: ARP, broadcast, multicast → LAN games work | ✔ |
| F10 | Gateway mode: share physical LAN behind a node | ✔ |
| F11 | "Public network" firewall profile (block SMB etc. by default) | ✔ |
| F12 | Daemon (systemd / Windows service / launchd) + CLI on all platforms; optional GUI/tray later | Radmin is GUI-first; CLI-first keeps parity across OSes |
| F13 | Multiple concurrent networks per node | ✔ |
| F14 | Auto-update via distro package / built-in updater | partial |
| F15 | Public gaming networks directory (hosted list) | ✔ (community feature) |
| F16 | Native installers for all three platforms: `deb`/`rpm`/AUR, Windows MSI, macOS `.pkg` | ✔ |

### 5.2 Non-functional targets

- **Throughput**: ≥ 500 Mbps single stream, ≥ 1 Gbps aggregate on a modern x86 (userspace
  TAP + ChaCha20; comparable to ZeroTier/wireguard-go class).
- **Latency overhead**: ≤ 1 ms added over direct WAN RTT (P2P path).
- **Memory**: daemon idle < 50 MB.
- **Connection setup**: P2P established < 5 s (easy NAT), < 15 s worst case, relay < 5 s.
- **Scale**: 256 peers/network mesh (flood optimization for more); coordinator: 100k nodes.
- **Robustness**: no restarts needed on network change (multi-homing, Wi-Fi↔Ethernet).
- **Auditability**: no logs by default; optional debug logging opt-in.

### 5.3 Non-goals (v1)

- Obfuscation/censorship resistance (domain-fronting etc.).
- ACLs per network (everyone-in-network is trusted — Radmin model; post-v1 feature).
- Kernel-side fast path (eBPF/XDP, NDIS filter, NetworkExtension) — userspace first.
- Third-party mobile platforms (iOS/Android) at launch.

---

## 6. Proposed architecture

### 6.1 System components

```
──────────────── Server side (self-hostable) ────────────────
 coordinator (REST/gRPC + WebSocket):
   • network registry, membership, invitations via name+password
   • identity certs (Ed25519) signing + revocation lists
   • candidate exchange for hole punching (signaling)
   • virtual IP assignment (deterministic, collision-free)
 relay (TURN-like): forwards encrypted datagrams (E2E opaque)
 stun service: RFC 5389/8489 binding + coturn reuse option
 directory (optional): public gaming networks list

──────────────── Client side (cross-platform daemon) ────────────────
 lanlinkd (root / elevated admin):
   • nic manager      – create/destroy virtual NIC per network, set MTU/UP
     (Linux TAP, Windows Wintun ring, macOS utun)
   • switch core      – frame classification, MAC table, flooding
   • peer sessions    – Noise channels, timers, keepalive, rekey
   • transport        – UDP sockets; relay mode; PMTU discovery
   • nat puncher      – STUN client, candidate mgmt, hole firing
   • gateway module   – proxy-ARP, ip_forward, platform firewall rules
   • local firewall   – per-NIC firewall profile (nftables on Linux, Windows
     Firewall rules, macOS pf anchors) for public/gaming networks
   • control client   – authenticated channel to coordinator (mTLS or Noise)
  lanlinkctl (user): create/join/leave/list/status/ping/diag
  lanlink-gui (user): unified launcher for TUI (`lanlink-tui`), WebUI (`lanlink-web`), and Desktop (`lanlink-desktop`)
  lanlink-desktop (user): native Tauri desktop app with tray icon, bundled web frontend, and direct daemon/coordinator APIs
```

### 6.2 Wire protocol (data plane) — sketch

UDP datagram to peer's mapped address:

```
 0               15 16            31
┌─────────────────┬───────────────┐
│  version | type │   reserved    │  4 B header
├─────────────────┴───────────────┤
│  receiver peer id (8 B)         │  anti-crosstalk
├─────────────────────────────────┤
│  counter (8 B, monoton. inc)    │  nonce input / anti-replay
├─────────────────────────────────┤
│  AEAD ciphertext                │
│   plaintext = [ethertype? no —  │
│   full Ethernet frame chunk(s)] │
│   + minimal inner header        │
│   (chunk seq for >PMTU frames)  │
└─────────────────────────────────┘
```

- Outer datagram ≤ ~1400 B to dodge IPv4 fragmentation; Ethernet frames are
  chunked with (frame-id, chunk-idx, chunks-total) and reassembled — same approach
  as ZeroTier's fragmentation.
- Types: `DATA`, `PING`, `PONG`, `PUNCH_PROBE`, `HANDSHAKE_*`, `KEEPALIVE`.
  Handshakes are Noise messages; after handshake only DATA/KEEPALIVE flow.

### 6.3 Control protocol — sketch

mTLS/Noise-authenticated REST + WebSocket push:

- `POST /networks` (create) → `{network_id, assigned_cidr}`
- `POST /networks/{id}/join` (password verifier: OPAQUE or Argon2id-derived PSK
  proof) → `{member_cert, virtual_ip, peer_list}`
- `WS /events` → join/leave/peer-address-change pushes (drives puncher)
- `POST /networks/{id}/candidates` → submit+fetch reflexive candidates (signaling)
- `GET /networks/{id}/members` → statuses (coordinator tracks last-seen via
  keepalives of the control channel)

### 6.4 IP & MAC allocation

- Virtual MAC: random locally-administered (`02:xx…`) per TAP, stable per (node, network).
- Virtual IP: coordinator assigns free IPs in the network's /24 (default `26.<net-byte>.x.y`
  from a global /8-style space we own in the private universe) with lease + reclaim on
  long absence; deterministic hint from node-id hash to survive coordinator downtime.

### 6.5 Failure & lifecycle behavior

- Coordinator down → existing data-plane sessions keep working (Radmin behaves
  similarly; new joins fail). Peers re-announce to each other via known addresses.
- NAT mapping loss → keepalive timer + on-gap re-punch transparently.
- Peer disappearance → MAC entries age (60 s idle), session torn down after 3 missed
  keepalive intervals × backoff.

---

## 7. Stack decision: Rust vs Go

### 7.1 Decision matrix

| Criterion | Rust | Go | Weight |
|---|---|---|---|
| Raw forwarding throughput (userspace TAP + AEAD) | ★★★★★ (zero-copy, no GC) | ★★★☆ (wireguard-go ≈ 1–3 Gbps achievable with buffer pools) | High |
| Tail-latency jitter for games | ★★★★★ | ★★★☆ (GC sub-ms but nonzero) | Medium |
| Memory-safety for a root daemon parsing hostile packets | ★★★★★ | ★★★★ | High |
| NAT traversal ecosystem | ★★★ (librice/ICE, stun crates — usable, younger) | ★★★★★ (**pion/stun, pion/turn, pion/ICE** — de-facto standard, huge accelerator) | High |
| Crypto ecosystem | ★★★★★ (snow/Noise, ring, rustls, dalek, RustCrypto AEADs) | ★★★★★ (stdlib + x/crypto) | Medium |
| TUN/TAP + netlink | ★★★★ (tun, rtnetlink crates) | ★★★★ (vishvananda/netlink + TAP ioctl) | Medium |
| Speed of delivering MVP | ★★★ | ★★★★★ | High |
| Hiring/community for infra tooling | ★★★ | ★★★★★ | Low |
| Single static binary, systemd friendliness | Both excellent | Both excellent | — |
| Long-term maintainability of perf-critical code | ★★★★★ | ★★★★ | Medium |

### 7.2 Library inventories

**Rust**: tokio (or mio), `tun`/`tappers` (TAP), `rtnetlink`, `snow` (Noise), `ring` /
`chacha20poly1305` / `aes-gcm`, `ed25519-dalek` + `x25519-dalek`, `argon2`, `rustls`,
`quinn` (optional QUIC relay transport), `axum`/`tonic` (coordinator), `librice` or
hand-rolled STUN (`stun-rs`), `hickory-*` if needed, `clap` (CLI), `tracing`,
`criterion` (bench), `proptest`+`cargo-fuzz`.

**Go**: std net + `golang.org/x/net`, `pion/stun`, `pion/turn`, `pion/ice`
(or `p2p-go`), `vishvananda/netlink`, `water`/direct TUN ioctls, `crypto/ed25519`,
`golang.org/x/crypto/nacl|chacha20poly1305|argon2`, `google/golang/grpc` + protobuf,
`spf13/cobra`, `uber/zap` or `slog`, `golang.org/x/net/ipv4` (packet crafting),
testing: `stretchr/testify`, `netns` for netns-based tests, fuzzing via native go fuzz.

### 7.3 Recommendation

**Primary recommendation: Rust** for the client daemon + coordinator (single language,
top-tier data-plane performance, memory safety for a root-level packet parser — this is
exactly the class of software where Rust shines, and it cross-compiles cleanly to Windows
and macOS from Linux CI). The main portability cost is writing the small NIC-backend
modules (TAP, Wintun, utun); these are isolated in `lanlink-tap` and do not affect the
protocol or switch core. Mitigate Rust's weakest point (NAT traversal ecosystem) by
**reusing `coturn`** for STUN + relay (language-agnostic) and hand-rolling a lean
ICE-lite puncher — the Radmin connection model (single UDP socket per network,
symmetric-ish behavior) is much simpler than full WebRTC ICE.

**Choose Go instead if**: time-to-MVP dominates (ship in weeks not months), you want the
pion ecosystem's maturity for traversal, and ~1 Gbps throughput is acceptable. A hybrid
(Go coordinator + Rust daemon, or Go v1 → Rust data-plane rewrite later) is a legitimate
ladder — design the wire protocol to be language-agnostic from day one (it is, §6.2).

**Cross-platform note**: both languages compile to Linux/Windows/macOS. The harder parts
are platform-specific *driver plumbing* (Wintun ring API, Windows service control,
macOS utun/launchd) and installer packaging — not the language itself. Rust's
conditional-compilation (`#[cfg(target_os = ...)]`) keeps these backends clean.

---

## 8. Project structure

```
lanlink/
├── Cargo.toml                # workspace (or go.work)
├── crates/
│   ├── lanlink-proto/        # wire format, control API types, versioning
│   ├── lanlink-crypto/       # noise channels, AEAD, argon2, replay window
│   ├── lanlink-tap/          # virtual NIC abstraction: TAP (Linux), Wintun (Windows), utun (macOS)
│   ├── lanlink-switch/       # MAC learning, flooding, chunker/reassembler
│   ├── lanlink-punch/        # STUN client, hole puncher, mapper (UPnP/PCP)
│   ├── lanlink-relay/        # content-blind UDP relay server library + binary
│   ├── lanlink-firewall/     # per-network inbound firewall profile
│   ├── lanlink-gateway/      # gateway mode: proxy-ARP, NAT, route push
│   ├── lanlinkd/             # daemon binary: wires everything; platform control sockets
│   ├── lanlinkctl/           # CLI binary
│   ├── lanlink-tui/          # terminal UI binary
│   ├── lanlink-web/          # browser UI binary
│   ├── lanlink-gui/          # unified GUI launcher (TUI, WebUI, or Desktop with one command)
│   ├── lanlink-desktop/      # native desktop UI using Tauri
│   └── coordinator/          # server binary (REST+WS), sqlite storage
├── proto/                    # .proto / schema definitions
├── dist/                     # systemd units, Windows service/MSI, macOS launchd/pkg, Docker
├── tests/                    # integration: netns topologies, Windows/macOS CI smoke tests
└── docs/                     # protocol.md, threat-model.md, ops runbooks
```

---

## 9. Roadmap & milestones

Each milestone is demoable and independently testable. Effort assumes 1 senior dev,
full-time (halve with AI-assisted workflow; scale with team).

| M | Status | Scope | Acceptance criteria | Effort |
|---|---|---|---|---|
| **M0** | ✅ Done | Spikes: TAP/Wintun/utun frame loopback, Noise handshake between two processes, STUN binding via coturn | Spike reports per OS; go/no-go on stack | 1–2 wk |
| **M1** | ✅ Done | Data plane, static config: two nodes, pre-shared keys, direct UDP, TAP up, ARP+ICMP works | `ping` between two `ip netns` over tunnel; `iperf3` ≥ 300 Mbps | 2–3 wks |
| **M2** | ✅ Done | Switch core: MAC learning, flooding, broadcast; chunking >PMTU; replay protection | LAN game discovery test (e.g., Warzone2100/Terraria server list) in netns lab; fuzz corpus on reassembler clean | 2–3 wks |
| **M3** | ✅ Done | Coordinator v1: create/join by name+password (Argon2id+cert), IP assignment, member list | Fresh VMs join by name; unauthorized join rejected; session survives coordinator restart | 2–3 wks |
| **M4** | ✅ Done | NAT traversal: STUN, simultaneous-open punch, port prediction, UPnP/PCP; automatic fallback | Connects through NAT lab: full-cone, restricted, port-restricted, symmetric+relay; metrics on punch success | 3–4 wks |
| **M5** | ✅ Done | Relay service (coturn or custom) + E2E-encrypted relaying; connection-type shown in CLI | Traffic flows with all UDP blocked; relay never sees plaintext (verified by capture) | 1–2 wks |
| **M6** | ✅ Done | Gateway mode (proxy-ARP, forwarding, NAT); public-network firewall profile (nftables) | Access physical-LAN printer/SMB behind remote gateway node; portscan from "public" peer blocked on 445 | 2 wks |
| **M7** | ✅ Done | Hardening & packaging: systemd units + Windows service + launchd, deb/rpm/AUR + MSI + `.pkg`, privilege separation (CAP_NET_ADMIN only / Windows elevated admin / macOS root), auto-update channel, crash-safe; soak + memory-audit harnesses ready | Clean installs on Linux/Windows/macOS; 72 h soak test with churn; valgrind/miri/ASAN clean | 3–4 wks |
| **M8** | ✅ Done | v1.0 polish: docs site, public directory server, telemetry-opt-in metrics; (stretch) GUI/TUI deferred | Public beta launch | 2–4 wks |

**Total: ~4–6 months** full-time for v1.0 parity feature set.

Post-1.0 candidates: multicast-group optimization, ACLs, SSO/private coordinators,
kernel fast path (Linux eBPF/XDP, Windows NDIS filter, macOS NetworkExtension),
QUIC-based relay, DNS names for peers, iOS/Android clients.

---

## 10. Testing strategy

- **Unit**: crypto (test vectors), replay window, MAC table aging, chunk reassembly
  (property-based + fuzz — this parser faces the Internet).
- **Integration (netns lab)**: scripts build topologies with `ip netns` + veth + nftables
  to emulate NAT types (full-cone via MASQUERADE+static mapping, symmetric via
  per-destination MASQUERADE) — assert punch success/fallback per topology.
- **System**: multi-node CI job — 3 peers, kill -9 a peer, coordinator restart,
  Wi-Fi↔wired interface switch (`ip link` flap) — connections must self-heal.
- **Performance**: `iperf3` + `pktgen` benchmarks in CI (nightly) with regression gates;
  criterion micro-benchmarks for hot path (per-frame cost budget < 2 µs).
- **Security**: `cargo audit` / `govulncheck`, fuzz targets in CI (oss-fuzz), threat-model
  review checkpoint at M4 and M7.
- **Interop/labs**: physical test with 2 real home routers + 1 CGNAT LTE hotspot before beta.
- **Cross-platform CI matrix**: Linux (netns lab), Windows (Hyper-V/VM lab), macOS (local or
  self-hosted runner) — at minimum a build + unit-test gate on each OS.

---

## 11. Threat model & security checklist

Assets: traffic confidentiality/integrity, network membership, node identity, coordinator DB.

- Off-path injection → rejected by AEAD + receiver-peer-id check.
- Replay → monotonic counter + sliding window.
- Pre-shared password compromise → Argon2id (high memory) + rate-limited join +
  per-network certs with revocation; optional OPAQUE PAKE (never send password).
- Malicious peer inside network → firewall profile (F11) + document trust model
  (like Radmin: members are mutually trusting).
- Relay compromise → no plaintext (E2E), plus pin relay identity.
- Coordinator compromise → cannot decrypt traffic (keys never leave peers);
  can DoS or impersonate join — mitigate with cert transparency-style logging later.
- DoS on daemon → packet parser fuzzed, rate limiting on handshake types,
  zero-copy bounded buffers, sandbox (systemd `ProtectSystem=strict`,
  `CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_RAW`).
- Key hygiene: sessions keys in mlock memory where feasible; zeroize on drop.

---

## 12. Performance engineering notes

- Batch TAP reads (`TUNSETVNETHDRSZ`+readv or io_uring / `TAP_RXRSS`-style queues) to cut syscalls.
- Per-peer send queues with a single UDP socket per network (not per peer) — matches NAT
  mapping behavior and punch model.
- Buffer pool; avoid per-frame allocation; AEAD in place.
- Optional SIMD (AES-NI via `aes-gcm`) — benchmark vs ChaCha20 on target CPUs.
- MTU: TAP 1500 default but advertise 1300+ until PMTU probe completes, then raise.
- NAPI-style budget: cap frames processed per event-loop tick to bound latency.

---

## 13. Packaging & distribution

- **Linux**: `deb` (Debian/Ubuntu), `rpm` (Fedora), AUR (Arch); static musl binary optional.
  systemd: `lanlinkd.service` (`Restart=always`, watchdog), socket-activated control API.
  Root via sudo/polkit rule for `lanlinkctl`; daemon holds `CAP_NET_ADMIN`.
- **Windows**: MSI installer bundling the Wintun driver + a Windows service wrapper
  (`lanlinkd service install/start/stop/uninstall`). Control socket over named pipe.
- **macOS**: `.pkg` installer + `launchd` plist; control socket over UNIX domain socket
  inside `/var/run/lanlink`.
- Docker image for coordinator/relay (self-host story).
- Optional built-in updater for tarball installs; distro packages update via package manager.

---

## 14. Legal & naming

- Do **not** use "Radmin", Famatech trademarks, or their assets. Clean-room: our plan is
  derived from public marketing/docs and general knowledge of VPN architecture, not from
  decompiling their binary. **Never reverse-engineer or link against the Radmin binary.**
- Interop goal: behave like Radmin (UX parity), not interoperate with Radmin networks.
- License suggestion for our code: Apache-2.0 (or GPLv3 if copyleft desired).
- Choose a neutral name (working name "LanLink" — check trademark before publishing).

---

## 15. Risks & mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Symmetric NAT / CGNAT punch failures | High | Relay fallback by design (M5); multiple relays; QUIC-over-443 escape hatch |
| Userspace TAP throughput ceiling | Medium | Perf budget from M1; io_uring/recv-mmsg batching; XDP fast path post-1.0 |
| Broadcast storms in big networks | Medium | Rate-limit floods; multicast groups post-1.0; cap network size v1 |
| Coordinator single point of failure | Medium | Sessions survive outage (§6.5); documented self-host |
| Rust NAT-traversal ecosystem gaps | Medium | coturn reuse; ICE-lite scope; Go fallback ladder (§7.3) |
| Scope creep (GUI, ACLs, kernel fast path) | High | Non-goals §5.3 enforced per milestone |
| Security bug in root daemon | Critical | Fuzzing, sandboxing, minimal privileges, external audit pre-beta |

---

## 16. Appendix

### 16.1 CLI UX sketch (parity with Radmin flows)

```
$ lanlinkctl network create gaming-night
Password: ••••••••
✔ Network 'gaming-night' created (26.12.34.0/24)
  You are 26.12.34.1 — share name+password with friends

$ lanlinkctl network join friend-lan
Password: ••••••••
✔ Joined 'friend-lan' as 26.77.0.14

$ lanlinkctl status
NETWORK        IP            PEERS  TYPE
gaming-night   26.12.34.1      4/6  p2p
friend-lan     26.77.0.14      1/1  relay

$ lanlinkctl peers gaming-night
PEER                  IP             PING   CONN     SEEN
alice-desktop         26.12.34.2     18 ms  direct   now
bob-steamdeck         26.12.34.3     42 ms  direct   now
carol-laptop          26.12.34.5     88 ms  relay    2s

$ lanlinkctl ping 26.12.34.3
pong from bob-steamdeck: seq=1 rtt=41.7 ms conn=direct
```

### 16.2 Definition of "done" for v1.0

Two friends on different home networks (one behind CGNAT) run:
`install → create/join by name+password → ping → host/join a LAN game → play`.
No terminal needed beyond install; relay fallback invisible unless inspected; traffic
E2E-encrypted; system survives reboots and ISP IP changes.

---

*Sources: radmin-vpn.com (main, /security/, /help/ pages, fetched Oct 2026); architecture
reconstructed from public documentation of comparable systems (ZeroTier, n2n, tinc,
Tailscale). Items marked "inferred" are engineering conclusions, not vendor statements.*
