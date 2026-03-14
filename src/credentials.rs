use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Deserialize, Serialize, Default)]
pub struct Credentials {
    pub gh_token: Option<String>,
}

pub fn sidequest_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("Could not determine home directory: neither HOME nor USERPROFILE is set")?;
    Ok(PathBuf::from(home).join(".sidequest"))
}

pub fn credentials_path() -> Result<PathBuf> {
    Ok(sidequest_dir()?.join("credentials"))
}

pub fn load_credentials() -> Result<Credentials> {
    let path = credentials_path()?;
    if !path.exists() {
        return Ok(Credentials::default());
    }
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read credentials: {}", path.display()))?;
    toml::from_str(&contents)
        .with_context(|| format!("Failed to parse credentials: {}", path.display()))
}

pub fn save_credentials(creds: &Credentials) -> Result<()> {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    let dir = sidequest_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create directory: {}", dir.display()))?;

    let path = credentials_path()?;
    let contents = toml::to_string(creds).context("Failed to serialize credentials")?;
    std::fs::write(&path, &contents)
        .with_context(|| format!("Failed to write credentials: {}", path.display()))?;

    // Restrict to owner read/write only (600) — prevents other users from reading the token
    #[cfg(unix)]
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("Failed to set permissions on: {}", path.display()))?;

    Ok(())
}
