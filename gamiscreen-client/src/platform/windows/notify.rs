use tauri_winrt_notification::{Duration as ToastDuration, Sound, Toast};
use tracing::{debug, info, warn};

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
        self.show_toast(&text.summary, &text.body).await;
        self.active = true;
    }

    pub async fn update(&mut self, remaining_secs: i64) {
        let text: NotificationMessage = notify_common::message_text(remaining_secs);
        debug!(remaining_secs, "updating countdown toast");
        self.show_toast(&text.summary, &text.body).await;
        self.active = true;
    }

    pub async fn close(&mut self) {
        if self.active {
            debug!("closing countdown toast");
            // TODO: Dismiss the visible toast via WinRT API. The tauri-winrt-notification
            // crate's show() returns Result<(), _> without a notification handle, so there
            // is no API to programmatically dismiss the toast. A future approach could use
            // the windows crate directly to get a ToastNotification handle with a dismiss
            // method. For now the toast remains on screen until its duration expires.
            self.active = false;
        }
    }

    /// Show a toast notification. Wrapped in `spawn_blocking` because the
    /// underlying COM/WinRT calls are synchronous and would block the Tokio runtime.
    async fn show_toast(&self, title: &str, body: &str) {
        let title = title.to_owned();
        let body = body.to_owned();
        let result = tokio::task::spawn_blocking(move || {
            Toast::new(APP_ID)
                .title(&title)
                .text1(&body)
                .duration(ToastDuration::Long)
                .sound(Some(Sound::Default))
                .show()
        })
        .await;

        match result {
            Ok(Ok(())) => debug!("toast notification shown"),
            Ok(Err(e)) => {
                warn!(error = %e, "failed to show toast notification");
                info!("[NOTIFICATION] toast failed");
            }
            Err(e) => {
                warn!(error = %e, "toast spawn_blocking task panicked");
            }
        }
    }
}
