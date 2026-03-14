use anyhow::{bail, Context, Result};
use serde::Deserialize;

pub struct GitHubClient {
    client: reqwest::Client,
    token: String,
}

#[derive(Debug, Deserialize)]
pub struct SearchResult {
    pub items: Vec<SearchItem>,
}

#[derive(Debug, Deserialize)]
pub struct SearchItem {
    pub number: u64,
    pub title: String,
    pub html_url: String,
    /// "owner/repo" extracted from repository_url
    pub repository_url: String,
}

impl SearchItem {
    /// Extract "owner/repo" from repository_url like "https://api.github.com/repos/owner/repo"
    pub fn repo_full_name(&self) -> &str {
        self.repository_url
            .strip_prefix("https://api.github.com/repos/")
            .unwrap_or(&self.repository_url)
    }
}

#[derive(Debug, Deserialize)]
pub struct Review {
    pub id: u64,
    pub state: String,
    pub body: Option<String>,
    pub user: Option<User>,
}

#[derive(Debug, Deserialize)]
pub struct Comment {
    pub id: u64,
    pub body: Option<String>,
    pub user: Option<User>,
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub login: String,
}

impl GitHubClient {
    pub fn new(token: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            token,
        }
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let resp = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "sidequest-cli")
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .with_context(|| format!("Failed to reach GitHub API: {url}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("GitHub API returned {status} for {url}: {body}");
        }

        resp.json()
            .await
            .with_context(|| format!("Failed to parse GitHub API response from {url}"))
    }

    /// Resolve authenticated user's login name
    pub async fn resolve_user(&self) -> Result<String> {
        let json: serde_json::Value = self.get_json("https://api.github.com/user").await?;
        json["login"]
            .as_str()
            .map(|s| s.to_string())
            .context("GitHub API response did not contain a login field")
    }

    /// If `repo` has no `/`, resolve the authenticated GitHub user and prepend them.
    pub async fn resolve_repo(&self, repo: String) -> Result<String> {
        if repo.contains('/') {
            return Ok(repo);
        }
        let login = self.resolve_user().await?;
        Ok(format!("{login}/{repo}"))
    }

    /// Search for all open PRs with branches matching the given prefix.
    /// Uses GitHub Search API — one call finds all repos the token can access.
    pub async fn search_sidequest_prs(&self, branch_prefix: &str) -> Result<Vec<SearchItem>> {
        let query = format!("is:pr is:open head:{branch_prefix}");
        let url = format!(
            "https://api.github.com/search/issues?q={}&per_page=100",
            urlencoding::encode(&query)
        );
        let result: SearchResult = self.get_json(&url).await?;
        Ok(result.items)
    }

    /// Get all reviews for a PR
    pub async fn get_reviews(&self, repo: &str, pr_number: u64) -> Result<Vec<Review>> {
        let url = format!(
            "https://api.github.com/repos/{repo}/pulls/{pr_number}/reviews?per_page=100"
        );
        self.get_json(&url).await
    }

    /// Get issue comments (top-level PR comments)
    pub async fn get_pr_comments(&self, repo: &str, pr_number: u64) -> Result<Vec<Comment>> {
        let url = format!(
            "https://api.github.com/repos/{repo}/issues/{pr_number}/comments?per_page=100"
        );
        self.get_json(&url).await
    }

    /// Get review comments (inline code comments)
    pub async fn get_review_comments(&self, repo: &str, pr_number: u64) -> Result<Vec<Comment>> {
        let url = format!(
            "https://api.github.com/repos/{repo}/pulls/{pr_number}/comments?per_page=100"
        );
        self.get_json(&url).await
    }
}
