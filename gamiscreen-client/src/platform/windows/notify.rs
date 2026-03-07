use tracing::{debug, info, warn};
use winrt_notification::{Duration as ToastDuration, Sound, Toast};

use crate::platform::notify_common::{self, NotificationMessage};

const APP_ID: &str = Toast::POWERSHELL_APP_ID;

#[derive(Debug, Default)]
pub struct Notifier {
    active: bool,
}

impl Notifier {
    pub fn new() -> Self {
        Self { active: false }
    }

    pub async fn show_countdown(&mut self, total_secs: u64) {
        let text: NotificationMessage = notify_common::countdown_message(total_secs);
        debug!(total_secs, "showing countdown toast");
        self.show_toast(&text.summary, &text.body);
        self.active = true;
    }

    pub async fn update(&mut self, remaining_secs: i64) {
        let text: NotificationMessage = notify_common::message_text(remaining_secs);
        debug!(remaining_secs, "updating countdown toast");
        self.show_toast(&text.summary, &text.body);
        self.active = true;
    }

    pub async fn close(&mut self) {
        if self.active {
            debug!("closing countdown toast");
            self.active = false;
        }
    }

    fn show_toast(&self, title: &str, body: &str) {
        let result = Toast::new(APP_ID)
            .title(title)
            .text1(body)
            .duration(ToastDuration::Long)
            .sound(Some(Sound::Default))
            .show();

        match result {
            Ok(()) => debug!("toast notification shown"),
            Err(e) => {
                warn!(error = %e, "failed to show toast notification");
                info!("[NOTIFICATION] {title}: {body}");
            }
        }
    }
}
