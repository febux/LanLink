//! TOML configuration file support for `lanlinkd`.
//!
//! A daemon run from a service manager must not take secrets on the command
//! line (visible in `ps`). The config file supplies everything `lanlinkd run`
//! needs; CLI flags override individual settings when both are given.
//!
//! ```toml
//! [network]
//! coordinator = "https://coord.example.com:8080"
//! network_id = 42
//! password_file = "/etc/lanlink/gaming-night.secret"
//! bind = "0.0.0.0:9000"
//! tap = "lanlink0"
//! mtu = 1400
//! stun_servers = ["stun.l.google.com:19302"]
//!
//! [gateway]
//! enabled = false
//! shared_subnet = "192.168.1.0/24"
//! phys_interface = "eth0"
//!
//! [telemetry]
//! enabled = false
//! # endpoint = "https://coord.example.com:8080/telemetry"
//! # interval_hours = 24
//!
//! [status]
//! # socket = "/run/lanlink/lanlinkd-42.sock"
//! ```

use serde::Deserialize;
use std::net::SocketAddr;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    pub network: NetworkSection,
    #[serde(default)]
    pub gateway: GatewaySection,
    #[serde(default)]
    pub update: UpdateSection,
    #[serde(default)]
    pub telemetry: TelemetrySection,
    #[serde(default)]
    pub status: StatusSection,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkSection {
    /// Coordinator base URL, e.g. "https://coord.example.com:8080".
    pub coordinator: String,
    /// Network id to join.
    pub network_id: u64,
    /// Network password (prefer `password_file` for production).
    #[serde(default)]
    pub password: Option<String>,
    /// Path to a file (mode 0600) containing the network password.
    #[serde(default)]
    pub password_file: Option<String>,
    /// UDP socket to listen on (also advertised as direct candidate).
    #[serde(default = "default_bind")]
    pub bind: SocketAddr,
    /// TAP/virtual NIC name.
    #[serde(default = "default_tap")]
    pub tap: String,
    /// Virtual NIC MTU.
    #[serde(default = "default_mtu")]
    pub mtu: usize,
    /// STUN servers (host:port). Empty means "use built-in defaults".
    #[serde(default)]
    pub stun_servers: Vec<String>,
}

impl Default for NetworkSection {
    fn default() -> Self {
        Self {
            coordinator: String::new(),
            network_id: 0,
            password: None,
            password_file: None,
            bind: default_bind(),
            tap: default_tap(),
            mtu: default_mtu(),
            stun_servers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewaySection {
    /// Enable gateway mode (share the physical LAN with the virtual network).
    #[serde(default)]
    pub enabled: bool,
    /// Physical subnet to share, e.g. "192.168.1.0/24". Required when enabled.
    #[serde(default)]
    pub shared_subnet: Option<String>,
    /// Physical interface used to reach the shared subnet. Required when enabled.
    #[serde(default)]
    pub phys_interface: Option<String>,
}

/// Update-channel settings. Disabled by default: the daemon does not check
/// for updates (or contact any third party) unless this section opts in.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateSection {
    /// Opt in to periodic update-manifest checks (default false).
    #[serde(default)]
    pub enabled: bool,
    /// Manifest URL. Defaults to the public update channel when enabled.
    #[serde(default)]
    pub manifest: Option<String>,
    /// Hours between checks (default 24; first check runs immediately).
    #[serde(default = "default_update_interval")]
    pub interval_hours: u64,
}

impl Default for UpdateSection {
    fn default() -> Self {
        Self {
            enabled: false,
            manifest: None,
            interval_hours: default_update_interval(),
        }
    }
}

/// Telemetry settings. Disabled by default: the daemon never sends metrics
/// unless the operator explicitly opts in.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetrySection {
    /// Opt in to periodic anonymized telemetry reports (default false).
    #[serde(default)]
    pub enabled: bool,
    /// Coordinator telemetry endpoint. Defaults to `<coordinator>/telemetry`.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Hours between reports (default 24).
    #[serde(default = "default_telemetry_interval")]
    pub interval_hours: u64,
}

impl Default for TelemetrySection {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: None,
            interval_hours: default_telemetry_interval(),
        }
    }
}

fn default_update_interval() -> u64 {
    24
}

/// Local status-API settings (read-only socket served to frontends).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusSection {
    /// Override the status socket path. Defaults to
    /// `/run/lanlink/lanlinkd-<network_id>.sock` (with a `/tmp` fallback).
    #[serde(default)]
    pub socket: Option<String>,
}

fn default_telemetry_interval() -> u64 {
    24
}

fn default_bind() -> SocketAddr {
    SocketAddr::from(([0, 0, 0, 0], 9000))
}
fn default_tap() -> String {
    "lanlink0".to_string()
}
fn default_mtu() -> usize {
    1400
}

/// Fully-resolved settings for `lanlinkd run`.
#[derive(Debug, Clone)]
pub struct RunConfig {
    pub coordinator: String,
    pub network_id: u64,
    pub password: String,
    pub bind: SocketAddr,
    pub tap: String,
    pub mtu: usize,
    pub stun_servers: Vec<String>,
    pub gateway: bool,
    pub shared_subnet: Option<String>,
    pub phys_interface: Option<String>,
    /// Resolved update-channel settings.
    pub update: UpdateSettings,
    /// Resolved telemetry settings.
    pub telemetry: TelemetrySettings,
    /// Status socket path override (None = platform default).
    pub status_socket: Option<String>,
}

/// Fully-resolved update-channel settings for `lanlinkd run`.
#[derive(Debug, Clone)]
pub struct UpdateSettings {
    pub enabled: bool,
    pub manifest: String,
    pub interval: std::time::Duration,
}

/// Fully-resolved telemetry settings for `lanlinkd run`.
#[derive(Debug, Clone)]
pub struct TelemetrySettings {
    pub enabled: bool,
    pub endpoint: String,
    pub interval: std::time::Duration,
}

impl Default for TelemetrySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: String::new(),
            interval: std::time::Duration::from_secs(24 * 3600),
        }
    }
}

/// A single CLI-flag override set (None = "not given on the command line").
#[derive(Debug, Clone, Default)]
pub struct CliOverrides {
    pub coordinator: Option<String>,
    pub network_id: Option<u64>,
    pub password: Option<String>,
    pub bind: Option<SocketAddr>,
    pub tap: Option<String>,
    pub mtu: Option<usize>,
    pub stun_servers: Option<Vec<String>>,
    pub gateway: Option<bool>,
    pub shared_subnet: Option<String>,
    pub phys_interface: Option<String>,
    pub status_socket: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to parse config file {path}: {source}")]
    Parse {
        path: String,
        source: toml::de::Error,
    },
    #[error("missing required setting: {0}")]
    Missing(String),
    #[error("no password configured (set network.password or network.password_file)")]
    NoPassword,
    #[error("failed to read password file {path}: {source}")]
    PasswordFile {
        path: String,
        source: std::io::Error,
    },
    #[error("password file {path} is empty")]
    EmptyPasswordFile { path: String },
}

/// Load `path` and merge it with CLI overrides into a [`RunConfig`].
pub fn load(path: &str, cli: &CliOverrides) -> Result<RunConfig, ConfigError> {
    let raw = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_string(),
        source,
    })?;
    let file: FileConfig = toml::from_str(&raw).map_err(|source| ConfigError::Parse {
        path: path.to_string(),
        source,
    })?;

    resolve(&file, cli)
}

/// Merge a parsed file config with CLI overrides. Public for tests.
pub fn resolve(file: &FileConfig, cli: &CliOverrides) -> Result<RunConfig, ConfigError> {
    let coordinator = cli
        .coordinator
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let f = file.network.coordinator.clone();
            (!f.is_empty()).then_some(f)
        })
        .ok_or_else(|| ConfigError::Missing("network.coordinator".into()))?;

    let network_id = cli
        .network_id
        .filter(|v| *v != 0)
        .or(Some(file.network.network_id))
        .filter(|v| *v != 0)
        .ok_or_else(|| ConfigError::Missing("network.network_id".into()))?;

    let password = if let Some(p) = &cli.password {
        p.clone()
    } else if let Some(p) = &file.network.password {
        p.clone()
    } else if let Some(pf) = &file.network.password_file {
        read_password_file(pf)?
    } else {
        return Err(ConfigError::NoPassword);
    };

    let gateway = cli.gateway.unwrap_or(file.gateway.enabled);
    let shared_subnet = cli
        .shared_subnet
        .clone()
        .or_else(|| file.gateway.shared_subnet.clone());
    let phys_interface = cli
        .phys_interface
        .clone()
        .or_else(|| file.gateway.phys_interface.clone());

    if gateway && shared_subnet.as_deref().unwrap_or_default().is_empty() {
        return Err(ConfigError::Missing(
            "gateway.shared_subnet (required when gateway mode is enabled)".into(),
        ));
    }
    if gateway && phys_interface.as_deref().unwrap_or_default().is_empty() {
        return Err(ConfigError::Missing(
            "gateway.phys_interface (required when gateway mode is enabled)".into(),
        ));
    }

    let telemetry_endpoint = file
        .telemetry
        .endpoint
        .clone()
        .unwrap_or_else(|| format!("{}/telemetry", coordinator.trim_end_matches('/')));

    Ok(RunConfig {
        coordinator,
        network_id,
        password,
        bind: cli.bind.unwrap_or(file.network.bind),
        tap: cli.tap.clone().unwrap_or_else(|| file.network.tap.clone()),
        mtu: cli.mtu.unwrap_or(file.network.mtu),
        stun_servers: cli
            .stun_servers
            .clone()
            .unwrap_or_else(|| file.network.stun_servers.clone()),
        gateway,
        shared_subnet,
        phys_interface,
        update: UpdateSettings {
            enabled: file.update.enabled,
            manifest: file
                .update
                .manifest
                .clone()
                .unwrap_or_else(|| lanlink_proto::update::DEFAULT_MANIFEST_URL.to_string()),
            interval: std::time::Duration::from_secs(file.update.interval_hours.max(1) * 3600),
        },
        telemetry: TelemetrySettings {
            enabled: file.telemetry.enabled,
            endpoint: telemetry_endpoint,
            interval: std::time::Duration::from_secs(file.telemetry.interval_hours.max(1) * 3600),
        },
        status_socket: cli
            .status_socket
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| file.status.socket.clone()),
    })
}

/// Read a password from a file: first line, trimmed, must be non-empty.
pub fn read_password_file(path: &str) -> Result<String, ConfigError> {
    let content = std::fs::read_to_string(path).map_err(|source| ConfigError::PasswordFile {
        path: path.to_string(),
        source,
    })?;
    let password = content.lines().next().unwrap_or("").trim().to_string();
    if password.is_empty() {
        return Err(ConfigError::EmptyPasswordFile {
            path: path.to_string(),
        });
    }
    Ok(password)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
[network]
coordinator = "http://localhost:8080"
network_id = 7
password = "hunter2"
"#;

    #[test]
    fn minimal_config_resolves_with_defaults() {
        let file: FileConfig = toml::from_str(MINIMAL).unwrap();
        let cfg = resolve(&file, &CliOverrides::default()).unwrap();
        assert_eq!(cfg.coordinator, "http://localhost:8080");
        assert_eq!(cfg.network_id, 7);
        assert_eq!(cfg.password, "hunter2");
        assert_eq!(cfg.tap, "lanlink0");
        assert_eq!(cfg.mtu, 1400);
        assert_eq!(cfg.bind.port(), 9000);
        assert!(!cfg.gateway);
    }

    #[test]
    fn full_config_parses() {
        let file: FileConfig = toml::from_str(
            r#"
[network]
coordinator = "https://coord.example.com"
network_id = 42
password_file = "/etc/lanlink/secret"
bind = "0.0.0.0:9999"
tap = "lanlink9"
mtu = 1300
stun_servers = ["stun.example.com:3478"]

[gateway]
enabled = true
shared_subnet = "10.1.0.0/16"
phys_interface = "enp3s0"
"#,
        )
        .unwrap();
        let cfg = resolve(&file, &CliOverrides::default()).unwrap_err();
        // password_file points nowhere in the test env; expect PasswordFile error
        assert!(matches!(cfg, ConfigError::PasswordFile { .. }));

        // with the password supplied via CLI override it resolves
        let cli = CliOverrides {
            password: Some("pw".into()),
            ..Default::default()
        };
        let cfg = resolve(&file, &cli).unwrap();
        assert!(cfg.gateway);
        assert_eq!(cfg.shared_subnet.as_deref(), Some("10.1.0.0/16"));
        assert_eq!(cfg.phys_interface.as_deref(), Some("enp3s0"));
        assert_eq!(cfg.tap, "lanlink9");
        assert_eq!(cfg.mtu, 1300);
        assert_eq!(cfg.stun_servers, vec!["stun.example.com:3478"]);
    }

    #[test]
    fn cli_overrides_beat_file() {
        let file: FileConfig = toml::from_str(MINIMAL).unwrap();
        let cli = CliOverrides {
            coordinator: Some("http://override:1234".into()),
            network_id: Some(99),
            password: Some("cli-pw".into()),
            bind: Some("0.0.0.0:7777".parse().unwrap()),
            tap: Some("tapX".into()),
            mtu: Some(1200),
            stun_servers: Some(vec!["s:1".into()]),
            gateway: Some(true),
            shared_subnet: Some("172.16.0.0/12".into()),
            phys_interface: Some("eth9".into()),
            status_socket: Some("/tmp/custom.sock".into()),
        };
        let cfg = resolve(&file, &cli).unwrap();
        assert_eq!(cfg.coordinator, "http://override:1234");
        assert_eq!(cfg.network_id, 99);
        assert_eq!(cfg.password, "cli-pw");
        assert_eq!(cfg.bind.port(), 7777);
        assert_eq!(cfg.tap, "tapX");
        assert_eq!(cfg.mtu, 1200);
        assert_eq!(cfg.stun_servers, vec!["s:1"]);
        assert!(cfg.gateway);
    }

    #[test]
    fn missing_required_fields_are_reported() {
        let file = FileConfig::default();
        let err = resolve(&file, &CliOverrides::default()).unwrap_err();
        assert!(matches!(err, ConfigError::Missing(ref m) if m.contains("coordinator")));

        let file: FileConfig =
            toml::from_str("[network]\ncoordinator = \"http://x\"\nnetwork_id = 1\n").unwrap();
        let err = resolve(&file, &CliOverrides::default()).unwrap_err();
        assert!(matches!(err, ConfigError::NoPassword));
    }

    #[test]
    fn gateway_requires_subnet_and_interface() {
        let file: FileConfig = toml::from_str(
            "[network]\ncoordinator = \"http://x\"\nnetwork_id = 1\npassword = \"p\"\n[gateway]\nenabled = true\n",
        )
        .unwrap();
        let err = resolve(&file, &CliOverrides::default()).unwrap_err();
        assert!(matches!(err, ConfigError::Missing(ref m) if m.contains("shared_subnet")));
    }

    #[test]
    fn password_file_reads_first_line_trimmed() {
        let dir = std::env::temp_dir().join(format!("lanlink-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pw");
        std::fs::write(&path, "  secret line  \nsecond line\n").unwrap();
        let pw = read_password_file(path.to_str().unwrap()).unwrap();
        assert_eq!(pw, "secret line");

        std::fs::write(&path, "\n").unwrap();
        let err = read_password_file(path.to_str().unwrap()).unwrap_err();
        assert!(matches!(err, ConfigError::EmptyPasswordFile { .. }));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_fields_rejected() {
        let err = toml::from_str::<FileConfig>("[network]\nfoo = 1\n");
        assert!(err.is_err());
    }

    #[test]
    fn update_section_defaults_off_and_resolves() {
        let file: FileConfig = toml::from_str(MINIMAL).unwrap();
        let cfg = resolve(&file, &CliOverrides::default()).unwrap();
        assert!(!cfg.update.enabled);
        assert_eq!(
            cfg.update.manifest,
            lanlink_proto::update::DEFAULT_MANIFEST_URL
        );
        assert_eq!(
            cfg.update.interval,
            std::time::Duration::from_secs(24 * 3600)
        );

        let file: FileConfig = toml::from_str(
            "[network]\ncoordinator = \"http://x\"\nnetwork_id = 1\npassword = \"p\"\n\
             [update]\nenabled = true\nmanifest = \"https://mirror.example.com/m.json\"\ninterval_hours = 6\n",
        )
        .unwrap();
        let cfg = resolve(&file, &CliOverrides::default()).unwrap();
        assert!(cfg.update.enabled);
        assert_eq!(cfg.update.manifest, "https://mirror.example.com/m.json");
        assert_eq!(
            cfg.update.interval,
            std::time::Duration::from_secs(6 * 3600)
        );
    }

    #[test]
    fn telemetry_section_defaults_off_and_resolves() {
        let file: FileConfig = toml::from_str(MINIMAL).unwrap();
        let cfg = resolve(&file, &CliOverrides::default()).unwrap();
        assert!(!cfg.telemetry.enabled);
        assert_eq!(cfg.telemetry.endpoint, "http://localhost:8080/telemetry");
        assert_eq!(
            cfg.telemetry.interval,
            std::time::Duration::from_secs(24 * 3600)
        );

        let file: FileConfig = toml::from_str(
            "[network]\ncoordinator = \"https://coord.example.com\"\nnetwork_id = 1\npassword = \"p\"\n\
             [telemetry]\nenabled = true\nendpoint = \"https://metrics.example.com/telemetry\"\ninterval_hours = 12\n",
        )
        .unwrap();
        let cfg = resolve(&file, &CliOverrides::default()).unwrap();
        assert!(cfg.telemetry.enabled);
        assert_eq!(
            cfg.telemetry.endpoint,
            "https://metrics.example.com/telemetry"
        );
        assert_eq!(
            cfg.telemetry.interval,
            std::time::Duration::from_secs(12 * 3600)
        );
    }
}
