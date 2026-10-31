use notify_rust::Hint;
use tracing::{debug, info, warn};

use crate::platform::notify_common::{self, NotificationMessage};

#[derive(Debug)]
pub struct Notifier {
    replace_id: u32,
    handle: Option<u32>,
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
        let text: NotificationMessage = notify_common::countdown_message(total_secs);
        let replace_id = self.replace_id;
        let mut n = notify_rust::Notification::new();
        n.appname("GamiScreen")
            .summary(&text.summary)
            .body(&text.body)
            .id(replace_id)
            .urgency(notify_rust::Urgency::Critical)
            .hint(Hint::SuppressSound(false))
            .hint(Hint::SoundName("dialog-warning".into()))
            .hint(Hint::Resident(true));
        let res = tokio::task::spawn_blocking(move || n.show().map(|handle| handle.id())).await;

        match res {
            Ok(Ok(handle)) => {
                debug!(seconds = total_secs, "show_countdown: notification shown");
                self.replace_id = handle;
                self.handle = Some(handle);
            }
            e => {
                warn!(error=?e, "notify-rust failed while showing countdown");
                self.handle = None;
                info!("{}", text.log);
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
        n.appname("GamiScreen")
            .summary(&text.summary)
            .body(&text.body)
            .id(replace_id)
            .urgency(notify_rust::Urgency::Critical)
            .hint(Hint::SuppressSound(false))
            .hint(Hint::SoundName("dialog-warning".into()))
            .hint(Hint::Resident(true));
        let res = tokio::task::spawn_blocking(move || n.show().map(|handle| handle.id())).await;
        match res {
            Ok(Ok(handle)) => {
                debug!(remaining = remaining_secs, "update: notification updated");
                self.replace_id = handle;
                self.handle = Some(handle);
            }
            e => {
                warn!(error=?e, "notify-rust update failed");
                self.handle = None;
                info!("{}", text.log);
            }
        }
    }

    pub async fn close(&mut self) {
        if let Some(handle) = self.handle.take() {
            debug!("close: dismissing active notification");
            let result: Result<(), zbus::Error> = async {
                let conn = zbus::Connection::session().await?;
                let proxy = zbus::proxy::Proxy::new(
                    &conn,
                    "org.freedesktop.Notifications",
                    "/org/freedesktop/Notifications",
                    "org.freedesktop.Notifications",
                )
                .await?;
                proxy.call("CloseNotification", &(handle,)).await
            }
            .await;
            debug!(?result, "close: notification dismissed");
        }
    }
}
