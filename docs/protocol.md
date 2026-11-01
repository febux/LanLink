# Wire Protocol and Control API

This document describes the data-plane and control-plane protocols used by
LanLink.

## Data plane

The data plane is connectionless UDP. Every outer datagram has the same header:

```text
0               15 16            31
┌─────────────────┬───────────────┐
│ version | type │   reserved    │  4 bytes
├─────────────────┴───────────────┤
│ receiver node id (8 bytes)      │
├─────────────────────────────────┤
│ counter (8 bytes, big-endian)   │
├─────────────────────────────────┤
│ AEAD ciphertext                 │
│   - 12 byte nonce               │
│   - inner plaintext             │
│   - 16 byte Poly1305 tag        │
└─────────────────────────────────┘
```

- `version` is the current wire-protocol version (`1`).
- `type` identifies the packet purpose:
  - `1` — DATA
  - `2` — PING
  - `3` — PONG
  - `4` — PUNCH_PROBE
  - `5` — PUNCH_REPLY
  - `6` — KEEPALIVE
- `receiver node id` prevents a peer from decrypting traffic not addressed to it.
- `counter` is a per-sender monotonic value used as part of the nonce and for
  anti-replay protection.

The AEAD key is derived from the network password with Argon2id (network id
salted). Pairwise sessions will migrate to a Noise-based design in a future
release.

### Inner DATA packet layout

```text
[sender node id: 8 bytes]
[counter: 8 bytes BE]
[frame id: 4 bytes BE]
[chunk index: 2 bytes BE]
[total chunks: 2 bytes BE]
[chunk payload: variable]
```

Ethernet frames larger than the inner payload are split into chunks and
reassembled by the receiver. The default outer MTU budget is 1300 bytes, leaving
headroom for UDP/IP headers and avoiding IPv4 fragmentation.

### Inner control-probe layout (PING / PONG / KEEPALIVE)

Keepalives and latency probes carry a 24-byte inner payload:

```text
[sender node id: 8 bytes]
[counter: 8 bytes BE]
[timestamp: 8 bytes BE, milliseconds since the sender's daemon start]
```

- The same per-sender counter stream is shared with DATA packets, so probes
  are replay-protected like any other packet.
- KEEPALIVEs refresh peer liveness and learn NAT rebinds; the timestamp is
  zero and ignored.
- A PING receiver immediately answers with a PONG echoing the timestamp
  verbatim. Because the timestamp came from the pinger's own monotonic
  clock, the RTT is simply `now - timestamp` — clock skew between peers is
  irrelevant. The daemon probes each known peer every 5 seconds so the
  local status API (below) always has fresh latency numbers.

## NAT traversal

1. The daemon gathers **host candidates** from local interfaces and
   **server-reflexive candidates** from a configured STUN server.
2. Candidates are submitted to the coordinator via `POST /networks/:id/candidates`.
3. The coordinator returns the latest peer list including each peer's
   candidates.
4. The daemon performs simultaneous UDP sends (**hole punching**) to every
   candidate pair.
5. A verified reply updates the switch's peer address.
6. If direct punching fails, the daemon falls back to a **relay candidate**.
   Traffic remains E2E-encrypted; the relay only routes by the plaintext receiver
   node id.

## Control plane

The coordinator exposes a JSON REST API:

| Method | Path | Purpose |
|---|---|---|
| POST | `/networks` | Create a network. |
| POST | `/networks/:id/join` | Join a network with name+password. |
| GET | `/networks/:id/members` | List current members and candidates (requires network password). |
| POST | `/networks/:id/candidates` | Update local candidates, fetch peer list. |
| GET | `/directory` | List public/gaming networks. |
| POST | `/telemetry` | Submit an opt-in anonymized telemetry report. |
| GET | `/metrics` | Return aggregated telemetry for operators. |

All control-plane messages are ordinary HTTPS. Future releases may add mTLS or
Noise authentication between daemon and coordinator.

## Local status API

Each `lanlinkd run` serves a read-only status snapshot over a local Unix
domain socket (a named-pipe equivalent is planned for Windows):

- Default path: `/run/lanlink/lanlinkd-<network_id>.sock` (mode 0660), with a
  `/tmp/lanlinkd-<network_id>.sock` fallback when `/run` is not writable.
  Override with the `--status-socket` flag or the `[status] socket` config key.
- Protocol: newline-delimited JSON. The client writes one request per line and
  may half-close after the last one; the daemon answers each request line with
  exactly one response line. Connections may stay open for repeated polls.

```json
> {"cmd":"snapshot"}
< {"status":{"version":"0.1.0","network_id":1,"assigned_ip":"26.0.0.2",...},
   "peers":[{"node_id":"...","virtual_ip":"26.0.0.3","path":"direct",
             "latency_ms":12,"last_seen_secs_ago":1,...}]}
```

`path` is one of `direct`, `relay`, `punching`, or `waiting`. The snapshot
carries no secrets — no passwords or key material. `lanlink-tui` (and
`socat - UNIX-CONNECT:<path>`) consume this API.
