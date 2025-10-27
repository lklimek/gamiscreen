use tracing::{debug, info};

/// Simple notifier placeholder for Windows: logs to tracing for now.
#[derive(Debug, Default)]
pub struct Notifier;

impl Notifier {
    pub fn new() -> Self {
        Self
    }

    pub async fn show_countdown(&mut self, total_secs: u64) {
        info!(total_secs, "Windows: countdown notification opened");
    }

    pub async fn update(&mut self, remaining_secs: i64) {
        if remaining_secs > 0 {
            debug!(remaining_secs, "Windows: countdown notification updated");
        } else {
            info!(remaining_secs, "Windows: negative time notification");
        }
    }

    pub async fn close(&mut self) {
        debug!("Windows: countdown notification closed");
    }
}
