use std::process::Command;

pub enum NotifyBackend {
    OsaScript,
    NotifySend,
    None,
}

pub fn detect_backend() -> NotifyBackend {
    if Command::new("which")
        .arg("osascript")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        return NotifyBackend::OsaScript;
    }

    if Command::new("which")
        .arg("notify-send")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        return NotifyBackend::NotifySend;
    }

    NotifyBackend::None
}

pub fn send_notification(title: &str, message: &str) {
    match detect_backend() {
        NotifyBackend::OsaScript => {
            let script = format!(
                "display notification \"{}\" with title \"{}\"",
                message.replace('"', "\\\""),
                title.replace('"', "\\\"")
            );
            let _ = Command::new("osascript").args(["-e", &script]).output();
        }
        NotifyBackend::NotifySend => {
            let _ = Command::new("notify-send")
                .args([title, message])
                .output();
        }
        NotifyBackend::None => {
            eprintln!("[notify] No notification backend available");
        }
    }
}
