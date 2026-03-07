use notify_rust::Hint;
use tracing::{debug, info, warn};

use crate::platform::notify_common::{self, NotificationMessage};

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
            .hint(Hint::SuppressSound(false))
            .hint(Hint::SoundName("dialog-warning".into()))
            .hint(Hint::Resident(true))
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

    pub async fn update(&mut self, remaining_secs: i64) {
        debug!(
            remaining = remaining_secs,
            replace_id = self.replace_id,
            "update: building notification"
        );
        let replace_id = self.replace_id;
        let mut n = notify_rust::Notification::new();
        let text: NotificationMessage = notify_common::message_text(remaining_secs);
        let res = n
            .appname("GamiScreen")
            .summary(&text.summary)
            .body(&text.body)
            .id(replace_id)
            .urgency(notify_rust::Urgency::Critical)
            .hint(Hint::SuppressSound(false))
            .hint(Hint::SoundName("dialog-warning".into()))
            .hint(Hint::Resident(true))
            .show_async()
            .await;
        match res {
            Ok(handle) => {
                debug!(remaining = remaining_secs, "update: notification updated");
                self.handle = Some(handle);
            }
            Err(e) => {
                warn!(error=%e, "notify-rust update failed");
                self.handle = None;
                info!("{}", text.log);
            }
        }
    }

    pub async fn close(&mut self) {
        if let Some(handle) = self.handle.take() {
            debug!("close: dismissing active notification");
            let result = tokio::task::spawn_blocking(move || handle.close()).await;
            debug!(?result, "close: notification dismissed");
        }
    }
}
