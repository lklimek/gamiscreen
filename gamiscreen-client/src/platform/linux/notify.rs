use notify_rust::Hint;
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
            // Request sound via desktop notification hint
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
        let text = message_text(remaining_secs);
        let res = n
            .appname("GamiScreen")
            .summary(&text.summary)
            .body(&text.body)
            .id(replace_id)
            .urgency(notify_rust::Urgency::Critical)
            // Request sound via desktop notification hint
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

#[derive(Debug)]
struct NotificationMessage {
    summary: String,
    body: String,
    log: String,
}

fn message_text(remaining_secs: i64) -> NotificationMessage {
    if remaining_secs > 0 {
        countdown_message(remaining_secs as u64)
    } else {
        overtime_message(remaining_secs.saturating_abs() as u64)
    }
}

const FINAL_WARNING_SECS: u64 = 45;

fn countdown_message(seconds_left: u64) -> NotificationMessage {
    let formatted = format_duration(seconds_left);
    if seconds_left > FINAL_WARNING_SECS {
        return NotificationMessage {
            summary: format!("Wylogowanie za {}", formatted),
            body: "Pozostało niewiele czasu. Przygotuj się do zakończenia pracy.".to_string(),
            log: format!("[CAUTION] {} s do wylogowania", seconds_left),
        };
    }

    NotificationMessage {
        summary: format!("Wylogowanie za {}", formatted),
        body: "Zapisz swoją pracę. Czas dobiega końca.".to_string(),
        log: format!("[COUNTDOWN] {} s do wylogowania", seconds_left),
    }
}

fn overtime_message(overdue_secs: u64) -> NotificationMessage {
    let summary = overtime_summary(overdue_secs);
    NotificationMessage {
        summary,
        body: "Twoje limity są ujemne. Sesja wkrótce zostanie zablokowana ponownie.".to_string(),
        log: format!("[TIME-NEGATIVE] przekroczono limit o {} s", overdue_secs),
    }
}

fn overtime_summary(overdue_secs: u64) -> String {
    if overdue_secs == 0 {
        return "Czas skończył się".to_string();
    }

    let minutes = overdue_secs / 60;
    let seconds = overdue_secs % 60;
    if minutes > 0 {
        if seconds > 0 {
            format!("Czas przekroczony o {} min {} s", minutes, seconds)
        } else {
            format!("Czas przekroczony o {} min", minutes)
        }
    } else {
        format!("Czas przekroczony o {} s", seconds)
    }
}

fn format_duration(total_secs: u64) -> String {
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    match (minutes, seconds) {
        (0, s) => format!("{} s", s),
        (m, 0) => format!("{} min", m),
        (m, s) => format!("{} min {} s", m, s),
    }
}
