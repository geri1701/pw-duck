use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::identity::ConfiguredSource;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_duck_percent")]
    pub duck_percent: u8,
    #[serde(default = "default_vad_threshold")]
    pub vad_threshold: f32,
    #[serde(default = "default_hold_ms")]
    pub hold_ms: u64,
    #[serde(default)]
    pub voice_source: Option<ConfiguredSource>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            duck_percent: default_duck_percent(),
            vad_threshold: default_vad_threshold(),
            hold_ms: default_hold_ms(),
            voice_source: None,
        }
    }
}

impl Config {
    pub fn path() -> Result<PathBuf> {
        let base = dirs::config_dir().context("XDG config directory not found")?;
        Ok(base.join("pw-duck").join("config.toml"))
    }

    pub fn load_or_default() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        toml::from_str(&raw).with_context(|| format!("invalid TOML in {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let raw = toml::to_string_pretty(self).context("failed to encode config TOML")?;
        fs::write(&path, raw).with_context(|| format!("failed to write {}", path.display()))
    }
}

fn default_duck_percent() -> u8 {
    25
}

fn default_vad_threshold() -> f32 {
    0.01
}

fn default_hold_ms() -> u64 {
    700
}
