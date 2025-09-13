use async_trait::async_trait;
use tracing::{debug, info, warn};

#[derive(Debug)]
pub enum NotifierKind {
    NotifyRust,
    LogOnly,
}

/// Abstraction for showing and updating user countdown notifications.
#[async_trait]
pub trait NotificationBackend: Send {
    async fn show_countdown(&mut self, total_secs: u64);
    async fn update(&mut self, seconds_left: u64);
    async fn close(&mut self);
}

#[derive(Debug)]
pub struct Notifier {
    kind: NotifierKind,
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
        // Start optimistic; if we fail to show, we downgrade to LogOnly.
        let s = Self {
            kind: NotifierKind::NotifyRust,
            replace_id: 1001u32,
            handle: None,
        };
        debug!("Notifier created: using notify-rust backend initially");
        s
    }

    async fn show_countdown_inner(&mut self, total_secs: u64) {
        match self.kind {
            NotifierKind::NotifyRust => {
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
                        warn!(error=%e, "notify-rust failed; downgrading to LogOnly notifier");
                        self.kind = NotifierKind::LogOnly;
                        self.handle = None;
                        info!("[COUNTDOWN] {} s do wylogowania", total_secs);
                    }
                }
            }
            NotifierKind::LogOnly => {
                info!("[COUNTDOWN] {} s do wylogowania", total_secs);
            }
        }
    }

    async fn update_inner(&mut self, seconds_left: u64) {
        match self.kind {
            NotifierKind::NotifyRust => {
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
                        warn!(error=%e, "notify-rust update failed; switching to LogOnly");
                        self.kind = NotifierKind::LogOnly;
                        self.handle = None;
                        info!("[COUNTDOWN] {} s do wylogowania", seconds_left);
                    }
                }
            }
            NotifierKind::LogOnly => {
                info!("[COUNTDOWN] {} s do wylogowania", seconds_left);
            }
        }
    }

    async fn close_inner(&mut self) {
        match self.kind {
            NotifierKind::NotifyRust => {
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
            NotifierKind::LogOnly => {
                // Nothing to do; nothing was shown via notify backend.
            }
        }
    }
}

#[async_trait]
impl NotificationBackend for Notifier {
    async fn show_countdown(&mut self, total_secs: u64) {
        self.show_countdown_inner(total_secs).await;
    }
    async fn update(&mut self, seconds_left: u64) {
        self.update_inner(seconds_left).await;
    }
    async fn close(&mut self) {
        self.close_inner().await;
    }
}

/// Factory for the default backend (notify-rust with log fallback)
pub fn default_backend() -> Box<dyn NotificationBackend + Send> {
    Box::new(Notifier::new())
}
