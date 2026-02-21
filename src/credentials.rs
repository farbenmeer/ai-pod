use anyhow::Result;
use colored::Colorize;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const CREDENTIAL_PATTERNS: &[&str] = &[
    ".env",
    ".env.local",
    ".env.production",
    ".env.staging",
    "id_rsa",
    "id_ed25519",
    "id_ecdsa",
    "id_dsa",
    ".npmrc",
    ".pypirc",
    ".netrc",
    "credentials.json",
    "service-account.json",
    "terraform.tfstate",
];

const CREDENTIAL_EXTENSIONS: &[&str] = &[
    "pem", "key", "p12", "pfx", "jks", "keystore", "tfvars",
];

const CREDENTIAL_DIR_PATTERNS: &[&str] = &[
    ".aws/credentials",
    ".aws/config",
    ".ssh/",
    ".gnupg/",
];

fn is_credential_file(path: &Path) -> bool {
    let file_name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };

    if CREDENTIAL_PATTERNS.iter().any(|p| file_name == *p) {
        return true;
    }

    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if CREDENTIAL_EXTENSIONS.iter().any(|e| ext == *e) {
            return true;
        }
    }

    let path_str = path.to_string_lossy();
    if CREDENTIAL_DIR_PATTERNS.iter().any(|p| path_str.contains(p)) {
        return true;
    }

    false
}

pub fn scan_workspace(workspace: &Path) -> Vec<PathBuf> {
    WalkDir::new(workspace)
        .max_depth(5)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip common non-relevant directories
            !matches!(
                name.as_ref(),
                "node_modules" | ".git" | "target" | "__pycache__" | ".venv" | "venv"
            )
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| is_credential_file(e.path()))
        .map(|e| e.into_path())
        .collect()
}

pub fn check_credentials(workspace: &Path) -> Result<bool> {
    let found = scan_workspace(workspace);
    if found.is_empty() {
        return Ok(true);
    }

    println!(
        "\n{}",
        "⚠  Potential credential files found in workspace:"
            .yellow()
            .bold()
    );
    for path in &found {
        let relative = path.strip_prefix(workspace).unwrap_or(path);
        println!("  {} {}", "•".yellow(), relative.display());
    }
    println!(
        "\n{}",
        "These files will be accessible inside the container."
            .yellow()
    );

    let proceed = dialoguer::Confirm::new()
        .with_prompt("Continue anyway?")
        .default(false)
        .interact()?;

    Ok(proceed)
}
