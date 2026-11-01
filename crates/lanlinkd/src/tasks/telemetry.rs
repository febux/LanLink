use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::Metrics;

/// Spawn the opt-in telemetry reporting task.
pub fn spawn(
    endpoint: String,
    interval: Duration,
    relay_servers: Vec<SocketAddr>,
    switch: Arc<Mutex<lanlink_switch::Switch>>,
    metrics: Arc<Metrics>,
    gateway_enabled: bool,
    firewall_enabled: bool,
    cancel: CancellationToken,
    tracker: &TaskTracker,
    start_time: Instant,
) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => Arc::new(c),
        Err(e) => {
            tracing::warn!(error = %e, "failed to build telemetry client; skipping telemetry task");
            return;
        }
    };

    tracker.spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = ticker.tick() => {
                    let report = build_telemetry_report(
                        &start_time,
                        &metrics,
                        &switch,
                        gateway_enabled,
                        firewall_enabled,
                        &relay_servers,
                    )
                    .await;
                    match client.post(&endpoint).json(&report).send().await {
                        Ok(r) => {
                            if r.status().is_success() {
                                tracing::debug!("telemetry report submitted");
                            } else {
                                tracing::debug!(status = %r.status(), "telemetry submission rejected");
                            }
                        }
                        Err(e) => tracing::debug!(error = %e, "telemetry submission failed"),
                    }
                }
            }
        }
    });
}

async fn build_telemetry_report(
    start_time: &Instant,
    metrics: &Metrics,
    switch: &Arc<Mutex<lanlink_switch::Switch>>,
    gateway_enabled: bool,
    firewall_enabled: bool,
    relay_servers: &[SocketAddr],
) -> lanlink_proto::telemetry::TelemetryReport {
    let addrs = {
        let sw = switch.lock().await;
        sw.peer_addrs()
    };
    let peer_count = addrs.len() as u32;
    let relay_set: HashSet<SocketAddr> = relay_servers.iter().copied().collect();
    let relay_peer_count = addrs
        .iter()
        .filter(|(_, addr)| relay_set.contains(addr))
        .count() as u32;
    let direct_peer_count = peer_count.saturating_sub(relay_peer_count);

    let uptime_seconds = start_time.elapsed().as_secs();
    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    lanlink_proto::telemetry::TelemetryReport {
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::ARCH.to_string() + "-" + std::env::consts::OS,
        uptime_seconds,
        peer_count,
        direct_peer_count,
        relay_peer_count,
        gateway_enabled,
        firewall_enabled,
        tx_bytes: metrics.tx_bytes(),
        rx_bytes: metrics.rx_bytes(),
        timestamp,
    }
}
