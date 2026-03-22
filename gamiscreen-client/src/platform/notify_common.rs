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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_seconds_only() {
        assert_eq!(format_duration(0), "0 s");
        assert_eq!(format_duration(30), "30 s");
        assert_eq!(format_duration(59), "59 s");
    }

    #[test]
    fn format_duration_minutes_only() {
        assert_eq!(format_duration(60), "1 min");
        assert_eq!(format_duration(120), "2 min");
    }

    #[test]
    fn format_duration_mixed() {
        assert_eq!(format_duration(90), "1 min 30 s");
        assert_eq!(format_duration(61), "1 min 1 s");
    }

    #[test]
    fn countdown_above_final_warning() {
        let msg = countdown_message(60);
        assert!(msg.summary.contains("1 min"));
        assert!(msg.body.contains("Pozostało"));
    }

    #[test]
    fn countdown_at_final_warning_boundary() {
        let msg = countdown_message(FINAL_WARNING_SECS);
        assert!(msg.body.contains("Zapisz"));
    }

    #[test]
    fn countdown_below_final_warning() {
        let msg = countdown_message(10);
        assert!(msg.body.contains("Zapisz"));
    }

    #[test]
    fn message_text_positive_is_countdown() {
        let msg = message_text(30);
        assert!(msg.log.contains("COUNTDOWN") || msg.log.contains("CAUTION"));
    }

    #[test]
    fn message_text_negative_is_overtime() {
        let msg = message_text(-60);
        assert!(msg.log.contains("TIME-NEGATIVE"));
    }

    #[test]
    fn overtime_zero_seconds() {
        let summary = overtime_summary(0);
        assert_eq!(summary, "Czas skończył się");
    }

    #[test]
    fn overtime_with_seconds() {
        let summary = overtime_summary(30);
        assert!(summary.contains("30 s"));
    }

    #[test]
    fn overtime_with_minutes_and_seconds() {
        let summary = overtime_summary(90);
        assert!(summary.contains("1 min"));
        assert!(summary.contains("30 s"));
    }
}
