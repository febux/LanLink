# Proposal

## Why

LanLink originally sealed all data-plane frames with a single network-wide PSK derived (Argon2id) from the network password. That design offered no per-peer authentication, no forward secrecy, and offline PSK cracking exposed all peer traffic. A per-device identity + pairwise Noise sessions model removes those weaknesses.

## What Changes

- Replace the single network-wide PSK with per-device Ed25519 identity keys and pairwise Noise sessions.
- `NodeId` becomes derived from the Ed25519 public key; `JoinNetworkRequest.public_key` becomes required; `PeerInfo` carries public keys persisted by the coordinator.
- Password keeps gating membership only; PSK mode retained as a compatibility fallback.

## Capabilities

Implemented and shipped (see git `feat(tcp,noise): implement TCP direct/relay fallback and per-device Noise identity`); archived historical change, `skip_specs: true`.

### New Capabilities

- None.

### Modified Capabilities

- None.

## Impact

- `lanlink-crypto` (Noise sessions), `lanlink-proto` (protocol fields), `lanlinkd` (identity key storage), coordinator (public key persistence).
