//! Host-side Playwright MCP integration (`--playwright`).
//!
//! When ai-pod is launched with `--playwright`, a Playwright MCP server is
//! started **on the host** (`npx @playwright/mcp@latest --port 8931 --host
//! 0.0.0.0`) and wired into the agent running inside the container. The agent
//! then drives a real, headed browser on the host — with a profile that
//! persists between runs, so sites stay logged in — instead of a throwaway
//! browser inside the pod.
//!
//! Two details make the container -> host hop work:
//!   * the MCP url uses the runtime's host gateway name
//!     (`host.containers.internal` / `host.docker.internal`), not `localhost`;
//!   * Playwright MCP rejects any request whose `Host` header isn't in its
//!     allow-list (which defaults to the bound address, normalized to
//!     `localhost:<port>`), so the MCP entry sends a spoofed
//!     `Host: localhost:8931`. As a belt-and-braces measure for clients that
//!     refuse to override `Host`, the server is also started with an explicit
//!     `--allowed-hosts` list containing both gateway names.

use anyhow::{Context, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::config::AppConfig;

/// Port the host-side Playwright MCP server listens on.
pub const PORT: u16 = 8931;

/// npm spec used to launch the server via `npx`.
const PACKAGE: &str = "@playwright/mcp@latest";

/// How long to wait for the server to accept connections. Generous because the
/// first `npx` run downloads the package (and possibly a browser).
const STARTUP_TIMEOUT: Duration = Duration::from_secs(180);

/// The MCP endpoint as seen from inside a container, for the given runtime
/// gateway hostname.
pub fn mcp_url(host_gateway: &str) -> String {
    format!("http://{}:{}/mcp", host_gateway, PORT)
}

/// The `Host` header value the in-container agent must send. Playwright MCP
/// normalizes a wildcard bind to `localhost:<port>` when computing its default
/// allow-list, so this is the value it expects.
pub fn host_header() -> String {
    format!("localhost:{}", PORT)
}

/// Hostnames accepted by the server we start. Covers the spoofed `localhost`
/// value plus both container-runtime gateway names, so the integration also
/// works with an MCP client that refuses to override the `Host` header.
fn allowed_hosts() -> String {
    [
        "localhost",
        "127.0.0.1",
        "host.containers.internal",
        "host.docker.internal",
    ]
    .iter()
    .map(|h| format!("{}:{}", h, PORT))
    .collect::<Vec<_>>()
    .join(",")
}

/// State of the host-side server, stored in `~/.ai-pod/playwright.json`.
#[derive(Serialize, Deserialize, Default)]
struct PlaywrightState {
    pid: Option<u32>,
    port: Option<u16>,
}

fn state_file(config: &AppConfig) -> PathBuf {
    config.config_dir.join("playwright.json")
}

fn load_state(path: &Path) -> PlaywrightState {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_state(path: &Path, state: &PlaywrightState) -> Result<()> {
    let json = serde_json::to_string_pretty(state)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .context("Failed to write playwright state")?;
    file.write_all(json.as_bytes())
        .context("Failed to write playwright state contents")?;
    Ok(())
}

fn is_process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Whether something is already listening on the loopback side of `PORT`.
fn port_is_open() -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, PORT));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

/// Handle for a running Playwright MCP server.
///
/// `owned` is true only when *this* ai-pod process spawned the server; a
/// reused server (started by a concurrent session, or by the user by hand) is
/// left running on shutdown.
pub struct PlaywrightServer {
    pid: u32,
    owned: bool,
    state_path: PathBuf,
}

impl PlaywrightServer {
    /// Stop the server if we started it. Best-effort: signals the whole
    /// process group, since `npx` runs the server as a child process.
    pub fn shutdown(self) {
        if !self.owned {
            return;
        }
        unsafe {
            libc::kill(-(self.pid as i32), libc::SIGTERM);
        }
        let _ = std::fs::remove_file(&self.state_path);
        eprintln!("{}", "Playwright MCP stopped.".blue());
    }
}

/// Ensure a Playwright MCP server is reachable on the host, starting one if
/// needed. Reuses an already-running server (ours or the user's).
pub fn ensure_running(config: &AppConfig) -> Result<PlaywrightServer> {
    let state_path = state_file(config);
    let state = load_state(&state_path);

    if let Some(pid) = state.pid
        && is_process_alive(pid)
        && port_is_open()
    {
        eprintln!(
            "{} (PID {}, port {})",
            "Playwright MCP already running.".green(),
            pid,
            PORT
        );
        return Ok(PlaywrightServer {
            pid,
            owned: false,
            state_path,
        });
    }

    // Someone else owns the port (a hand-started server, or a stale record we
    // can no longer match to a pid). Use it rather than failing to bind.
    if port_is_open() {
        eprintln!(
            "{} reusing the server already listening on port {}.",
            "Playwright MCP:".blue().bold(),
            PORT
        );
        return Ok(PlaywrightServer {
            pid: 0,
            owned: false,
            state_path,
        });
    }

    let _ = std::fs::remove_file(&state_path);
    spawn(config, state_path)
}

fn npx_available() -> bool {
    Command::new("npx")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn spawn(config: &AppConfig, state_path: PathBuf) -> Result<PlaywrightServer> {
    if !npx_available() {
        anyhow::bail!(
            "`npx` was not found on PATH. --playwright runs {} on the host; install Node.js (>=18) and retry.",
            PACKAGE
        );
    }

    let log_path = config.config_dir.join("playwright.log");
    let log = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&log_path)
        .context("Failed to create playwright log file")?;
    let log_err = log.try_clone()?;

    eprintln!(
        "{} {} on port {} (host 0.0.0.0)...",
        "Starting Playwright MCP:".blue().bold(),
        PACKAGE,
        PORT
    );
    eprintln!(
        "{} the Playwright MCP server binds all interfaces and has no \
         authentication — anyone who can reach port {} on this machine can \
         drive your browser.",
        "warning:".yellow().bold(),
        PORT
    );

    let child = Command::new("npx")
        .args([
            "--yes",
            PACKAGE,
            "--port",
            &PORT.to_string(),
            "--host",
            "0.0.0.0",
            "--allowed-hosts",
            &allowed_hosts(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        // Own process group so shutdown can signal `npx` and the node server
        // it spawns together.
        .process_group(0)
        .spawn()
        .context("Failed to spawn Playwright MCP")?;

    let pid = child.id();
    save_state(
        &state_path,
        &PlaywrightState {
            pid: Some(pid),
            port: Some(PORT),
        },
    )?;

    wait_until_ready(pid, &log_path)?;

    eprintln!(
        "{} (PID {}, port {})",
        "Playwright MCP started.".green(),
        pid,
        PORT
    );

    Ok(PlaywrightServer {
        pid,
        owned: true,
        state_path,
    })
}

/// Poll the port until the server accepts connections, bailing early if the
/// process dies (e.g. the port is taken or `npx` failed to fetch the package).
fn wait_until_ready(pid: u32, log_path: &Path) -> Result<()> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        if port_is_open() {
            return Ok(());
        }
        if !is_process_alive(pid) {
            let log = std::fs::read_to_string(log_path).unwrap_or_default();
            anyhow::bail!(
                "Playwright MCP exited during startup. Log ({}):\n{}",
                log_path.display(),
                log.trim()
            );
        }
        if Instant::now() >= deadline {
            anyhow::bail!(
                "Playwright MCP did not start listening on port {} within {}s. See {}.",
                PORT,
                STARTUP_TIMEOUT.as_secs(),
                log_path.display()
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_url_uses_the_runtime_gateway() {
        assert_eq!(
            mcp_url("host.containers.internal"),
            "http://host.containers.internal:8931/mcp"
        );
        assert_eq!(
            mcp_url("host.docker.internal"),
            "http://host.docker.internal:8931/mcp"
        );
    }

    #[test]
    fn host_header_matches_playwrights_default_allow_list() {
        // Playwright MCP normalizes a 0.0.0.0 bind to `localhost:<port>` when
        // computing its default allowed-hosts entry.
        assert_eq!(host_header(), "localhost:8931");
    }

    #[test]
    fn allowed_hosts_cover_both_container_gateways() {
        let hosts = allowed_hosts();
        for expected in [
            "localhost:8931",
            "127.0.0.1:8931",
            "host.containers.internal:8931",
            "host.docker.internal:8931",
        ] {
            assert!(hosts.contains(expected), "missing {expected} in {hosts}");
        }
    }

    #[test]
    fn state_round_trips_through_disk() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("playwright.json");
        save_state(
            &path,
            &PlaywrightState {
                pid: Some(4242),
                port: Some(PORT),
            },
        )
        .unwrap();
        let loaded = load_state(&path);
        assert_eq!(loaded.pid, Some(4242));
        assert_eq!(loaded.port, Some(PORT));
    }

    #[test]
    fn state_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("playwright.json");
        save_state(&path, &PlaywrightState::default()).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn missing_state_file_loads_as_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let state = load_state(&dir.path().join("nope.json"));
        assert!(state.pid.is_none());
    }

    #[test]
    fn unowned_server_is_left_running_on_shutdown() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("playwright.json");
        save_state(&path, &PlaywrightState { pid: Some(1), port: Some(PORT) }).unwrap();
        let server = PlaywrightServer {
            // pid 1 would be fatal to signal; `owned: false` must short-circuit.
            pid: 1,
            owned: false,
            state_path: path.clone(),
        };
        server.shutdown();
        assert!(path.exists(), "state file must survive a non-owned shutdown");
    }
}
