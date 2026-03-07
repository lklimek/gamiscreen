pub const FINAL_WARNING_SECS: u64 = 45;

#[derive(Debug)]
pub struct NotificationMessage {
    pub summary: String,
    pub body: String,
    pub log: String,
}

pub fn message_text(remaining_secs: i64) -> NotificationMessage {
    if remaining_secs > 0 {
        countdown_message(remaining_secs as u64)
    } else {
        overtime_message(remaining_secs.saturating_abs() as u64)
    }
}

pub fn countdown_message(seconds_left: u64) -> NotificationMessage {
    let formatted = format_duration(seconds_left);
    if seconds_left > FINAL_WARNING_SECS {
        NotificationMessage {
            summary: format!("Wylogowanie za {formatted}"),
            body: "Pozostało niewiele czasu. Przygotuj się do zakończenia pracy.".to_string(),
            log: format!("[CAUTION] {seconds_left} s do wylogowania"),
        }
    } else {
        NotificationMessage {
            summary: format!("Wylogowanie za {formatted}"),
            body: "Zapisz swoją pracę. Czas dobiega końca.".to_string(),
            log: format!("[COUNTDOWN] {seconds_left} s do wylogowania"),
        }
    }
}

pub fn overtime_message(overdue_secs: u64) -> NotificationMessage {
    NotificationMessage {
        summary: overtime_summary(overdue_secs),
        body: "Twoje limity są ujemne. Sesja wkrótce zostanie zablokowana ponownie.".to_string(),
        log: format!("[TIME-NEGATIVE] przekroczono limit o {overdue_secs} s"),
    }
}

pub fn overtime_summary(overdue_secs: u64) -> String {
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

pub fn format_duration(total_secs: u64) -> String {
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    match (minutes, seconds) {
        (0, s) => format!("{s} s"),
        (m, 0) => format!("{m} min"),
        (m, s) => format!("{m} min {s} s"),
    }
}
