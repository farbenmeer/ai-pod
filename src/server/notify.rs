pub fn send_notification(title: &str, message: &str) {
    if let Err(e) = notify_rust::Notification::new()
        .summary(title)
        .body(message)
        .show()
    {
        eprintln!("[notify] Failed to send notification: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_notification_does_not_panic_with_normal_strings() {
        send_notification("Claude Code", "Task completed.");
    }

    #[test]
    fn send_notification_does_not_panic_with_quotes() {
        send_notification(r#"Title "quoted""#, r#"Message "quoted""#);
    }

    #[test]
    fn send_notification_does_not_panic_with_empty_strings() {
        send_notification("", "");
    }
}
