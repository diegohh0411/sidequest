use anyhow::{Context, Result};
use bollard::Docker;
use uuid::Uuid;

use crate::cmd_run::resolve_auth;
use crate::config::load_config;
use crate::credentials::load_credentials;
use crate::docker::{run_container, ContainerOpts};
use crate::github::GitHubClient;
use crate::state::{PrState, WatchState};

pub async fn cmd_watch(interval: u64) -> Result<()> {
    let config = load_config()?;

    let gh_token = std::env::var("GH_TOKEN")
        .ok()
        .or_else(|| load_credentials().ok().and_then(|c| c.gh_token))
        .context("GH_TOKEN is not set. Run `sidequest connect github` to store your token, or set the GH_TOKEN env var.")?;

    let watch_config = config.watch.clone();
    let bot_mention = watch_config
        .as_ref()
        .and_then(|w| w.bot_mention.clone())
        .unwrap_or_else(|| "@sidequest".to_string());

    let gh_client = GitHubClient::new(gh_token.clone());

    println!("==> Watching for PR review activity on sidequest branches");
    println!("    Branch prefix: {}", config.github.branch_prefix);
    println!("    Bot mention: {bot_mention}");
    println!("    Poll interval: {interval}s");
    println!();

    loop {
        if let Err(e) = poll_once(&gh_client, &gh_token, &config, &bot_mention).await {
            eprintln!("==> Poll error (will retry): {e}");
        }

        println!("==> Sleeping {interval}s...");
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
    }
}

async fn poll_once(
    gh_client: &GitHubClient,
    gh_token: &str,
    config: &crate::config::AppConfig,
    bot_mention: &str,
) -> Result<()> {
    let mut state = WatchState::load()?;

    println!("==> Searching for open sidequest PRs...");
    let prs = gh_client
        .search_sidequest_prs(&config.github.branch_prefix)
        .await?;

    if prs.is_empty() {
        println!("    No open sidequest PRs found.");
        return Ok(());
    }

    println!("    Found {} open PR(s)", prs.len());

    for pr in &prs {
        let repo = pr.repo_full_name();
        let pr_key = format!("{repo}#{}", pr.number);

        if let Err(e) = process_pr(
            gh_client,
            gh_token,
            config,
            &mut state,
            repo,
            pr.number,
            &pr_key,
            &pr.title,
            bot_mention,
        )
        .await
        {
            eprintln!("    Error processing {pr_key}: {e}");
            // Continue to next PR
        }
    }

    state.save()?;
    Ok(())
}

async fn process_pr(
    gh_client: &GitHubClient,
    gh_token: &str,
    config: &crate::config::AppConfig,
    state: &mut WatchState,
    repo: &str,
    pr_number: u64,
    pr_key: &str,
    pr_title: &str,
    bot_mention: &str,
) -> Result<()> {
    let prev_state = state.get(pr_key);

    // Fetch reviews and comments
    let reviews = gh_client.get_reviews(repo, pr_number).await?;
    let pr_comments = gh_client.get_pr_comments(repo, pr_number).await?;
    let review_comments = gh_client.get_review_comments(repo, pr_number).await?;

    // Find new "changes requested" reviews
    let new_change_requests: Vec<_> = reviews
        .iter()
        .filter(|r| r.id > prev_state.last_review_id && r.state == "CHANGES_REQUESTED")
        .collect();

    // Find new comments that @mention the bot (both issue comments and inline review comments)
    let new_mention_comments: Vec<_> = pr_comments
        .iter()
        .filter(|c| {
            c.id > prev_state.last_comment_id
                && c.body
                    .as_deref()
                    .map(|b| b.contains(bot_mention))
                    .unwrap_or(false)
        })
        .collect();

    let new_mention_review_comments: Vec<_> = review_comments
        .iter()
        .filter(|c| {
            c.id > prev_state.last_comment_id
                && c.body
                    .as_deref()
                    .map(|b| b.contains(bot_mention))
                    .unwrap_or(false)
        })
        .collect();

    // Track max IDs for dedup
    let max_review_id = reviews.iter().map(|r| r.id).max().unwrap_or(prev_state.last_review_id);
    let max_comment_id = pr_comments
        .iter()
        .map(|c| c.id)
        .chain(review_comments.iter().map(|c| c.id))
        .max()
        .unwrap_or(prev_state.last_comment_id);

    if new_change_requests.is_empty()
        && new_mention_comments.is_empty()
        && new_mention_review_comments.is_empty()
    {
        // Update state even if no action needed (so we don't re-scan old IDs)
        state.set(
            pr_key.to_string(),
            PrState {
                last_review_id: max_review_id,
                last_comment_id: max_comment_id,
            },
        );
        return Ok(());
    }

    // Build the review feedback prompt
    let mut feedback_parts = Vec::new();

    for review in &new_change_requests {
        let reviewer = review
            .user
            .as_ref()
            .map(|u| u.login.as_str())
            .unwrap_or("unknown");
        let body = review.body.as_deref().unwrap_or("(no body)");
        feedback_parts.push(format!("Review by @{reviewer} (changes requested):\n{body}"));
    }

    for comment in new_mention_comments
        .iter()
        .chain(new_mention_review_comments.iter())
    {
        let author = comment
            .user
            .as_ref()
            .map(|u| u.login.as_str())
            .unwrap_or("unknown");
        let body = comment.body.as_deref().unwrap_or("");
        feedback_parts.push(format!("Comment by @{author}:\n{body}"));
    }

    let feedback = feedback_parts.join("\n\n---\n\n");

    println!("    {pr_key}: \"{}\" — triggering review container", pr_title);
    println!(
        "      {} change request(s), {} mention comment(s)",
        new_change_requests.len(),
        new_mention_comments.len() + new_mention_review_comments.len()
    );

    // Spawn review-mode container
    spawn_review_container(gh_token, config, repo, pr_number, &feedback).await?;

    // Update dedup state
    state.set(
        pr_key.to_string(),
        PrState {
            last_review_id: max_review_id,
            last_comment_id: max_comment_id,
        },
    );

    Ok(())
}

async fn spawn_review_container(
    gh_token: &str,
    config: &crate::config::AppConfig,
    repo: &str,
    pr_number: u64,
    feedback: &str,
) -> Result<()> {
    let task_id = Uuid::new_v4().to_string();
    let container_name = format!("{}review-{}", config.docker.container_prefix, &task_id[..8]);
    let anthropic_key = std::env::var("ANTHROPIC_API_KEY").ok();

    let docker = Docker::connect_with_local_defaults()
        .context("Failed to connect to Docker daemon. Is Docker running?")?;

    let (extra_env, host_config) = resolve_auth(anthropic_key)?;

    let mut env_vars = vec![
        format!("REPO={repo}"),
        format!("GH_TOKEN={gh_token}"),
        format!("PR_NUMBER={pr_number}"),
        format!("REVIEW_FEEDBACK={feedback}"),
        format!("CLAUDE_MODEL={}", config.claude.model),
        format!("CLAUDE_FAST_MODEL={}", config.claude.fast_model),
        format!("CLAUDE_MAX_TURNS={}", config.claude.max_turns),
    ];
    env_vars.extend(extra_env);

    let exit_code = run_container(
        &docker,
        ContainerOpts {
            image_name: config.docker.image_name.clone(),
            container_name: container_name.clone(),
            env_vars,
            host_config,
            entrypoint: Some(vec!["/entrypoint-review.sh".to_string()]),
        },
    )
    .await?;

    if exit_code != 0 {
        eprintln!("    Review container for {repo}#{pr_number} exited with code {exit_code}");
    }

    Ok(())
}
