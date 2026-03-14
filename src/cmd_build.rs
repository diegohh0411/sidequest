use anyhow::{bail, Context, Result};

use crate::config::load_config;

pub async fn cmd_build_image() -> Result<()> {
    let config = load_config()?;
    let image_name = &config.docker.image_name;

    println!("==> Building Docker image '{image_name}' from docker/ ...");

    let status = tokio::process::Command::new("docker")
        .args(["build", "-t", image_name, "docker/"])
        .status()
        .await
        .context("Failed to run 'docker build'. Is Docker installed and running?")?;

    if !status.success() {
        bail!("Docker build failed with exit code: {:?}", status.code());
    }

    println!("==> Image '{image_name}' built successfully.");
    Ok(())
}
