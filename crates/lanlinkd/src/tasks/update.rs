use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// Spawn the opt-in update-channel checking task.
pub fn spawn(
    manifest_url: String,
    interval: Duration,
    cancel: CancellationToken,
    tracker: &TaskTracker,
) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => Arc::new(c),
        Err(e) => {
            tracing::warn!(error = %e, "failed to build update-check client; skipping update task");
            return;
        }
    };

    tracker.spawn(async move {
        let mut notified: Option<String> = None;
        let mut ticker = tokio::time::interval(interval);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = ticker.tick() => {
                    match client
                        .get(&manifest_url)
                        .send()
                        .await
                        .and_then(|r| r.error_for_status())
                    {
                        Ok(resp) => match resp.json::<lanlink_proto::update::UpdateManifest>().await {
                            Ok(m) => {
                                if let lanlink_proto::update::UpdateCheck::Available {
                                    latest, ..
                                } =
                                    lanlink_proto::update::check_manifest(&m, env!("CARGO_PKG_VERSION"))
                                {
                                    if notified.as_deref() != Some(latest.to_string().as_str()) {
                                        tracing::info!(
                                            running = env!("CARGO_PKG_VERSION"),
                                            latest = %latest,
                                            "update available on the release channel"
                                        );
                                        notified = Some(latest.to_string());
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::debug!(error = %e, "update manifest parse failed")
                            }
                        },
                        Err(e) => {
                            tracing::debug!(error = %e, "update check failed")
                        }
                    }
                }
            }
        }
    });
}
