use crate::pricing::Price;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Animation {
    #[default]
    Full,
    Reduced,
    Off,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub animation: Animation,
    pub retention_days: u32,
    pub max_branches: usize,
    pub claude_dir: Option<PathBuf>,
    pub pricing: HashMap<String, Price>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            animation: Animation::Full,
            retention_days: 30,
            max_branches: 8,
            claude_dir: None,
            pricing: HashMap::new(),
        }
    }
}

impl Config {
    /// Reads `<dir>/config.toml`. A missing file gives defaults; an invalid one is logged and gives defaults.
    pub fn load(dir: &Path) -> Config {
        let path = dir.join("config.toml");
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Config::default();
        };
        Config::parse(&text).unwrap_or_else(|e| {
            crate::log::log(format!("invalid {}: {e}", path.display()));
            Config::default()
        })
    }

    pub fn parse(text: &str) -> Result<Config, toml::de::Error> {
        toml::from_str(text)
    }

    /// `claude_dir` from config, else `CLAUDE_CONFIG_DIR`, else `~/.claude`.
    pub fn claude_dir(&self) -> Option<PathBuf> {
        self.claude_dir
            .as_deref()
            .map(crate::paths::expand_tilde)
            .or_else(|| {
                std::env::var_os("CLAUDE_CONFIG_DIR")
                    .filter(|v| !v.is_empty())
                    .map(PathBuf::from)
            })
            .or_else(|| crate::paths::home_dir().map(|h| h.join(".claude")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults() {
        let c = Config::parse("").unwrap();
        assert_eq!(c.animation, Animation::Full);
        assert_eq!(c.retention_days, 30);
        assert_eq!(c.max_branches, 8);
        assert!(c.pricing.is_empty());
    }

    #[test]
    fn parses_overrides() {
        let c = Config::parse(
            r#"
animation = "off"
max_branches = 5
claude_dir = "~/alt-claude"

[pricing."claude-opus-5-5"]
input = 1.0
output = 2.0
cache_read = 0.1
cache_write_5m = 1.25
cache_write_1h = 2.0
"#,
        )
        .unwrap();
        assert_eq!(c.animation, Animation::Off);
        assert_eq!(c.max_branches, 5);
        assert_eq!(c.pricing["claude-opus-5-5"].output, 2.0);
        assert!(c.claude_dir().unwrap().ends_with("alt-claude"));
    }

    #[test]
    fn rejects_unknown_animation() {
        assert!(Config::parse(r#"animation = "wild""#).is_err());
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Config::load(dir.path()).max_branches, 8);
    }
}
