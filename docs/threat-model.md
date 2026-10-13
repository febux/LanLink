# Threat Model and Privacy

## Assets

- Traffic confidentiality and integrity between peers.
- Network membership (who can join a name+password network).
- Per-node identity.
- Coordinator database.

## Trust boundaries

- **Peers inside the same network** mutually trust each other, just as on a real
  LAN. This is the Radmin VPN model.
- **The coordinator** is trusted for membership and address signaling, but it
  is **not** trusted with decryption keys or traffic content.
- **Relay servers** are trusted only to forward UDP datagrams. They never see
  plaintext because all traffic is E2E-encrypted.

## Mitigations

| Threat | Mitigation |
|---|---|
| Off-path packet injection | AEAD authentication plus receiver node id check. |
| Replay | Monotonic counter + sliding replay window per peer. |
| Password compromise | Argon2id stretching; per-network certificates planned. |
| Malicious peer inside network | Public-network firewall blocks high-risk inbound ports. |
| Relay compromise | E2E encryption; relay only routes by receiver id. |
| Coordinator compromise | Cannot decrypt traffic; keys never leave peers. |
| DoS on daemon | Fuzz-tested parsers, bounded buffers, minimal privileges. |

## Privacy

- **No traffic logging** by default.
- **Optional debug logging** can be enabled locally; it never includes plaintext
  frames or private keys.
- **Telemetry is opt-in and anonymous.** When enabled, the daemon sends only:
  version, platform, uptime, peer count, direct/relay split, gateway/firewall
  flags, and byte counters. No node id, network id, network name, IP address, or
  traffic content is transmitted.
- The update channel is also opt-in. The daemon never phones home unless the
  operator explicitly enables it.

## Key hygiene

Session keys are held in process memory and discarded on rekey/shutdown. Future
releases will use `mlock`-protected key memory where supported by the platform.
