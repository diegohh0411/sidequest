use anyhow::{bail, Context, Result};
use bollard::container::LogOutput;
use bollard::query_parameters::LogsOptionsBuilder;
use bollard::Docker;
use futures_util::StreamExt;
use tokio::io::{self, AsyncWriteExt};

use crate::config::load_config;

pub async fn cmd_logs(task_id: String) -> Result<()> {
    let config = load_config()?;
    let short_id = if task_id.len() >= 8 {
        &task_id[..8]
    } else {
        &task_id
    };
    let container_name = format!("{}{}", config.docker.container_prefix, short_id);

    println!("==> Fetching logs for container '{container_name}'...\n");

    let docker = Docker::connect_with_local_defaults()
        .context("Failed to connect to Docker daemon. Is Docker running?")?;

    let log_opts = LogsOptionsBuilder::default()
        .follow(false)
        .stdout(true)
        .stderr(true)
        .build();

    let mut log_stream = docker.logs(&container_name, Some(log_opts));
    let mut stdout = io::stdout();

    while let Some(log_result) = log_stream.next().await {
        match log_result {
            Ok(output) => match output {
                LogOutput::StdOut { message }
                | LogOutput::StdErr { message }
                | LogOutput::Console { message } => {
                    stdout.write_all(&message).await?;
                }
                _ => {}
            },
            Err(e) => {
                bail!("Failed to read logs from container '{container_name}': {e}");
            }
        }
    }

    stdout.flush().await?;
    Ok(())
}
