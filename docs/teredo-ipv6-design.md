# Design: Teredo/IPv6 discovery phase for IPv4-only peers behind restrictive NATs

Status: design (windows-parity task 4.1 — deferred stretch). No implementation
yet; this document is the agreed design record a future change will implement
against.

## Problem

`lanlink-punch` gathers host and server-reflexive (STUN) candidates and
punches UDP pairs. This fails when both peers sit behind **symmetric NATs**
(each outbound flow maps to a different public port): the port observed by the
STUN server is not the port that will map traffic to the peer, so simultaneous
open never converges. Today such pairs fall back to the relay (content-blind
but bandwidth-limited) or the TCP fallback (high latency for games).

## Chosen direction: Teredo-style IPv6 tunnel as a candidate transport

Rather than implementing full Teredo (RFC 4380) client semantics with a
public Teredo server/relay infrastructure, LanLink adds an **IPv6-over-UDP
virtual candidate**: each peer derives a stable IPv6 address (ULA, fd26::/8,
from its node id) and, when IPv4 punching fails, peers exchange IPv6 packets
tunneled over the same UDP socket toward the peer's Teredo-ish candidate
(server-reflexive address + assigned port). A well-known open Teredo
infrastructure is **not** a goal; where possible, NAT64/real IPv6 is used
directly.

Phases:

1. **Detection.** After step 2 of the punch lifecycle, classify NAT type from
   the STUN responses (full-cone / restricted / port-restricted / symmetric —
   needs a second STUN query from a second local port to detect symmetry).
   If both peers report symmetric NAT and no relay is preferred, schedule the
   IPv6 discovery phase in parallel with relay fallback.
2. **Address assignment.** Coordinator issues each node a ULA
   `fd26:<net>:<node>::/64` derived deterministically from network id + node
   id (no new coordination state). These addresses are additional
   `CandidateKind::Ipv6` candidates in the exchange.
3. **Native path.** If both peers have global IPv6 (checked via a second STUN
   family query), connect directly over IPv6 UDP — symmetric NAT is usually
   irrelevant on IPv6 (no NAPT in practice).
4. **Tunneled path.** Otherwise, encapsulate the encrypted LanLink packet
   (the switch already hands over opaque encrypted frames) in a UDP/IPv4
   datagram addressed at the peer's reflexive candidate. Because both sides
   send from the same socket that performed the STUN query, port-restricted
   cones still pass; for true symmetric NAT this converges only if the two
   peers' mappings happen to be stable per destination — so the relay stays
   the final fallback. Honest expectation: covers port-restricted + IPv6
   capable pairs, not all symmetric pairs.
5. **Data path.** The Wintun/TAP layer already routes `fd26::/64` inside the
   virtual network; `lanlink-switch` gains an IPv6 ethertype branch; no
   application changes.

## Security

- All tunneled traffic is the existing end-to-end encrypted switch payload;
  the tunnel adds no new plaintext.
- ULA addresses derive from node id + network PSK salt, are not routable
  outside the virtual network, and are never announced beyond the coordinator.
- Punch probes stay authenticated (existing PUNCH_PROBE MAC) to prevent
  address-spoofed injection into the tunnel.

## Milestones (multi-week effort)

1. NAT classification in `lanlink-punch` + candidate kind `Ipv6` (proto
   change, coordinator pass-through).
2. Native IPv6 direct path + ULA assignment in `lanlinkd`/`lanlink-tap`.
3. UDP tunnel encapsulation for IPv4 symmetric pairs; metric wiring
   (`lanlinkctl status` shows per-peer path: direct-v6 / tunneled / relay / tcp).
4. Load/latency testing vs relay baseline; document adoption criteria.

## Open questions

- Teredo public infrastructure reuse (e.g. publicly available servers) vs
  self-hosted relay-with-encapsulation: reuse leaks timing metadata;
  self-hosting adds ops burden. Default: self-hosted encapsulation only.
- Whether coordinator should prefer IPv6 candidates ahead of IPv4 reflexive
  (ordering policy).
