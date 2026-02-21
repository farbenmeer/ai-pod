use anyhow::{Context, Result};
use colored::Colorize;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;

use crate::config::AppConfig;

pub const IMAGE_NAME: &str = "ai-pod:latest";

fn hash_dockerfile(path: &Path) -> Result<String> {
    let content = std::fs::read(path).context("Failed to read Dockerfile")?;
    let hash = Sha256::digest(&content);
    Ok(hex::encode(hash))
}

fn image_exists() -> Result<bool> {
    let status = Command::new("podman")
        .args(["image", "exists", IMAGE_NAME])
        .status()
        .context("Failed to run podman")?;
    Ok(status.success())
}

fn read_stored_hash(hash_file: &Path) -> Option<String> {
    std::fs::read_to_string(hash_file)
        .ok()
        .map(|s| s.trim().to_string())
}

pub fn needs_build(config: &AppConfig, force: bool) -> Result<bool> {
    if force {
        return Ok(true);
    }

    if !image_exists()? {
        return Ok(true);
    }

    let current_hash = hash_dockerfile(&config.dockerfile_path)?;
    match read_stored_hash(&config.hash_file) {
        Some(stored) if stored == current_hash => Ok(false),
        _ => Ok(true),
    }
}

pub fn build_image(config: &AppConfig) -> Result<()> {
    println!("{}", "Building container image...".blue().bold());

    let status = Command::new("podman")
        .args([
            "build",
            "-t",
            IMAGE_NAME,
            "-f",
            &config.dockerfile_path.to_string_lossy(),
            &config.config_dir.to_string_lossy(),
        ])
        .status()
        .context("Failed to run podman build")?;

    if !status.success() {
        anyhow::bail!("podman build failed");
    }

    let hash = hash_dockerfile(&config.dockerfile_path)?;
    std::fs::write(&config.hash_file, &hash).context("Failed to write image hash")?;

    println!("{}", "Image built successfully.".green().bold());
    Ok(())
}

pub fn ensure_image(config: &AppConfig, force: bool) -> Result<()> {
    if needs_build(config, force)? {
        build_image(config)?;
    } else {
        println!("{}", "Container image is up to date.".green());
    }
    Ok(())
}
