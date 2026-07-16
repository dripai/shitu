use std::{collections::HashMap, fs, path::PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub hotkey: Option<String>,
    #[serde(default)]
    pub pinned_ids: Vec<String>,
    #[serde(default)]
    pub custom_groups: HashMap<String, String>,
}

impl Config {
    pub fn load() -> Self {
        let path = Self::path();
        let Ok(bytes) = fs::read(&path) else {
            return Self::default();
        };
        serde_json::from_slice(&bytes).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        let json = serde_json::to_vec_pretty(self)?;
        fs::write(&path, json).with_context(|| format!("write {}", path.display()))
    }

    pub fn path() -> PathBuf {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("GridStart")
            .join("config.json")
    }
}
