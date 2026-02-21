use anyhow::{Context, Result};
use std::path::PathBuf;

pub struct AppConfig {
    pub config_dir: PathBuf,
    pub dockerfile_path: PathBuf,
    pub pid_file: PathBuf,
    pub log_file: PathBuf,
    pub hash_file: PathBuf,
    pub runtime_settings: PathBuf,
    pub runtime_claude_md: PathBuf,
    pub home_dir: PathBuf,
}

impl AppConfig {
    pub fn new() -> Result<Self> {
        let home_dir = dirs::home_dir().context("Could not determine home directory")?;
        let config_dir = home_dir.join(".ai-pod");

        Ok(Self {
            dockerfile_path: config_dir.join("Dockerfile"),
            pid_file: config_dir.join("server.pid"),
            log_file: config_dir.join("server.log"),
            hash_file: config_dir.join("image.sha256"),
            runtime_settings: config_dir.join("runtime-settings.json"),
            runtime_claude_md: config_dir.join("runtime-CLAUDE.md"),
            config_dir,
            home_dir,
        })
    }

    pub fn init(&self) -> Result<()> {
        std::fs::create_dir_all(&self.config_dir).context("Failed to create ~/.ai-pod/")?;

        if !self.dockerfile_path.exists() {
            let default = include_str!("../claude.Dockerfile");
            std::fs::write(&self.dockerfile_path, default)
                .context("Failed to write default Dockerfile")?;
            println!(
                "Created default Dockerfile at {}",
                self.dockerfile_path.display()
            );
        }

        Ok(())
    }

    pub fn claude_settings_path(&self) -> PathBuf {
        self.home_dir.join(".claude").join("settings.json")
    }

    pub fn claude_md_path(&self) -> PathBuf {
        self.home_dir.join(".claude").join("CLAUDE.md")
    }
}
