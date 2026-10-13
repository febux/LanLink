//! Minimal systemd `sd_notify(3)` client (unix only).
//!
//! Implements just enough of the notify protocol for `Type=notify` services:
//! `READY=1` on startup, `WATCHDOG=1` heartbeat pings, `STOPPING=1` on
//! shutdown. Hand-rolled to avoid a dependency; the protocol is a single
//! datagram to the unix socket in `$NOTIFY_SOCKET`.

/// Send a raw notification. No-op when `NOTIFY_SOCKET` is unset (e.g. when
/// running outside systemd).
#[cfg(target_os = "linux")]
pub fn notify(message: &str) {
    use std::os::unix::net::UnixDatagram;

    let Some(path) = std::env::var_os("NOTIFY_SOCKET") else {
        return;
    };
    let socket = match UnixDatagram::unbound() {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(error = %e, "failed to create notify socket");
            return;
        }
    };
    if let Err(e) = socket.send_to(message.as_bytes(), &path) {
        tracing::debug!(error = %e, "sd_notify failed");
    }
}

#[cfg(not(target_os = "linux"))]
pub fn notify(_message: &str) {}

/// Notify systemd that the service has finished starting up.
pub fn notify_ready() {
    notify("READY=1");
}

/// Notify systemd that the service is shutting down.
pub fn notify_stopping() {
    notify("STOPPING=1");
}

/// Spawn the watchdog heartbeat task when `WATCHDOG_USEC` is set (interval is
/// half the timeout, per sd_notify(3) recommendations). Returns a handle that
/// can be aborted on shutdown; `None` when no watchdog is configured.
pub fn spawn_watchdog() -> Option<tokio::task::JoinHandle<()>> {
    #[cfg(target_os = "linux")]
    {
        let usec: u64 = match std::env::var("WATCHDOG_USEC") {
            Ok(v) => match v.parse() {
                Ok(v) => v,
                Err(_) => return None,
            },
            Err(_) => return None,
        };
        if usec == 0 {
            return None;
        }
        let interval = std::time::Duration::from_micros(usec / 2);
        tracing::debug!(?interval, "systemd watchdog enabled");
        Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                notify("WATCHDOG=1");
            }
        }))
    }
    #[cfg(not(target_os = "linux"))]
    None
}
