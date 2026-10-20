//! Noise_XX handshake and transport session for pairwise LanLink encryption.
//!
//! Each peer uses the same Ed25519 identity key converted to X25519 for its
//! static Noise key. The handshake is authenticated by the static keys and
//! provides forward secrecy via ephemeral keys. After the handshake completes,
//! the negotiated transport state encrypts data frames using the standard
//! Noise framing (length-prefixed messages are handled by the caller).
//!
//! The wire layout on UDP/TCP is a normal LanLink outer header with the
//! [`FLAG_NOISE`] flag set, followed by the Noise handshake or transport
//! message. Handshake messages carry an 8-byte plaintext sender id prefix so
//! the receiver knows which peer key to use.

use crate::CryptoError;

/// Noise pattern used for pairwise sessions.
const NOISE_PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";

/// Maximum handshake message size (well below a UDP packet).
const MAX_HANDSHAKE_MSG_LEN: usize = 256;

/// Handshake message buffer size for transport messages. Noise adds a 16-byte
/// tag to each payload; we use a generous buffer for control and data frames.
const TRANSPORT_BUFFER_LEN: usize = 2048;

/// Result of feeding a handshake message into the state machine.
#[derive(Debug)]
pub enum HandshakeAction {
    /// Nothing to do (message consumed, waiting for the next one).
    None,
    /// A handshake message to send back to the peer. The optional session is
    /// present when the handshake completed as a side effect of producing the
    /// reply (e.g. the initiator after writing the third XX message).
    Send {
        message: Vec<u8>,
        session: Option<NoiseSession>,
    },
    /// The handshake completed successfully; the transport session is ready.
    Complete(NoiseSession),
}

/// A completed Noise transport session.
pub struct NoiseSession {
    state: snow::TransportState,
}

impl std::fmt::Debug for NoiseSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoiseSession")
            .field("initiator", &self.state.is_initiator())
            .field("sending_nonce", &self.state.sending_nonce())
            .field("receiving_nonce", &self.state.receiving_nonce())
            .finish()
    }
}

impl NoiseSession {
    /// Encrypt `plaintext` and return the Noise ciphertext. Each call
    /// increments the internal sending nonce.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let mut buf = vec![0u8; TRANSPORT_BUFFER_LEN];
        let n = self
            .state
            .write_message(plaintext, &mut buf)
            .map_err(CryptoError::Noise)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Decrypt a Noise ciphertext and return the plaintext.
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let mut buf = vec![0u8; TRANSPORT_BUFFER_LEN];
        let n = self
            .state
            .read_message(ciphertext, &mut buf)
            .map_err(CryptoError::Noise)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Return the remote static public key that authenticated this session,
    /// if the pattern provided it (it does for XX).
    pub fn remote_static(&self) -> Option<&[u8]> {
        self.state.get_remote_static()
    }
}

/// One side of an in-progress Noise_XX handshake.
#[derive(Debug)]
pub struct Handshake {
    state: Option<snow::HandshakeState>,
    is_initiator: bool,
    remote_public_key: Option<[u8; 32]>,
}

impl Handshake {
    /// Create a new initiator handshake. `local` is this device's identity,
    /// `remote_ed25519_public` is the peer's advertised Ed25519 public key.
    pub fn new_initiator(
        local: &crate::Identity,
        remote_ed25519_public: [u8; 32],
    ) -> Result<Self, CryptoError> {
        let x25519_secret = local.x25519_secret_bytes();
        let remote_x25519 = crate::ed25519_to_x25519_public(remote_ed25519_public)
            .ok_or(CryptoError::InvalidKey)?;

        let builder = snow::Builder::new(NOISE_PATTERN.parse().map_err(CryptoError::Noise)?)
            .local_private_key(&x25519_secret)
            .remote_public_key(&remote_x25519);
        let state = builder.build_initiator().map_err(CryptoError::Noise)?;
        Ok(Self {
            state: Some(state),
            is_initiator: true,
            remote_public_key: Some(remote_x25519),
        })
    }

    /// Create a new responder handshake.
    pub fn new_responder(local: &crate::Identity) -> Result<Self, CryptoError> {
        let x25519_secret = local.x25519_secret_bytes();
        let builder = snow::Builder::new(NOISE_PATTERN.parse().map_err(CryptoError::Noise)?)
            .local_private_key(&x25519_secret);
        let state = builder.build_responder().map_err(CryptoError::Noise)?;
        Ok(Self {
            state: Some(state),
            is_initiator: false,
            remote_public_key: None,
        })
    }

    fn state(&mut self) -> Result<&mut snow::HandshakeState, CryptoError> {
        self.state
            .as_mut()
            .ok_or(CryptoError::Malformed("handshake already complete"))
    }

    /// Produce the next handshake message to send, if any. For an initiator
    /// this is the first message; for a responder this is the reply.
    pub fn write_message(&mut self) -> Result<Vec<u8>, CryptoError> {
        let mut buf = vec![0u8; MAX_HANDSHAKE_MSG_LEN];
        let n = self
            .state()?
            .write_message(&[], &mut buf)
            .map_err(CryptoError::Noise)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Feed a handshake message from the peer. Returns the action to take:
    /// send a reply, complete the handshake, or wait.
    pub fn read_message(&mut self, msg: &[u8]) -> Result<HandshakeAction, CryptoError> {
        let mut payload = vec![0u8; MAX_HANDSHAKE_MSG_LEN];
        let finished_after_read = {
            let state = self.state()?;
            let _n = state
                .read_message(msg, &mut payload)
                .map_err(CryptoError::Noise)?;
            state.is_handshake_finished()
        };

        if finished_after_read {
            let state = self.state.take().expect("handshake state present");
            let transport = state.into_transport_mode().map_err(CryptoError::Noise)?;
            return Ok(HandshakeAction::Complete(NoiseSession { state: transport }));
        }

        // More handshake messages are required. Send the next one.
        let reply = self.write_message()?;
        let finished_after_write = self
            .state
            .as_ref()
            .is_some_and(|s| s.is_handshake_finished());

        if finished_after_write {
            let state = self.state.take().expect("handshake state present");
            let transport = state.into_transport_mode().map_err(CryptoError::Noise)?;
            return Ok(HandshakeAction::Send {
                message: reply,
                session: Some(NoiseSession { state: transport }),
            });
        }

        Ok(HandshakeAction::Send {
            message: reply,
            session: None,
        })
    }

    /// Consume this handshake and return the transport session. Call this after
    /// writing the final handshake message for the initiator, or after receiving
    /// the final handshake message for the responder.
    pub fn into_session(self) -> Result<NoiseSession, CryptoError> {
        let state = self
            .state
            .ok_or(CryptoError::Malformed("handshake state missing"))?
            .into_transport_mode()
            .map_err(CryptoError::Noise)?;
        Ok(NoiseSession { state })
    }

    /// Expected remote X25519 static key, set for initiators.
    pub fn remote_public_key(&self) -> Option<[u8; 32]> {
        self.remote_public_key
    }

    /// True if this handshake was started as the initiator.
    pub fn is_initiator(&self) -> bool {
        self.is_initiator
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_xx_handshake_roundtrip() {
        let alice = crate::Identity::generate();
        let bob = crate::Identity::generate();

        let mut alice_hs =
            Handshake::new_initiator(&alice, bob.public_key()).expect("alice initiator");
        let mut bob_hs = Handshake::new_responder(&bob).expect("bob responder");

        // -> e
        let m1 = alice_hs.write_message().unwrap();
        let action = bob_hs.read_message(&m1).unwrap();
        let m2 = match action {
            HandshakeAction::Send {
                message,
                session: None,
            } => message,
            other => panic!("expected responder reply, got {other:?}"),
        };

        // <- e, ee, s, es
        let action = alice_hs.read_message(&m2).unwrap();
        let (m3, mut alice_session) = match action {
            HandshakeAction::Send {
                message,
                session: Some(s),
            } => (message, s),
            other => panic!("initiator should send third message and complete, got {other:?}"),
        };

        // -> s, se
        let action = bob_hs.read_message(&m3).unwrap();
        let mut bob_session = match action {
            HandshakeAction::Complete(s) => s,
            other => panic!("bob should complete after m3, got {other:?}"),
        };

        let plaintext = b"hello noise";
        let ciphertext = alice_session.encrypt(plaintext).unwrap();
        let decrypted = bob_session.decrypt(&ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);

        let reply = bob_session.encrypt(b"ack").unwrap();
        assert_eq!(alice_session.decrypt(&reply).unwrap(), b"ack".as_slice());
    }
}
