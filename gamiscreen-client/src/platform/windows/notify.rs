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

    pub async fn update(&mut self, seconds_left: u64) {
        debug!(seconds_left, "Windows: countdown notification updated");
    }

    pub async fn close(&mut self) {
        debug!("Windows: countdown notification closed");
    }
}
