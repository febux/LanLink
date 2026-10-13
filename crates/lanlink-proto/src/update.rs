//! Update-channel types: the release manifest format and version comparison.
//!
//! The daemon and `lanlinkctl` fetch a small JSON manifest from a configurable
//! HTTPS URL and compare its version against the running build. Checking is
//! **opt-in and disabled by default** — LanLink does not phone home unless the
//! operator configures it (PLAN.md §5.2 "Auditability").
//!
//! Manifest format (host it anywhere static — the release bucket, a CDN, or
//! your coordinator):
//!
//! ```json
//! {
//!   "product": "lanlink",
//!   "version": "1.2.0",
//!   "released": "2026-10-11",
//!   "notes": "Relay path stability fixes.",
//!   "platforms": {
//!     "x86_64-unknown-linux-musl": {
//!       "url": "https://dl.example.com/lanlink-1.2.0-x86_64-linux.tar.zst",
//!       "sha256": "…",
//!       "size": 12345678
//!     }
//!   }
//! }
//! ```
//!
//! The updater never executes anything from the manifest; installing is left
//! to the platform package manager (deb/rpm/MSI/pkg) or the operator.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Default public update manifest URL (opt-in; used only when enabled).
pub const DEFAULT_MANIFEST_URL: &str = "https://updates.lanlink.dev/stable/manifest.json";

/// A per-platform downloadable artifact described by the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateArtifact {
    /// Download URL for the installer/archive.
    pub url: String,
    /// Hex-encoded SHA-256 of the artifact, if published.
    #[serde(default)]
    pub sha256: Option<String>,
    /// Artifact size in bytes, if published.
    #[serde(default)]
    pub size: Option<u64>,
}

/// The release manifest served on the update channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateManifest {
    /// Product name; clients should verify it matches `"lanlink"`.
    pub product: String,
    /// Latest released version (`major.minor.patch`, optional leading `v`).
    pub version: String,
    /// Release date (informational, ISO 8601).
    #[serde(default)]
    pub released: Option<String>,
    /// Short human-readable release notes.
    #[serde(default)]
    pub notes: Option<String>,
    /// Artifacts keyed by Rust target triple (e.g. `x86_64-unknown-linux-musl`).
    #[serde(default)]
    pub platforms: BTreeMap<String, UpdateArtifact>,
}

/// A parsed `major.minor.patch` version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    /// Parse `major.minor.patch` with an optional leading `v`. A missing
    /// patch or minor component defaults to 0.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim().strip_prefix('v').unwrap_or(s.trim());
        let mut parts = s.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = match parts.next() {
            Some(p) => p.parse().ok()?,
            None => 0,
        };
        let patch = match parts.next() {
            Some(p) => p.parse().ok()?,
            None => 0,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// True when `candidate` is strictly newer than `current`. Unparsable input
/// returns `None` (callers treat that as "no update", never as "update").
pub fn is_newer(candidate: &str, current: &str) -> Option<bool> {
    let c = Version::parse(candidate)?;
    let cur = Version::parse(current)?;
    Some(c > cur)
}

/// Outcome of comparing a manifest against the running version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCheck {
    /// The manifest is newer than `running`.
    Available {
        running: Version,
        latest: Version,
        notes: Option<String>,
    },
    /// We are at or ahead of the manifest.
    UpToDate { running: Version, latest: Version },
    /// Version strings could not be compared; nothing is reported.
    Unknown,
}

/// Compare a fetched manifest against the running version string.
pub fn check_manifest(manifest: &UpdateManifest, running: &str) -> UpdateCheck {
    let (Some(latest), Some(run)) = (Version::parse(&manifest.version), Version::parse(running))
    else {
        return UpdateCheck::Unknown;
    };
    if latest > run {
        UpdateCheck::Available {
            running: run,
            latest,
            notes: manifest.notes.clone(),
        }
    } else {
        UpdateCheck::UpToDate {
            running: run,
            latest,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parse_forms() {
        assert_eq!(Version::parse("1.2.3"), Some(Version::new(1, 2, 3)));
        assert_eq!(Version::parse("v1.2.3"), Some(Version::new(1, 2, 3)));
        assert_eq!(Version::parse("1.2"), Some(Version::new(1, 2, 0)));
        assert_eq!(Version::parse(" 2.0.0 "), Some(Version::new(2, 0, 0)));
        assert_eq!(Version::parse("2"), Some(Version::new(2, 0, 0)));
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert_eq!(Version::parse("1.x.3"), None);
        assert_eq!(Version::parse(""), None);
        assert_eq!(Version::parse("1.2.3-beta"), None);
    }

    #[test]
    fn version_ordering() {
        assert!(Version::new(0, 2, 0) > Version::new(0, 1, 999));
        assert!(Version::new(1, 0, 0) > Version::new(0, 9, 9));
        assert!(Version::new(1, 2, 3) < Version::new(1, 2, 4));
        assert_eq!(Version::new(1, 0, 0), Version::parse("1").unwrap());
    }

    #[test]
    fn is_newer_safe_default() {
        assert_eq!(is_newer("1.1.0", "1.0.9"), Some(true));
        assert_eq!(is_newer("1.0.9", "1.1.0"), Some(false));
        assert_eq!(is_newer("v1.0.0", "1.0.0"), Some(false));
        assert_eq!(is_newer("junk", "1.0.0"), None);
        assert_eq!(is_newer("1.0.0", "not-a-version"), None);
    }

    fn manifest(version: &str, notes: Option<&str>) -> UpdateManifest {
        UpdateManifest {
            product: "lanlink".into(),
            version: version.into(),
            released: None,
            notes: notes.map(Into::into),
            platforms: BTreeMap::new(),
        }
    }

    #[test]
    fn check_manifest_outcomes() {
        let m = manifest("0.2.0", Some("fixes"));
        assert_eq!(
            check_manifest(&m, "0.1.3"),
            UpdateCheck::Available {
                running: Version::new(0, 1, 3),
                latest: Version::new(0, 2, 0),
                notes: Some("fixes".into()),
            }
        );
        assert_eq!(
            check_manifest(&m, "0.2.0"),
            UpdateCheck::UpToDate {
                running: Version::new(0, 2, 0),
                latest: Version::new(0, 2, 0),
            }
        );
        assert_eq!(
            check_manifest(&m, "0.3.0"),
            UpdateCheck::UpToDate {
                running: Version::new(0, 3, 0),
                latest: Version::new(0, 2, 0),
            }
        );
        assert_eq!(
            check_manifest(&manifest("release-9", None), "0.1.0"),
            UpdateCheck::Unknown
        );
    }

    #[test]
    fn manifest_json_roundtrip() {
        let json = r#"{
            "product": "lanlink",
            "version": "1.0.0",
            "released": "2026-10-11",
            "notes": "first stable",
            "platforms": {
                "x86_64-unknown-linux-musl": {
                    "url": "https://dl/lanlink.tar.zst",
                    "sha256": "abc",
                    "size": 42
                }
            }
        }"#;
        let m: UpdateManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.product, "lanlink");
        assert_eq!(m.version, "1.0.0");
        let artifact = m.platforms.get("x86_64-unknown-linux-musl").unwrap();
        assert_eq!(artifact.url, "https://dl/lanlink.tar.zst");
        assert_eq!(artifact.size, Some(42));

        let back = serde_json::to_string(&m).unwrap();
        let m2: UpdateManifest = serde_json::from_str(&back).unwrap();
        assert_eq!(m2.version, m.version);
    }
}
