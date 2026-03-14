mod cmd_build;
mod cmd_connect;
mod cmd_logs;
mod cmd_run;
mod cmd_watch;
mod config;
mod credentials;
mod docker;
mod github;
mod state;

use anyhow::Result;
use clap::{Parser, Subcommand};

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
        /// Target repository in "owner/repo" format (default: detected from current directory's git remote)
        #[arg(long)]
        repo: Option<String>,

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

    /// Watch for PR review activity and auto-respond with fixes
    Watch {
        /// Poll interval in seconds (default: from config or 60)
        #[arg(long)]
        interval: Option<u64>,
    },
}

#[derive(Subcommand)]
enum ConnectService {
    /// Save a GitHub personal access token (needs repo + workflow scopes)
    Github,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::BuildImage => cmd_build::cmd_build_image().await,
        Commands::Run {
            repo,
            prompt,
            base_branch,
        } => cmd_run::cmd_run(repo, prompt, base_branch).await,
        Commands::Logs { task_id } => cmd_logs::cmd_logs(task_id).await,
        Commands::Connect { service } => match service {
            ConnectService::Github => cmd_connect::cmd_connect_github(),
        },
        Commands::Watch { interval } => cmd_watch::cmd_watch(interval).await,
    }
}
