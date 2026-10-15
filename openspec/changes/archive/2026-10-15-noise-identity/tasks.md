# Tasks

Migration order from the original plan (`docs/noise-identity-plan.md`), all shipped:

- [x] 1 Generate and persist Ed25519 identity keys in `lanlinkd`
- [x] 2 Send public key to coordinator and store it in `PeerInfo`
- [x] 3 Implement Noise handshake over UDP
- [x] 4 Implement Noise-encrypted data frames
- [x] 5 Keep PSK mode as a compatibility fallback
- [x] 6 Add a network-level flag to require Noise-only peers
