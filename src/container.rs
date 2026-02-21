use anyhow::{Context, Result};
use colored::Colorize;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;

use crate::config::AppConfig;
use crate::image::IMAGE_NAME;

const CONTAINER_CLAUDE_MD: &str = r#"# Container Environment
You are running inside a Podman container. To reach services on the host machine,
use `host.containers.internal` instead of `localhost`.

For example: `curl http://host.containers.internal:3000`

Working directory: /app
"#;

fn generate_container_name(workspace: &Path) -> String {
    let workspace_str = workspace.to_string_lossy();
    let hash = Sha256::digest(workspace_str.as_bytes());
    let short_hash = hex::encode(&hash[..6]);
    format!("claude-{}", short_hash)
}

fn container_exists(name: &str) -> Result<bool> {
    let output = Command::new("podman")
        .args([
            "ps",
            "-a",
            "--filter",
            &format!("name=^{}$", name),
            "--format",
            "{{.Names}}",
        ])
        .output()
        .context("Failed to check if container exists")?;

    Ok(!output.stdout.is_empty())
}

fn container_is_running(name: &str) -> Result<bool> {
    let output = Command::new("podman")
        .args([
            "ps",
            "--filter",
            &format!("name=^{}$", name),
            "--format",
            "{{.Names}}",
        ])
        .output()
        .context("Failed to check if container is running")?;

    Ok(!output.stdout.is_empty())
}

fn generate_runtime_claude_md(config: &AppConfig) -> Result<()> {
    let mut content = CONTAINER_CLAUDE_MD.to_string();

    let host_claude_md = config.claude_md_path();
    if host_claude_md.exists() {
        let existing = std::fs::read_to_string(&host_claude_md)
            .context("Failed to read existing CLAUDE.md")?;
        content.push('\n');
        content.push_str(&existing);
    }

    std::fs::write(&config.runtime_claude_md, content)
        .context("Failed to write runtime CLAUDE.md")?;

    Ok(())
}

fn generate_runtime_settings(config: &AppConfig, port: u16) -> Result<()> {
    let mut settings: serde_json::Value = if config.claude_settings_path().exists() {
        let raw = std::fs::read_to_string(config.claude_settings_path())
            .context("Failed to read settings.json")?;
        serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let hook_command = format!(
        "curl -sf -X POST http://host.containers.internal:{}/notify || true",
        port
    );

    let stop_hook = serde_json::json!([{
        "matcher": "*",
        "hooks": [{
            "type": "command",
            "command": hook_command
        }]
    }]);

    let obj = settings
        .as_object_mut()
        .context("settings.json is not an object")?;

    let hooks = obj.entry("hooks").or_insert_with(|| serde_json::json!({}));
    let hooks_obj = hooks.as_object_mut().context("hooks is not an object")?;
    hooks_obj.insert("Stop".to_string(), stop_hook);

    let output = serde_json::to_string_pretty(&settings)?;
    std::fs::write(&config.runtime_settings, output).context("Failed to write runtime settings")?;

    Ok(())
}

fn create_container(
    config: &AppConfig,
    workspace: &Path,
    container_name: &str,
    port: u16,
) -> Result<()> {
    generate_runtime_claude_md(config)?;
    generate_runtime_settings(config, port)?;

    let workspace_str = workspace.to_string_lossy();
    let volume_name = format!("{}-data", container_name);

    let mut args: Vec<String> = vec![
        "run".into(),
        "-dit".into(),
        "--init".into(),
        "--name".into(),
        container_name.to_string(),
    ];

    // Workspace mount
    args.push("-v".into());
    args.push(format!("{}:/app:Z", workspace_str));

    // Persistent volume for Claude data
    args.push("-v".into());
    args.push(format!("{}:/home/claude/.claude", volume_name));

    // Host gateway
    args.push("--add-host=host.containers.internal:host-gateway".into());

    // Environment variables
    args.push("-e".into());
    args.push("HOST_GATEWAY=host.containers.internal".into());
    args.push("-e".into());
    args.push(format!(
        "NOTIFY_URL=http://host.containers.internal:{}/notify",
        port
    ));

    // Image
    args.push(IMAGE_NAME.into());

    println!("{} {}", "Creating container:".blue().bold(), container_name);

    let status = Command::new("podman")
        .args(&args)
        .status()
        .context("Failed to create container")?;

    if !status.success() {
        anyhow::bail!("Failed to create container");
    }

    // Copy merged CLAUDE.md
    Command::new("podman")
        .args([
            "cp",
            &config.runtime_claude_md.to_string_lossy(),
            &format!("{}:/home/claude/.claude/CLAUDE.md", container_name),
        ])
        .status()
        .context("Failed to copy CLAUDE.md")?;

    // Copy merged settings.json
    Command::new("podman")
        .args([
            "cp",
            &config.runtime_settings.to_string_lossy(),
            &format!("{}:/home/claude/.claude/settings.json", container_name),
        ])
        .status()
        .context("Failed to copy settings.json")?;

    println!("{}", "Container created successfully.".green());

    Ok(())
}

fn attach_to_container(container_name: &str) -> Result<()> {
    println!(
        "{} {}",
        "Attaching to container:".blue().bold(),
        container_name
    );

    let status = Command::new("podman")
        .args(["attach", container_name])
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .context("Failed to attach to container")?;

    if !status.success() {
        anyhow::bail!("Failed to attach to container");
    }

    Ok(())
}

fn start_container(container_name: &str) -> Result<()> {
    println!("{} {}", "Starting container:".blue().bold(), container_name);

    let status = Command::new("podman")
        .args(["start", container_name])
        .status()
        .context("Failed to start container")?;

    if !status.success() {
        anyhow::bail!("Failed to start container");
    }

    Ok(())
}

pub fn launch_container(config: &AppConfig, workspace: &Path, port: u16) -> Result<()> {
    let container_name = generate_container_name(workspace);

    if container_exists(&container_name)? {
        println!("{} {}", "Found existing container:".green(), container_name);

        if !container_is_running(&container_name)? {
            start_container(&container_name)?;
        }

        attach_to_container(&container_name)?;
    } else {
        create_container(config, workspace, &container_name, port)?;
        attach_to_container(&container_name)?;
    }

    Ok(())
}

pub fn list_containers() -> Result<()> {
    let output = Command::new("podman")
        .args([
            "ps",
            "-a",
            "--filter",
            "name=^claude-",
            "--format",
            "{{.Names}}\t{{.Status}}\t{{.CreatedAt}}",
        ])
        .output()
        .context("Failed to list containers")?;

    if output.stdout.is_empty() {
        println!("{}", "No claude containers found.".yellow());
    } else {
        println!("{}", "Claude containers:".blue().bold());
        println!("{:<20} {:<30} {}", "NAME", "STATUS", "CREATED");
        println!("{}", "-".repeat(80));
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }

    Ok(())
}

pub fn clean_container(workspace: &Path) -> Result<()> {
    let container_name = generate_container_name(workspace);

    if !container_exists(&container_name)? {
        println!(
            "{} {}",
            "Container does not exist:".yellow(),
            container_name
        );
        return Ok(());
    }

    println!("{} {}", "Removing container:".red().bold(), container_name);

    // Stop if running
    if container_is_running(&container_name)? {
        Command::new("podman")
            .args(["stop", &container_name])
            .status()
            .context("Failed to stop container")?;
    }

    // Remove container
    Command::new("podman")
        .args(["rm", &container_name])
        .status()
        .context("Failed to remove container")?;

    // Remove associated volume
    let volume_name = format!("{}-data", container_name);
    let _ = Command::new("podman")
        .args(["volume", "rm", &volume_name])
        .status();

    println!("{}", "Container removed.".green());

    Ok(())
}
