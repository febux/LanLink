//! Noise/AEAD channels and key derivation for LanLink.
//!
//! Design: password -> network PSK via Argon2id (salted by network id).
//! Per-node identity: Ed25519 signing + X25519 static DH.
//! Pairwise sessions: Noise `Noise_IKpsk2` with ChaCha20-Poly1305, replay window.
//!
//! For the M2 milestone the data plane uses a shared network key to seal packets.
//! Each outer datagram is authenticated with the receiver node id as additional
//! data; the inner plaintext carries the sender node id, a per-sender counter, and
//! the Ethernet frame (or fragment).  A small sliding-window replay check is
//! performed by the switch core using the counter once the packet is decrypted.

use thiserror::Error;

pub mod identity;
pub mod noise;

pub use identity::{default_identity_path, ed25519_to_x25519_public, load_or_create, Identity};
pub use noise::{Handshake, HandshakeAction, NoiseSession};

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("noise protocol error")]
    Noise(#[from] snow::Error),
    #[error("invalid key length")]
    InvalidKey,
    #[error("encryption failed")]
    Encrypt,
    #[error("decryption failed")]
    Decrypt,
    #[error("malformed packet: {0}")]
    Malformed(&'static str),
    #[error("key derivation failed")]
    KeyDerivation,
}

/// Derive a 32-byte network PSK from a user password.
///
/// The salt is derived from the public `network_id` so that:
/// 1. All peers with the same password and network id derive the same key without
///    requiring a separate secure salt distribution channel.
/// 2. Identical passwords on different networks do not produce the same PSK.
///
/// # Security/threat-model note
///
/// The salt is **public by design**. Anyone who knows the `network_id` can compute
/// the exact salt used here. The security of the derived key therefore rests on the
/// password and on the Argon2id work factor, not on salt secrecy. This is the same
/// threat model as any password-based KDF with a public, context-specific salt.
///
/// A predictable salt allows an attacker to pre-compute a rainbow table for a
/// specific `network_id` *before* learning the password. The Argon2id parameters
/// (64 MiB, 3 iterations, 4 lanes) are chosen to make such offline attacks costly.
/// If stronger resistance to precomputation is required, the coordinator should
/// generate a random per-network salt and distribute it to peers alongside the
/// network id, and this function should be updated to accept that salt.
pub fn derive_network_psk(password: &str, network_id: u64) -> Result<[u8; 32], CryptoError> {
    use argon2::{password_hash::SaltString, Argon2, Params, PasswordHasher};

    let salt = SaltString::encode_b64(format!("{network_id:032x}").as_bytes())
        .map_err(|_| CryptoError::KeyDerivation)?;

    let argon2 = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        Params::new(64 * 1024, 3, 4, Some(32)).map_err(|_| CryptoError::KeyDerivation)?,
    );

    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| CryptoError::KeyDerivation)?;

    let mut out = [0u8; 32];
    out.copy_from_slice(hash.hash.ok_or(CryptoError::KeyDerivation)?.as_bytes());
    Ok(out)
}

/// Shared-network-key AEAD session used for early milestones (M1/M2).
///
/// Packet layout:
/// ```text
/// [version: u8]
/// [type: u8]
/// [flags: u8]
/// [reserved: u8]
/// [receiver node id: 8 bytes]      -- AAD, allows early drop if not for us
/// [nonce: 12 bytes]                 -- first 4 bytes = sender id prefix,
///                                        last 8 bytes = per-sender counter
///                                        (big-endian, monotonically increasing)
/// [ciphertext + 16-byte Poly1305 tag]
/// ```
/// The 4-byte header and the receiver id are used as additional authenticated data.
///
/// The inner plaintext is expected to start with the sender node id followed by
/// an 8-byte per-sender counter. The counter is reused as the low 8 bytes of the
/// AEAD nonce; the sender id prefix prevents nonce collisions between different
/// senders under the shared network key.
#[derive(Clone)]
pub struct StaticSession {
    cipher: chacha20poly1305::ChaCha20Poly1305,
}

impl StaticSession {
    pub fn new(key: [u8; 32]) -> Self {
        use chacha20poly1305::KeyInit;
        Self {
            cipher: chacha20poly1305::ChaCha20Poly1305::new(&key.into()),
        }
    }

    /// Encrypt a plaintext inner packet for `receiver`.
    ///
    /// `plaintext` must begin with `[sender node id: 8 bytes][counter: 8 bytes]`;
    /// the counter is used as the low 8 bytes of the AEAD nonce and must increase
    /// monotonically for each sender.
    pub fn seal(
        &self,
        receiver: lanlink_types::NodeId,
        packet_type: u8,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        use aead::{Aead, Payload};

        let min_inner = lanlink_types::NODE_ID_LEN + 8;
        if plaintext.len() < min_inner {
            return Err(CryptoError::Malformed(
                "plaintext too short for sender id and counter",
            ));
        }

        let mut nonce = [0u8; 12];
        nonce[..4].copy_from_slice(&plaintext[..4]); // sender id prefix
        nonce[4..].copy_from_slice(&plaintext[lanlink_types::NODE_ID_LEN..min_inner]); // counter

        let header = [
            lanlink_types::WIRE_PROTOCOL_VERSION,
            packet_type,
            0x00,
            0x00,
        ];
        let mut aad = Vec::with_capacity(header.len() + lanlink_types::NODE_ID_LEN);
        aad.extend_from_slice(&header);
        aad.extend_from_slice(&receiver.0);

        let ciphertext = self
            .cipher
            .encrypt(
                (&nonce).into(),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::Encrypt)?;

        let mut packet =
            Vec::with_capacity(4 + lanlink_types::NODE_ID_LEN + nonce.len() + ciphertext.len());
        packet.extend_from_slice(&header);
        packet.extend_from_slice(&receiver.0);
        packet.extend_from_slice(&nonce);
        packet.extend_from_slice(&ciphertext);
        Ok(packet)
    }

    /// Decrypt an outer packet, returning `(receiver, packet_type, plaintext)`.
    ///
    /// The caller should check that `receiver` matches the local node id and then
    /// inspect the inner plaintext for the real sender id and counter.
    pub fn open(&self, packet: &[u8]) -> Result<(lanlink_types::NodeId, u8, Vec<u8>), CryptoError> {
        use aead::{Aead, Payload};

        let min_len = 4 + lanlink_types::NODE_ID_LEN + 12 + 16;
        if packet.len() < min_len {
            return Err(CryptoError::Malformed("packet too short"));
        }

        if packet[0] != lanlink_types::WIRE_PROTOCOL_VERSION {
            return Err(CryptoError::Malformed("unknown wire protocol version"));
        }

        let packet_type = packet[1];
        let header = &packet[0..4];
        let receiver_bytes = &packet[4..4 + lanlink_types::NODE_ID_LEN];
        let nonce = &packet[4 + lanlink_types::NODE_ID_LEN..4 + lanlink_types::NODE_ID_LEN + 12];
        let ciphertext = &packet[4 + lanlink_types::NODE_ID_LEN + 12..];

        let mut receiver = [0u8; lanlink_types::NODE_ID_LEN];
        receiver.copy_from_slice(receiver_bytes);

        let mut aad = Vec::with_capacity(header.len() + lanlink_types::NODE_ID_LEN);
        aad.extend_from_slice(header);
        aad.extend_from_slice(receiver_bytes);

        let plaintext = self
            .cipher
            .decrypt(
                nonce.into(),
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::Decrypt)?;

        Ok((lanlink_types::NodeId(receiver), packet_type, plaintext))
    }
}

/// A 64-packet sliding replay window.
///
/// Counters are expected to increase monotonically (with some reorder tolerance).
/// Packets with a counter that has already been seen, or that fall below the
/// current window, are rejected.
#[derive(Debug, Clone, Copy)]
pub struct ReplayWindow {
    base: u64,
    bitmap: u64,
}

impl ReplayWindow {
    pub fn new() -> Self {
        Self { base: 0, bitmap: 0 }
    }

    /// Returns `true` iff the counter is accepted and the window is updated.
    pub fn accept(&mut self, counter: u64) -> bool {
        if counter <= self.base {
            return false;
        }
        let offset = counter - self.base; // 1-based offset into the bitmap
        if offset <= 64 {
            let bit = 1u64 << (offset - 1);
            if self.bitmap & bit != 0 {
                return false;
            }
            self.bitmap |= bit;
            true
        } else {
            // Counter jumped ahead more than the window width. Slide the window
            // forward so the new counter becomes the highest entry.
            let shift = offset - 64;
            self.bitmap = (self.bitmap >> shift) | (1u64 << 63);
            self.base = counter - 64;
            true
        }
    }
}

impl Default for ReplayWindow {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_password_same_network_same_psk() {
        let a = derive_network_psk("hunter2", 42).unwrap();
        let b = derive_network_psk("hunter2", 42).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn different_network_different_psk() {
        let a = derive_network_psk("hunter2", 42).unwrap();
        let b = derive_network_psk("hunter2", 43).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn static_session_roundtrip() {
        let key = derive_network_psk("test-password", 1).unwrap();
        let alice = StaticSession::new(key);
        let bob = StaticSession::new(key);

        let sender = lanlink_types::NodeId([3u8; 8]);
        let receiver = lanlink_types::NodeId([7u8; 8]);
        let counter = 1u64;
        let frame = [
            0x02, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x02, 0x08, 0x00,
            0x45,
        ];
        let mut inner = Vec::with_capacity(lanlink_types::NODE_ID_LEN + 8 + frame.len());
        inner.extend_from_slice(&sender.0);
        inner.extend_from_slice(&counter.to_be_bytes());
        inner.extend_from_slice(&frame);

        let packet = alice
            .seal(receiver, lanlink_types::DATA_PACKET_TYPE, &inner)
            .unwrap();
        let (got_receiver, got_type, out) = bob.open(&packet).unwrap();
        assert_eq!(got_receiver, receiver);
        assert_eq!(got_type, lanlink_types::DATA_PACKET_TYPE);
        assert_eq!(out, inner);
    }

    #[test]
    fn tampered_packet_fails() {
        let key = derive_network_psk("test-password", 1).unwrap();
        let alice = StaticSession::new(key);
        let bob = StaticSession::new(key);

        let sender = lanlink_types::NodeId([3u8; 8]);
        let receiver = lanlink_types::NodeId([7u8; 8]);
        let counter = 42u64;
        let payload = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
        let mut inner = Vec::with_capacity(lanlink_types::NODE_ID_LEN + 8 + payload.len());
        inner.extend_from_slice(&sender.0);
        inner.extend_from_slice(&counter.to_be_bytes());
        inner.extend_from_slice(&payload);

        let mut packet = alice
            .seal(receiver, lanlink_types::DATA_PACKET_TYPE, &inner)
            .unwrap();
        let last = packet.len() - 1;
        packet[last] ^= 0xff;

        assert!(bob.open(&packet).is_err());
    }

    #[test]
    fn seal_rejects_plaintext_without_sender_counter() {
        let key = derive_network_psk("test-password", 1).unwrap();
        let s = StaticSession::new(key);
        let receiver = lanlink_types::NodeId([7u8; 8]);
        let short = [0u8; 15];
        assert!(s
            .seal(receiver, lanlink_types::DATA_PACKET_TYPE, &short)
            .is_err());
    }

    #[test]
    fn nonce_includes_sender_prefix_and_counter() {
        let key = derive_network_psk("test-password", 1).unwrap();
        let s = StaticSession::new(key);
        let sender = lanlink_types::NodeId([0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a]);
        let receiver = lanlink_types::NodeId([7u8; 8]);
        let counter = 0x0102030405060708u64;
        let mut inner = Vec::with_capacity(lanlink_types::NODE_ID_LEN + 8);
        inner.extend_from_slice(&sender.0);
        inner.extend_from_slice(&counter.to_be_bytes());

        let packet = s
            .seal(receiver, lanlink_types::DATA_PACKET_TYPE, &inner)
            .unwrap();
        let nonce_start = 4 + lanlink_types::NODE_ID_LEN;
        let nonce = &packet[nonce_start..nonce_start + 12];
        assert_eq!(&nonce[..4], &sender.0[..4]);
        assert_eq!(&nonce[4..], &counter.to_be_bytes());
    }

    #[test]
    fn replay_window_accepts_in_order() {
        let mut w = ReplayWindow::new();
        assert!(w.accept(1));
        assert!(w.accept(2));
        assert!(w.accept(3));
        assert!(!w.accept(2)); // replay
        assert!(!w.accept(1)); // below window
    }

    #[test]
    fn replay_window_accepts_limited_reorder() {
        let mut w = ReplayWindow::new();
        assert!(w.accept(10));
        assert!(w.accept(12));
        assert!(w.accept(11)); // within window
        assert!(!w.accept(10)); // replay
    }

    #[test]
    fn replay_window_drops_very_old() {
        let mut w = ReplayWindow::new();
        assert!(w.accept(100));
        assert!(!w.accept(1)); // far below window
    }

    #[test]
    fn replay_window_regression_large_jump_keeps_history() {
        let mut w = ReplayWindow::new();
        assert!(w.accept(64));
        assert!(w.accept(65));
        assert!(!w.accept(64)); // would re-accept without the right-shift fix
    }
}
