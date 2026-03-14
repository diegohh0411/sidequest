use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::credentials::sidequest_dir;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct WatchState {
    pub processed: HashMap<String, PrState>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct PrState {
    pub last_review_id: u64,
    pub last_comment_id: u64,
}

impl WatchState {
    pub fn load() -> Result<Self> {
        let path = sidequest_dir()?.join("watch_state.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read watch state: {}", path.display()))?;
        toml::from_str(&contents)
            .with_context(|| format!("Failed to parse watch state: {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let dir = sidequest_dir()?;
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create directory: {}", dir.display()))?;
        let path = dir.join("watch_state.toml");
        let contents = toml::to_string(self).context("Failed to serialize watch state")?;
        std::fs::write(&path, &contents)
            .with_context(|| format!("Failed to write watch state: {}", path.display()))?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> PrState {
        self.processed.get(key).cloned().unwrap_or_default()
    }

    pub fn set(&mut self, key: String, state: PrState) {
        self.processed.insert(key, state);
    }
}
