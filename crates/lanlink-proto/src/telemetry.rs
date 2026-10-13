//! Opt-in telemetry types.
//!
//! Telemetry is **disabled by default**. When a node operator explicitly opts in,
//! the daemon sends a small, fully anonymized report to the configured
//! coordinator endpoint at a low frequency (default every 24 hours). No node id,
//! network id, network name, IP address, or traffic content is included.
//!
//! The coordinator aggregates reports into coarse statistics used to understand
//! platform distribution, typical peer counts, and relay-vs-direct success
//! rates.

use serde::{Deserialize, Serialize};

/// An anonymized telemetry report sent by a daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryReport {
    /// Running daemon version, e.g. "0.1.0".
    pub version: String,
    /// Platform identifier, normally the Rust target triple
    /// (e.g. "x86_64-unknown-linux-gnu").
    pub platform: String,
    /// Seconds since the daemon started.
    pub uptime_seconds: u64,
    /// Number of peers currently known to the switch.
    pub peer_count: u32,
    /// Peers reachable via a direct UDP path.
    pub direct_peer_count: u32,
    /// Peers reachable only through a relay server.
    pub relay_peer_count: u32,
    /// Whether gateway mode is enabled.
    pub gateway_enabled: bool,
    /// Whether the public-network inbound firewall is active.
    pub firewall_enabled: bool,
    /// Approximate bytes transmitted since the daemon started.
    pub tx_bytes: u64,
    /// Approximate bytes received since the daemon started.
    pub rx_bytes: u64,
    /// ISO 8601 timestamp of the report (UTC).
    pub timestamp: String,
}

/// Aggregated telemetry snapshot returned by the coordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedMetrics {
    /// Unix timestamp of the aggregation window start (UTC).
    pub window_start: i64,
    /// Number of reports in the window.
    pub report_count: u64,
    /// Average peer count across reports.
    pub avg_peer_count: f64,
    /// Average direct peer count across reports.
    pub avg_direct_peers: f64,
    /// Average relay peer count across reports.
    pub avg_relay_peers: f64,
    /// Fraction of reports with gateway mode enabled.
    pub gateway_fraction: f64,
    /// Fraction of reports with the firewall enabled.
    pub firewall_fraction: f64,
    /// Aggregate transmitted bytes in the window.
    pub total_tx_bytes: u64,
    /// Aggregate received bytes in the window.
    pub total_rx_bytes: u64,
    /// Version distribution in the window.
    pub versions: Vec<VersionCount>,
    /// Platform distribution in the window.
    pub platforms: Vec<PlatformCount>,
}

/// Version distribution entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionCount {
    pub version: String,
    pub count: u64,
}

/// Platform distribution entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformCount {
    pub platform: String,
    pub count: u64,
}

/// Default telemetry submission endpoint path on the coordinator.
pub const TELEMETRY_ENDPOINT: &str = "/telemetry";

/// Default interval between telemetry reports, in seconds (24 hours).
pub const DEFAULT_TELEMETRY_INTERVAL_SECS: u64 = 24 * 3600;

/// Default metrics aggregation window, in seconds (24 hours).
pub const DEFAULT_METRICS_WINDOW_SECS: i64 = 24 * 3600;
