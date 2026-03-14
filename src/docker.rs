use anyhow::{bail, Context, Result};
use bollard::container::LogOutput;
use bollard::models::{ContainerCreateBody, HostConfig};
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, LogsOptionsBuilder, RemoveContainerOptionsBuilder,
    WaitContainerOptionsBuilder,
};
use bollard::Docker;
use futures_util::StreamExt;
use tokio::io::{self, AsyncWriteExt};

pub struct ContainerOpts {
    pub image_name: String,
    pub container_name: String,
    pub env_vars: Vec<String>,
    pub host_config: HostConfig,
    pub entrypoint: Option<Vec<String>>,
}

/// Create, run, stream logs, wait, and remove a container. Returns exit code.
pub async fn run_container(docker: &Docker, opts: ContainerOpts) -> Result<i64> {
    // Check that the image exists
    if docker.inspect_image(&opts.image_name).await.is_err() {
        bail!(
            "Docker image '{}' not found. Run `sidequest build-image` first.",
            opts.image_name
        );
    }

    let mut container_config = ContainerCreateBody {
        image: Some(opts.image_name),
        env: Some(opts.env_vars),
        host_config: Some(opts.host_config),
        tty: Some(true),
        ..Default::default()
    };

    if let Some(ep) = opts.entrypoint {
        container_config.entrypoint = Some(ep);
    }

    let create_opts = CreateContainerOptionsBuilder::default()
        .name(&opts.container_name)
        .build();

    println!("==> Creating container...");
    docker
        .create_container(Some(create_opts), container_config)
        .await
        .context("Failed to create container")?;

    // Start the container
    println!("==> Starting container...");
    docker
        .start_container(&opts.container_name, None)
        .await
        .context("Failed to start container")?;

    // Stream container logs to terminal in real-time
    println!("==> Streaming container output...\n");
    let log_opts = LogsOptionsBuilder::default()
        .follow(true)
        .stdout(true)
        .stderr(true)
        .build();

    let mut log_stream = docker.logs(&opts.container_name, Some(log_opts));
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();

    while let Some(log_result) = log_stream.next().await {
        match log_result {
            Ok(output) => match output {
                LogOutput::StdOut { message } | LogOutput::Console { message } => {
                    stdout.write_all(&message).await?;
                    stdout.flush().await?;
                }
                LogOutput::StdErr { message } => {
                    stderr.write_all(&message).await?;
                    stderr.flush().await?;
                }
                _ => {}
            },
            Err(e) => {
                eprintln!("Error reading container logs: {e}");
                break;
            }
        }
    }

    // Wait for the container to finish
    let wait_opts = WaitContainerOptionsBuilder::default()
        .condition("not-running")
        .build();
    let mut wait_stream = docker.wait_container(&opts.container_name, Some(wait_opts));

    let exit_code = match wait_stream.next().await {
        Some(Ok(response)) => response.status_code,
        Some(Err(e)) => {
            eprintln!("Error waiting for container: {e}");
            1
        }
        None => {
            eprintln!("Container wait stream ended unexpectedly");
            1
        }
    };

    println!();
    if exit_code == 0 {
        println!("==> Task completed successfully (exit code 0).");
    } else {
        println!("==> Task failed with exit code {exit_code}.");
    }

    // Clean up: force-remove the container
    println!("==> Removing container...");
    let remove_opts = RemoveContainerOptionsBuilder::default()
        .force(true)
        .build();
    docker
        .remove_container(&opts.container_name, Some(remove_opts))
        .await
        .context("Failed to remove container")?;

    Ok(exit_code)
}
