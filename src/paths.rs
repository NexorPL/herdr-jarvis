use std::path::{Path, PathBuf};

/// Runtime state (events, transcript index, lock, log): `HERDR_PLUGIN_STATE_DIR`, else `~/.jarvis/state`.
pub fn state_dir() -> PathBuf {
    env_path("HERDR_PLUGIN_STATE_DIR").unwrap_or_else(|| fallback("state"))
}

/// User config (`config.toml`): `HERDR_PLUGIN_CONFIG_DIR`, else `~/.jarvis/config`.
pub fn config_dir() -> PathBuf {
    env_path("HERDR_PLUGIN_CONFIG_DIR").unwrap_or_else(|| fallback("config"))
}

pub fn home_dir() -> Option<PathBuf> {
    env_path("HOME").or_else(|| env_path("USERPROFILE"))
}

/// Replaces a leading `~` with the home directory.
pub fn expand_tilde(path: &Path) -> PathBuf {
    match (path.strip_prefix("~"), home_dir()) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => path.to_path_buf(),
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn fallback(kind: &str) -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".jarvis")
        .join(kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn expand_tilde_leaves_plain_paths() {
        assert_eq!(expand_tilde(Path::new("/opt/x")), Path::new("/opt/x"));
    }

    #[test]
    fn expand_tilde_uses_home() {
        let home = home_dir().expect("HOME or USERPROFILE set in tests");
        assert_eq!(expand_tilde(Path::new("~/a/b")), home.join("a/b"));
    }
}
