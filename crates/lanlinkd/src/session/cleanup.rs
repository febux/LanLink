use std::sync::Arc;
use tokio::sync::Mutex;

/// Best-effort cleanup of process-persistent session state.
pub async fn cleanup(
    firewall_profile: Option<&lanlink_firewall::FirewallProfile>,
    gateway_manager: Option<&Arc<lanlink_gateway::GatewayManager>>,
    route_manager: &Arc<lanlink_gateway::RouteManager>,
    nic: Arc<Mutex<Box<dyn lanlink_tap::VirtualNic>>>,
    tap_name: &str,
    state_tx: &crate::control::StateTx,
    watchdog: Option<tokio::task::JoinHandle<()>>,
) {
    crate::sdnotify::notify_stopping();
    if let Some(watchdog) = watchdog {
        watchdog.abort();
    }
    if let Some(fw) = firewall_profile {
        if let Err(e) = fw.clear().await {
            tracing::warn!(error = %e, "failed to clear firewall on shutdown");
        }
    }
    if let Some(gw) = gateway_manager {
        if let Err(e) = gw.disable().await {
            tracing::warn!(error = %e, "failed to disable gateway mode on shutdown");
        }
    }
    if let Err(e) = route_manager.sync_routes(&[]).await {
        tracing::warn!(error = %e, "failed to remove routes on shutdown");
    }
    // Drop the TAP device explicitly so the interface/IP are released before
    // the daemon reports itself as disconnected. `destroy_nic` is best-effort.
    drop(nic);
    if let Err(e) = lanlink_tap::destroy_nic(tap_name) {
        tracing::debug!(error = %e, tap = %tap_name, "TAP destroy failed or unnecessary");
    }

    // Publish the empty/disconnected state to the control/status socket.
    let _ = state_tx.send_replace(crate::control::empty_state());
    tracing::info!("session cleanup complete");
}
