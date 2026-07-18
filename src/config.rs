//! Config: one small TOML file, no bang/alias tables by design.
//!
//! `~/.config/bang/config.toml`:
//! ```toml
//! engine = "duckduckgo"          # or "searxng"
//! searxng_url = "https://searx.example.org"
//! ```
//! Env overrides: `BANG_ENGINE`, `BANG_SEARXNG_URL`.

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(default)]
pub struct Config {
    pub engine: String,
    pub searxng_url: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            engine: "duckduckgo".into(),
            searxng_url: None,
        }
    }
}

pub fn load() -> Config {
    let mut config: Config = dirs::config_dir()
        .map(|d| d.join("bang/config.toml"))
        .filter(|p| p.exists())
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default();

    if let Ok(engine) = std::env::var("BANG_ENGINE") {
        if !engine.trim().is_empty() {
            config.engine = engine.trim().to_string();
        }
    }
    if let Ok(url) = std::env::var("BANG_SEARXNG_URL") {
        if !url.trim().is_empty() {
            config.searxng_url = Some(url.trim().to_string());
        }
    }
    config
}
