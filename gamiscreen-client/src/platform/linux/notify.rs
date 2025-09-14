use tracing::{debug, info, warn};

#[derive(Debug)]
pub struct Notifier {
    replace_id: u32,
    handle: Option<notify_rust::NotificationHandle>,
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Notifier {
    pub fn new() -> Self {
        let s = Self {
            replace_id: 1001u32,
            handle: None,
        };
        debug!("Linux Notifier created");
        s
    }

    pub async fn show_countdown(&mut self, total_secs: u64) {
        debug!(
            seconds = total_secs,
            replace_id = self.replace_id,
            "show_countdown: building notification"
        );
        let replace_id = self.replace_id;
        let mut n = notify_rust::Notification::new();
        let res = n
            .appname("GamiScreen")
            .summary(&format!("Wylogowanie za {} s", total_secs))
            .body("Zapisz swoją pracę. Czas dobiega końca.")
            .id(replace_id)
            .urgency(notify_rust::Urgency::Critical)
            .show_async()
            .await;

        match res {
            Ok(handle) => {
                debug!(seconds = total_secs, "show_countdown: notification shown");
                self.handle = Some(handle);
            }
            Err(e) => {
                warn!(error=%e, "notify-rust failed while showing countdown");
                self.handle = None;
                info!("[COUNTDOWN] {} s do wylogowania", total_secs);
            }
        }
    }

    pub async fn update(&mut self, seconds_left: u64) {
        debug!(
            seconds = seconds_left,
            replace_id = self.replace_id,
            "update: building notification"
        );
        let replace_id = self.replace_id;
        let mut n = notify_rust::Notification::new();
        let res = n
            .appname("GamiScreen")
            .summary(&format!("Wylogowanie za {} s", seconds_left))
            .body("Zapisz swoją pracę. Czas dobiega końca.")
            .id(replace_id)
            .urgency(notify_rust::Urgency::Critical)
            .show_async()
            .await;
        match res {
            Ok(handle) => {
                debug!(seconds = seconds_left, "update: notification updated");
                self.handle = Some(handle);
            }
            Err(e) => {
                warn!(error=%e, "notify-rust update failed");
                self.handle = None;
                info!("[COUNTDOWN] {} s do wylogowania", seconds_left);
            }
        }
    }

    pub async fn close(&mut self) {
        if self.handle.take().is_some() {
            debug!("close: replacing with short-timeout notification (async hack)");
            let replace_id = self.replace_id;
            let mut n = notify_rust::Notification::new();
            // Replace current notification with an empty, near-immediate timeout one.
            let _ = n
                .appname("GamiScreen")
                .summary("Koniec czasu")
                .body("Czas dobiegł końca.")
                .id(replace_id)
                .urgency(notify_rust::Urgency::Low)
                .timeout(notify_rust::Timeout::Milliseconds(1))
                .show_async()
                .await;
        }
    }
}
