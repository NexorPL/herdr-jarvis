//! Per-project ideas, stored as one JSON array in the plugin state directory.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// An idea belongs to a project, not to an agent: agents come and go, ideas stay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Idea {
    pub project_key: String,
    /// Kept so ideas of projects without live agents still show a readable name.
    pub project_name: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// A missing file is no ideas. An unreadable one is copied to `ideas.json.bad`, so the next save cannot lose it.
pub fn load(path: &Path) -> Result<Vec<Idea>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("could not read {}: {e}", path.display())),
    };
    serde_json::from_str(&text).map_err(|e| {
        let bad = path.with_extension("json.bad");
        let _ = std::fs::copy(path, &bad);
        format!("ideas file unreadable ({e}); kept a copy in {}", bad.display())
    })
}

/// Rewrites the whole file through a temporary one, so a crash never leaves half a file.
pub fn save(path: &Path, ideas: &[Idea]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(ideas)?)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn idea(name: &str) -> Idea {
        Idea {
            project_key: "/home/u/alpha".into(),
            project_name: "alpha".into(),
            name: name.into(),
            description: format!("about {name}"),
        }
    }

    #[test]
    fn missing_file_is_no_ideas() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load(&tmp.path().join("ideas.json")).unwrap(), vec![]);
    }

    #[test]
    fn save_then_load_round_trips_without_leftovers() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ideas.json");
        save(&path, &[idea("a"), idea("Zażółć")]).unwrap();
        assert_eq!(load(&path).unwrap(), vec![idea("a"), idea("Zażółć")]);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn unreadable_file_is_backed_up_and_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ideas.json");
        fs::write(&path, "{not json").unwrap();
        let err = load(&path).unwrap_err();
        assert!(err.contains("ideas.json.bad"), "{err}");
        assert_eq!(
            fs::read_to_string(path.with_extension("json.bad")).unwrap(),
            "{not json"
        );
    }
}
