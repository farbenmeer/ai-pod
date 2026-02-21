use anyhow::{Context, Result};
use colored::Colorize;
use std::path::Path;
use std::process::Command;

fn read_pid(pid_file: &Path) -> Option<u32> {
    std::fs::read_to_string(pid_file)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

fn is_process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

fn health_check(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{}/health", port);
    Command::new("curl")
        .args(["-sf", "--max-time", "2", &url])
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn is_server_running(pid_file: &Path, port: u16) -> bool {
    if let Some(pid) = read_pid(pid_file) {
        if is_process_alive(pid) && health_check(port) {
            return true;
        }
    }
    false
}

pub fn start_server(pid_file: &Path, log_file: &Path, port: u16) -> Result<()> {
    let exe = std::env::current_exe().context("Failed to get current executable path")?;

    let log = std::fs::File::create(log_file).context("Failed to create server log file")?;
    let log_err = log.try_clone()?;

    let child = Command::new(exe)
        .args(["serve-notifications", "--notify-port", &port.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(log_err))
        .spawn()
        .context("Failed to spawn notification server")?;

    std::fs::write(pid_file, child.id().to_string())
        .context("Failed to write PID file")?;

    // Wait briefly and verify it started
    std::thread::sleep(std::time::Duration::from_millis(500));

    if health_check(port) {
        println!(
            "{} (PID {}, port {})",
            "Notification server started.".green(),
            child.id(),
            port
        );
    } else {
        println!(
            "{}",
            "Notification server started but health check failed; it may still be initializing."
                .yellow()
        );
    }

    Ok(())
}

pub fn stop_server(pid_file: &Path) -> Result<()> {
    match read_pid(pid_file) {
        Some(pid) if is_process_alive(pid) => {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
            let _ = std::fs::remove_file(pid_file);
            println!("{} (PID {})", "Notification server stopped.".green(), pid);
        }
        Some(_) => {
            let _ = std::fs::remove_file(pid_file);
            println!("{}", "Server was not running (stale PID file removed).".yellow());
        }
        None => {
            println!("{}", "No PID file found; server is not running.".yellow());
        }
    }
    Ok(())
}

pub fn print_status(pid_file: &Path, port: u16) {
    match read_pid(pid_file) {
        Some(pid) => {
            let alive = is_process_alive(pid);
            let healthy = if alive { health_check(port) } else { false };
            println!("PID:     {}", pid);
            println!("Process: {}", if alive { "running".green() } else { "dead".red() });
            println!("Health:  {}", if healthy { "ok".green() } else { "unreachable".red() });
            println!("Port:    {}", port);
        }
        None => {
            println!("{}", "No PID file found; server is not running.".yellow());
        }
    }
}

pub fn ensure_server(pid_file: &Path, log_file: &Path, port: u16) -> Result<()> {
    if is_server_running(pid_file, port) {
        return Ok(());
    }
    start_server(pid_file, log_file, port)
}
