mod cli;
mod config;
mod container;
mod credentials;
mod image;
mod server;

use anyhow::Result;
use clap::Parser;
use colored::Colorize;

use cli::{Cli, Command};
use config::AppConfig;

fn launch_flow(cli: &Cli) -> Result<()> {
    let config = AppConfig::new()?;
    config.init()?;

    // 1. Resolve workspace
    let workspace = match &cli.workdir {
        Some(p) => std::fs::canonicalize(p)?,
        None => std::env::current_dir()?,
    };
    println!("{} {}", "Workspace:".blue(), workspace.display());

    // 2. Credential scan
    if !cli.no_credential_check {
        if !credentials::check_credentials(&workspace)? {
            println!("{}", "Aborted.".red());
            return Ok(());
        }
    }

    // 3. Build image if needed
    image::ensure_image(&config, cli.rebuild)?;

    // 4. Ensure notification server
    server::lifecycle::ensure_server(&config.pid_file, &config.log_file, cli.notify_port)?;

    // 5 & 6. Generate settings + launch container
    container::launch_container(&config, &workspace, cli.notify_port)?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Command::Build) => {
            let config = AppConfig::new()?;
            config.init()?;
            image::ensure_image(&config, cli.rebuild)?;
        }
        Some(Command::ServeNotifications) => {
            server::run_server(cli.notify_port).await?;
        }
        Some(Command::StopServer) => {
            let config = AppConfig::new()?;
            server::lifecycle::stop_server(&config.pid_file)?;
        }
        Some(Command::ServerStatus) => {
            let config = AppConfig::new()?;
            server::lifecycle::print_status(&config.pid_file, cli.notify_port);
        }
        Some(Command::List) => {
            container::list_containers()?;
        }
        Some(Command::Clean { workdir }) => {
            let workspace = match workdir {
                Some(p) => std::fs::canonicalize(p)?,
                None => std::env::current_dir()?,
            };
            container::clean_container(&workspace)?;
        }
        None => {
            launch_flow(&cli)?;
        }
    }

    Ok(())
}
