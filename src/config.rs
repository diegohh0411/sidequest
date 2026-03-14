use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub docker: DockerConfig,
    pub github: GithubConfig,
    pub claude: ClaudeConfig,
    pub watch: Option<WatchConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DockerConfig {
    pub image_name: String,
    pub container_prefix: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GithubConfig {
    pub branch_prefix: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaudeConfig {
    pub model: String,
    pub fast_model: String,
    pub max_turns: i64,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct WatchConfig {
    pub poll_interval: Option<u64>,
    pub bot_mention: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            docker: DockerConfig {
                image_name: "sidequest-agent".to_string(),
                container_prefix: "sq-task-".to_string(),
            },
            github: GithubConfig {
                branch_prefix: "sidequest/".to_string(),
            },
            claude: ClaudeConfig {
                model: "sonnet".to_string(),
                fast_model: "haiku".to_string(),
                max_turns: -1,
            },
            watch: None,
        }
    }
}

/// Load config from config.toml in current dir or ~/.config/sidequest/config.toml.
/// Falls back to defaults if neither file exists.
pub fn load_config() -> Result<AppConfig> {
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
