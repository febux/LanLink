# Noise / Ed25519 identity migration plan

## Goal

Replace the single network-wide PSK with per-device identity keys and pairwise Noise
sessions. This gives LanLink:

- Per-peer authentication (a peer proves it owns its node id).
- Forward secrecy for each session.
- Resistance to offline PSK cracking: an attacker who learns one peer's key cannot decrypt
  traffic between other peers.

## Current state

- The network password is stretched with Argon2id to produce a single 256-bit PSK.
- Every peer in the network uses the same PSK to AEAD-seal frames to every other peer.
- `JoinNetworkRequest` already has a `public_key: Vec<u8>` field, but it is unused.
- The `NodeId` type is an 8-byte identifier, currently random per run.

## Target design

1. **Device identity**
   - Each installation generates an Ed25519 signing keypair once and stores it in
     `%APPDATA%/LanLink/identity.key` (Windows) or `/etc/lanlink/identity.key` (Linux).
   - The `NodeId` becomes the first 8 bytes of the Ed25519 public key (or a hash of it).
   - The public key is advertised to the coordinator in `JoinNetworkRequest.public_key`.

2. **Network membership still gated by password**
   - The coordinator verifies the password with Argon2id as today.
   - The password is no longer the data-plane key; it only authorizes membership.

3. **Pairwise Noise sessions**
   - When peer A learns peer B's public key from the coordinator, it initiates a Noise_XX
     or Noise_IK handshake over the existing UDP path.
   - The handshake is authenticated by the Ed25519 static keys (translated to X25519 for
     Noise).
   - After the handshake, data frames use the negotiated per-session symmetric keys.

4. **Backward compatibility**
   - Phase 1: keep the PSK as a fallback. Peers that do not yet advertise a public key use
     PSK mode.
   - Phase 2: once all clients support Noise, disable PSK mode for new networks.

## Protocol changes

- `NodeId` definition stays 8 bytes, but its value should be derived from the public key.
- `JoinNetworkRequest.public_key` becomes required and carries the 32-byte Ed25519 public key.
- Add a `NoiseHandshake` packet type and inner payload format.
- `PeerInfo` should include `public_key: Vec<u8>`.
- The coordinator should persist the public key per member.

## Crypto implementation

- Use the `snow` crate (already in workspace dependencies) for the Noise framework.
- Use `ed25519-dalek` and `x25519-dalek` (already in workspace dependencies) for keys.
- Convert Ed25519 public keys to X25519 for Noise using `ed25519-dalek` +
  `curve25519-dalek` or `dalek-xEd25519` patterns.

## Storage

- The private key must be stored with restrictive permissions (0600 on Unix).
- Key generation on first run; never regenerate unless the user explicitly requests it.

## Migration order

1. Generate and persist Ed25519 identity keys in `lanlinkd`.
2. Send public key to coordinator and store it in `PeerInfo`.
3. Implement Noise handshake over UDP.
4. Implement Noise-encrypted data frames.
5. Keep PSK mode as a compatibility fallback.
6. Add a network-level flag to require Noise-only peers.

## Security notes

- The coordinator remains untrusted for traffic confidentiality. It only binds names,
  passwords, public keys, and addresses.
- Argon2id is still useful for rate-limiting membership attempts.
- Identity private keys must never leave the device.
