use anyhow::{bail, Context, Result};
use bollard::container::LogOutput;
use bollard::models::{ContainerCreateBody, HostConfig};
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, LogsOptionsBuilder, RemoveContainerOptionsBuilder,
    WaitContainerOptionsBuilder,
};
use bollard::Docker;
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{self, AsyncWriteExt};
use uuid::Uuid;

// ── CLI ──────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "sidequest",
    version,
    about = "Lightweight CLI that runs Claude Code in disposable Docker containers to implement features and open PRs."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build the sidequest Docker image locally
    BuildImage,

    /// Run a coding task in a disposable container
    Run {
        /// Target repository in "owner/repo" format
        #[arg(long)]
        repo: String,

        /// Natural language task description for Claude Code
        #[arg(long)]
        prompt: String,

        /// Base branch to work from (default: from config or "main")
        #[arg(long)]
        base_branch: Option<String>,
    },

    /// Show logs from a past task run (if the container still exists)
    Logs {
        /// Task ID (UUID) of the run
        #[arg(long)]
        task_id: String,
    },

    /// Connect to external services and store credentials in ~/.sidequest/credentials
    Connect {
        #[command(subcommand)]
        service: ConnectService,
    },
}

#[derive(Subcommand)]
enum ConnectService {
    /// Save a GitHub personal access token (needs repo + workflow scopes)
    Github,
}

// ── Credentials ──────────────────────────────────────────────────────────────

#[derive(Deserialize, Serialize, Default)]
struct Credentials {
    gh_token: Option<String>,
}

fn sidequest_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".sidequest")
}

fn credentials_path() -> PathBuf {
    sidequest_dir().join("credentials")
}

fn load_credentials() -> Result<Credentials> {
    let path = credentials_path();
    if !path.exists() {
        return Ok(Credentials::default());
    }
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read credentials: {}", path.display()))?;
    toml::from_str(&contents)
        .with_context(|| format!("Failed to parse credentials: {}", path.display()))
}

fn save_credentials(creds: &Credentials) -> Result<()> {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    let dir = sidequest_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create directory: {}", dir.display()))?;

    let path = credentials_path();
    let contents = toml::to_string(creds).context("Failed to serialize credentials")?;
    std::fs::write(&path, &contents)
        .with_context(|| format!("Failed to write credentials: {}", path.display()))?;

    // Restrict to owner read/write only (600) — prevents other users from reading the token
    #[cfg(unix)]
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("Failed to set permissions on: {}", path.display()))?;

    Ok(())
}

// ── Config ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AppConfig {
    docker: DockerConfig,
    github: GithubConfig,
    claude: ClaudeConfig,
}

#[derive(Deserialize)]
struct DockerConfig {
    image_name: String,
    container_prefix: String,
}

#[derive(Deserialize)]
struct GithubConfig {
    default_base_branch: String,
    branch_prefix: String,
}

#[derive(Deserialize)]
struct ClaudeConfig {
    model: String,
    max_turns: i64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            docker: DockerConfig {
                image_name: "sidequest-agent".to_string(),
                container_prefix: "sq-task-".to_string(),
            },
            github: GithubConfig {
                default_base_branch: "main".to_string(),
                branch_prefix: "sidequest/".to_string(),
            },
            claude: ClaudeConfig {
                model: "sonnet".to_string(),
                max_turns: -1,
            },
        }
    }
}

/// Load config from config.toml in current dir or ~/.config/sidequest/config.toml.
/// Falls back to defaults if neither file exists.
fn load_config() -> Result<AppConfig> {
    let candidates: Vec<PathBuf> = vec![PathBuf::from("config.toml"), dirs_config_path()];

    for path in &candidates {
        if path.exists() {
            let contents = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read config file: {}", path.display()))?;
            let config: AppConfig = toml::from_str(&contents)
                .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
            return Ok(config);
        }
    }

    // No config file found — use defaults
    Ok(AppConfig::default())
}

fn dirs_config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("sidequest")
        .join("config.toml")
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::BuildImage => cmd_build_image().await,
        Commands::Run {
            repo,
            prompt,
            base_branch,
        } => cmd_run(repo, prompt, base_branch).await,
        Commands::Logs { task_id } => cmd_logs(task_id).await,
        Commands::Connect { service } => match service {
            ConnectService::Github => cmd_connect_github(),
        },
    }
}

// ── build-image ──────────────────────────────────────────────────────────────

async fn cmd_build_image() -> Result<()> {
    let config = load_config()?;
    let image_name = &config.docker.image_name;

    println!("==> Building Docker image '{image_name}' from docker/ ...");

    // Shell out to `docker build` for simplicity (MVP approach)
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

// ── run ──────────────────────────────────────────────────────────────────────

async fn cmd_run(repo: String, prompt: String, base_branch: Option<String>) -> Result<()> {
    let config = load_config()?;
    let task_id = Uuid::new_v4().to_string();
    let base_branch = base_branch.unwrap_or(config.github.default_base_branch.clone());

    // Resolve GH_TOKEN: env var takes precedence, then fall back to stored credentials
    let gh_token = std::env::var("GH_TOKEN").ok()
        .or_else(|| load_credentials().ok().and_then(|c| c.gh_token))
        .context("GH_TOKEN is not set. Run `sidequest connect github` to store your token, or set the GH_TOKEN env var.")?;
    let anthropic_key = std::env::var("ANTHROPIC_API_KEY").ok();

    let image_name = &config.docker.image_name;
    let container_name = format!("{}{}", config.docker.container_prefix, &task_id[..8]);

    println!("==> Task ID: {task_id}");
    println!("==> Repository: {repo}");
    println!("==> Base branch: {base_branch}");
    println!("==> Container: {container_name}");

    // Connect to Docker daemon
    let docker = Docker::connect_with_local_defaults()
        .context("Failed to connect to Docker daemon. Is Docker running?")?;

    // Check that the image exists
    if docker.inspect_image(image_name).await.is_err() {
        bail!("Docker image '{image_name}' not found. Run `sidequest build-image` first.");
    }

    // Resolve auth: API key takes precedence; fall back to mounting host ~/.claude session
    let (extra_env, host_config) = if let Some(key) = anthropic_key {
        println!("==> Auth: using ANTHROPIC_API_KEY");
        (
            vec![format!("ANTHROPIC_API_KEY={key}")],
            HostConfig::default(),
        )
    } else {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .context("Neither HOME nor USERPROFILE environment variable is set")?;
        let claude_path = PathBuf::from(&home).join(".claude");
        if !claude_path.exists() {
            bail!(
                "No ANTHROPIC_API_KEY set and ~/.claude not found at {}. \
                Run `claude` once interactively to log in, or set ANTHROPIC_API_KEY.",
                claude_path.display()
            );
        }
        println!("==> Auth: mounting host ~/.claude session (no API key)");
        let claude_json = PathBuf::from(&home).join(".claude.json");
        let mut binds = vec![format!("{}:/home/agent/.claude:ro", claude_path.display())];
        if claude_json.exists() {
            binds.push(format!("{}:/home/agent/.claude.json:ro", claude_json.display()));
        }
        (
            vec![],
            HostConfig {
                binds: Some(binds),
                ..Default::default()
            },
        )
    };

    // Create the container with all env vars for the entrypoint
    let mut env_vars = vec![
        format!("REPO={repo}"),
        format!("GH_TOKEN={gh_token}"),
        format!("TASK_PROMPT={prompt}"),
        format!("TASK_ID={task_id}"),
        format!("BASE_BRANCH={base_branch}"),
        format!("BRANCH_PREFIX={}", config.github.branch_prefix),
        format!("CLAUDE_MODEL={}", config.claude.model),
        format!("CLAUDE_MAX_TURNS={}", config.claude.max_turns),
    ];
    env_vars.extend(extra_env);

    let container_config = ContainerCreateBody {
        image: Some(image_name.clone()),
        env: Some(env_vars),
        host_config: Some(host_config),
        tty: Some(true),
        ..Default::default()
    };

    let create_opts = CreateContainerOptionsBuilder::default()
        .name(&container_name)
        .build();

    println!("==> Creating container...");
    docker
        .create_container(Some(create_opts), container_config)
        .await
        .context("Failed to create container")?;

    // Start the container
    println!("==> Starting container...");
    docker
        .start_container(&container_name, None)
        .await
        .context("Failed to start container")?;

    // Stream container logs to terminal in real-time
    println!("==> Streaming container output...\n");
    let log_opts = LogsOptionsBuilder::default()
        .follow(true)
        .stdout(true)
        .stderr(true)
        .build();

    let mut log_stream = docker.logs(&container_name, Some(log_opts));
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();

    while let Some(log_result) = log_stream.next().await {
        match log_result {
            Ok(output) => match output {
                LogOutput::StdOut { message } => {
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
    let mut wait_stream = docker.wait_container(&container_name, Some(wait_opts));

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
    let remove_opts = RemoveContainerOptionsBuilder::default().force(true).build();
    docker
        .remove_container(&container_name, Some(remove_opts))
        .await
        .context("Failed to remove container")?;

    if exit_code != 0 {
        bail!("Container exited with code {exit_code}");
    }

    Ok(())
}

// ── connect ──────────────────────────────────────────────────────────────────

fn cmd_connect_github() -> Result<()> {
    println!("==> Connecting to GitHub");
    println!("    Enter a GitHub personal access token with repo + workflow scopes.");
    println!("    It will be saved to: {}", credentials_path().display());
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
    println!("==> GitHub token saved to {}", credentials_path().display());
    println!("    `sidequest run` will now use it automatically.");
    Ok(())
}

// ── logs ─────────────────────────────────────────────────────────────────────

async fn cmd_logs(task_id: String) -> Result<()> {
    let config = load_config()?;
    // The container name is prefix + first 8 chars of task_id
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
                LogOutput::StdOut { message } | LogOutput::StdErr { message } => {
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
