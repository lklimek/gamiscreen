use tracing::{debug, info};

/// Simple notifier placeholder for Windows: logs to tracing for now.
#[derive(Debug, Default)]
pub struct Notifier;

impl Notifier {
    pub fn new() -> Self {
        Self
    }

    pub async fn show_countdown(&mut self, total_secs: u64) {
        info!("[COUNTDOWN] {} s do wylogowania (Windows)", total_secs);
    }

    pub async fn update(&mut self, seconds_left: u64) {
        debug!("[COUNTDOWN UPDATE] {} s left (Windows)", seconds_left);
    }

    pub async fn close(&mut self) {
        debug!("[COUNTDOWN CLOSED] (Windows)");
    }
}

