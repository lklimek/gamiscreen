use tracing::{debug, info, warn};
use winrt_notification::{Duration as ToastDuration, Sound, Toast};

const APP_ID: &str = Toast::POWERSHELL_APP_ID;
const FINAL_WARNING_SECS: u64 = 45;

#[derive(Debug, Default)]
pub struct Notifier {
    active: bool,
}

impl Notifier {
    pub fn new() -> Self {
        Self { active: false }
    }

    pub async fn show_countdown(&mut self, total_secs: u64) {
        let text = countdown_message(total_secs);
        debug!(total_secs, "showing countdown toast");
        self.show_toast(&text.summary, &text.body);
        self.active = true;
    }

    pub async fn update(&mut self, remaining_secs: i64) {
        let text = message_text(remaining_secs);
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

struct NotificationMessage {
    summary: String,
    body: String,
}

fn message_text(remaining_secs: i64) -> NotificationMessage {
    if remaining_secs > 0 {
        countdown_message(remaining_secs as u64)
    } else {
        overtime_message(remaining_secs.saturating_abs() as u64)
    }
}

fn countdown_message(seconds_left: u64) -> NotificationMessage {
    let formatted = format_duration(seconds_left);
    if seconds_left > FINAL_WARNING_SECS {
        NotificationMessage {
            summary: format!("Wylogowanie za {formatted}"),
            body: "Pozostało niewiele czasu. Przygotuj się do zakończenia pracy.".to_string(),
        }
    } else {
        NotificationMessage {
            summary: format!("Wylogowanie za {formatted}"),
            body: "Zapisz swoją pracę. Czas dobiega końca.".to_string(),
        }
    }
}

fn overtime_message(overdue_secs: u64) -> NotificationMessage {
    NotificationMessage {
        summary: overtime_summary(overdue_secs),
        body: "Twoje limity są ujemne. Sesja wkrótce zostanie zablokowana ponownie.".to_string(),
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
            format!("Czas przekroczony o {minutes} min {seconds} s")
        } else {
            format!("Czas przekroczony o {minutes} min")
        }
    } else {
        format!("Czas przekroczony o {seconds} s")
    }
}

fn format_duration(total_secs: u64) -> String {
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    match (minutes, seconds) {
        (0, s) => format!("{s} s"),
        (m, 0) => format!("{m} min"),
        (m, s) => format!("{m} min {s} s"),
    }
}
