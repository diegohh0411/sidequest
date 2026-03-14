use anyhow::{bail, Context, Result};
use bollard::models::HostConfig;
use bollard::Docker;
use std::path::PathBuf;
use uuid::Uuid;

use crate::config::load_config;
use crate::credentials::load_credentials;
use crate::docker::{run_container, ContainerOpts};
use crate::github::GitHubClient;

pub async fn cmd_run(repo: String, prompt: String, base_branch: Option<String>) -> Result<()> {
    let config = load_config()?;
    let task_id = Uuid::new_v4().to_string();
    let base_branch = base_branch.unwrap_or(config.github.default_base_branch.clone());

    // Resolve GH_TOKEN: env var takes precedence, then fall back to stored credentials
    let gh_token = std::env::var("GH_TOKEN")
        .ok()
        .or_else(|| load_credentials().ok().and_then(|c| c.gh_token))
        .context("GH_TOKEN is not set. Run `sidequest connect github` to store your token, or set the GH_TOKEN env var.")?;
    let anthropic_key = std::env::var("ANTHROPIC_API_KEY").ok();

    // If the user omitted the owner prefix, prepend their GitHub username automatically
    let gh_client = GitHubClient::new(gh_token.clone());
    let repo = gh_client.resolve_repo(repo).await?;

    let image_name = &config.docker.image_name;
    let container_name = format!("{}{}", config.docker.container_prefix, &task_id[..8]);

    println!("==> Task ID: {task_id}");
    println!("==> Repository: {repo}");
    println!("==> Base branch: {base_branch}");
    println!("==> Container: {container_name}");

    // Connect to Docker daemon
    let docker = Docker::connect_with_local_defaults()
        .context("Failed to connect to Docker daemon. Is Docker running?")?;

    // Resolve auth: API key takes precedence; fall back to mounting host ~/.claude session
    let (extra_env, host_config) = resolve_auth(anthropic_key)?;

    // Create the container with all env vars for the entrypoint
    let mut env_vars = vec![
        format!("REPO={repo}"),
        format!("GH_TOKEN={gh_token}"),
        format!("TASK_PROMPT={prompt}"),
        format!("TASK_ID={task_id}"),
        format!("BASE_BRANCH={base_branch}"),
        format!("BRANCH_PREFIX={}", config.github.branch_prefix),
        format!("CLAUDE_MODEL={}", config.claude.model),
        format!("CLAUDE_FAST_MODEL={}", config.claude.fast_model),
        format!("CLAUDE_MAX_TURNS={}", config.claude.max_turns),
    ];
    env_vars.extend(extra_env);

    let exit_code = run_container(
        &docker,
        ContainerOpts {
            image_name: image_name.clone(),
            container_name,
            env_vars,
            host_config,
            entrypoint: None,
        },
    )
    .await?;

    if exit_code != 0 {
        bail!("Container exited with code {exit_code}");
    }

    Ok(())
}

/// Resolve auth: API key takes precedence; fall back to mounting host ~/.claude session
pub fn resolve_auth(anthropic_key: Option<String>) -> Result<(Vec<String>, HostConfig)> {
    if let Some(key) = anthropic_key {
        println!("==> Auth: using ANTHROPIC_API_KEY");
        Ok((
            vec![format!("ANTHROPIC_API_KEY={key}")],
            HostConfig::default(),
        ))
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
            binds.push(format!(
                "{}:/home/agent/.claude.json:ro",
                claude_json.display()
            ));
        }
        Ok((
            vec![],
            HostConfig {
                binds: Some(binds),
                ..Default::default()
            },
        ))
    }
}
