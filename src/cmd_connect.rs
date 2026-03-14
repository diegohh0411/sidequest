use anyhow::{bail, Context, Result};

use crate::credentials::{credentials_path, load_credentials, save_credentials};

pub fn cmd_connect_github() -> Result<()> {
    println!("==> Connecting to GitHub");
    println!("    Enter a GitHub personal access token with repo + workflow scopes.");
    println!("    It will be saved to: {}", credentials_path()?.display());
    println!();

    let token = rpassword::prompt_password("    Token: ")
        .context("Failed to read token from terminal")?;

    if token.trim().is_empty() {
        bail!("Token cannot be empty.");
    }

    let mut creds = load_credentials()?;
    creds.gh_token = Some(token.trim().to_string());
    save_credentials(&creds)?;

    println!();
    println!("==> GitHub token saved to {}", credentials_path()?.display());
    println!("    `sidequest run` will now use it automatically.");
    Ok(())
}
