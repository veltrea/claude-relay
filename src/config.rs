use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
    #[serde(default = "default_archive_dir")]
    pub archive_dir: String,
}

fn default_retention_days() -> u32 {
    30
}

fn default_archive_dir() -> String {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".claude-relay")
        .join("archive")
        .to_string_lossy()
        .to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            retention_days: default_retention_days(),
            archive_dir: default_archive_dir(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let config: Config = serde_json::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn config_path() -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".claude-relay").join("config.json")
    }

    pub fn db_path() -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let dir = home.join(".claude-relay");
        std::fs::create_dir_all(&dir).ok();
        dir.join("memory.db")
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "retention_days" => {
                self.retention_days = value.parse()?;
            }
            "archive_dir" => {
                self.archive_dir = value.to_string();
            }
            _ => {
                anyhow::bail!("Unknown config key: {key}");
            }
        }
        self.save()?;
        Ok(())
    }

    pub fn show(&self) -> String {
        format!(
            "Config file: {}\n\
             retention_days: {}\n\
             archive_dir:    {}",
            Self::config_path().display(),
            self.retention_days,
            self.archive_dir
        )
    }
}

/// DB パスを expand して返す
pub fn resolve_archive_dir(config: &Config) -> PathBuf {
    let expanded = shellexpand::tilde(&config.archive_dir);
    PathBuf::from(expanded.as_ref())
}
