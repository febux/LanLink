//! Per-device Ed25519 identity and X25519 conversion for Noise.
//!
//! The identity key is generated once per installation and persisted with
//! restrictive file permissions. The 8-byte [`NodeId`] is derived from the
//! first 8 bytes of the Ed25519 public key, so a peer that knows our public
//! key also knows our node id.

use std::path::{Path, PathBuf};

pub const ED25519_PUBLIC_KEY_LEN: usize = 32;
pub const ED25519_SECRET_KEY_LEN: usize = 32;

/// A device identity: Ed25519 signing key plus derived identifiers.
#[derive(Debug, Clone)]
pub struct Identity {
    signing_key: ed25519_dalek::SigningKey,
}

impl Identity {
    /// Generate a new random identity.
    pub fn generate() -> Self {
        Self {
            signing_key: ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng),
        }
    }

    /// Load a 32-byte Ed25519 secret key from bytes.
    pub fn from_secret_bytes(bytes: [u8; ED25519_SECRET_KEY_LEN]) -> Self {
        Self {
            signing_key: ed25519_dalek::SigningKey::from(bytes),
        }
    }

    /// The 32-byte Ed25519 public key.
    pub fn public_key(&self) -> [u8; ED25519_PUBLIC_KEY_LEN] {
        self.signing_key.verifying_key().to_bytes()
    }

    /// The 32-byte Ed25519 secret key.
    pub fn secret_key_bytes(&self) -> [u8; ED25519_SECRET_KEY_LEN] {
        self.signing_key.to_bytes()
    }

    /// The 8-byte node id derived from the public key.
    pub fn node_id(&self) -> lanlink_proto::NodeId {
        let pk = self.public_key();
        let mut id = [0u8; lanlink_proto::NODE_ID_LEN];
        id.copy_from_slice(&pk[..lanlink_proto::NODE_ID_LEN]);
        lanlink_proto::NodeId(id)
    }

    /// Convert this Ed25519 keypair to an X25519 static keypair. The scalar
    /// used here is compatible with the Montgomery form of the public key,
    /// matching the pattern used by Signal/wireguard-go.
    pub fn to_x25519_keypair(&self) -> ([u8; 32], [u8; 32]) {
        let scalar = self.signing_key.to_scalar_bytes();
        let static_secret = x25519_dalek::StaticSecret::from(scalar);
        let public_key = x25519_dalek::PublicKey::from(&static_secret);
        (static_secret.to_bytes(), public_key.to_bytes())
    }

    /// Return the X25519 static secret bytes (the scalar).
    pub fn x25519_secret_bytes(&self) -> [u8; 32] {
        self.signing_key.to_scalar_bytes()
    }

    /// Return the X25519 public key bytes (Montgomery form of the Ed25519 key).
    pub fn x25519_public_bytes(&self) -> [u8; 32] {
        let montgomery = self.signing_key.verifying_key().to_montgomery();
        montgomery.0
    }
}

/// Convert a 32-byte Ed25519 public key to the corresponding X25519 public key.
pub fn ed25519_to_x25519_public(ed25519_public: [u8; 32]) -> Option<[u8; 32]> {
    let verifying = ed25519_dalek::VerifyingKey::from_bytes(&ed25519_public).ok()?;
    Some(verifying.to_montgomery().0)
}

/// Platform-specific path for the persistent identity key.
#[cfg(unix)]
pub fn default_identity_path() -> PathBuf {
    PathBuf::from("/etc/lanlink/identity.key")
}

#[cfg(windows)]
pub fn default_identity_path() -> PathBuf {
    let appdata = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\\ProgramData"));
    appdata.join("LanLink").join("identity.key")
}

/// Load an existing identity from `path`, or create and persist a new one if
/// the file does not exist or is unreadable.
pub fn load_or_create(path: &Path) -> anyhow::Result<Identity> {
    match std::fs::read(path) {
        Ok(bytes) => {
            if bytes.len() != ED25519_SECRET_KEY_LEN {
                anyhow::bail!(
                    "identity file {} has wrong length (expected {}, got {})",
                    path.display(),
                    ED25519_SECRET_KEY_LEN,
                    bytes.len()
                );
            }
            let mut secret = [0u8; ED25519_SECRET_KEY_LEN];
            secret.copy_from_slice(&bytes);
            Ok(Identity::from_secret_bytes(secret))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let identity = Identity::generate();
            identity.save(path)?;
            Ok(identity)
        }
        Err(e) => Err(anyhow::anyhow!(
            "failed to read identity file {}: {e}",
            path.display()
        )),
    }
}

impl Identity {
    /// Persist the secret key to `path` with restrictive permissions.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Write to a temporary file and rename so the key is never half-written.
        let tmp = path.with_extension("key.tmp");
        std::fs::write(&tmp, self.signing_key.to_bytes())?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&tmp, perms)?;
        }

        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_is_public_key_prefix() {
        let id = Identity::generate();
        let pk = id.public_key();
        assert_eq!(id.node_id().0, pk[..8]);
    }

    #[test]
    fn x25519_keypair_matches() {
        let id = Identity::generate();
        let (secret, public) = id.to_x25519_keypair();
        let recomputed = x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(secret));
        assert_eq!(recomputed.to_bytes(), public);
    }

    #[test]
    fn ed25519_to_x25519_matches_identity() {
        let id = Identity::generate();
        assert_eq!(
            ed25519_to_x25519_public(id.public_key()).unwrap(),
            id.x25519_public_bytes()
        );
    }

    #[test]
    fn load_or_create_roundtrip() {
        let dir =
            std::env::temp_dir().join(format!("lanlink-identity-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("identity.key");
        let id = load_or_create(&path).unwrap();
        let id2 = load_or_create(&path).unwrap();
        assert_eq!(id.public_key(), id2.public_key());
        std::fs::remove_dir_all(&dir).ok();
    }
}
