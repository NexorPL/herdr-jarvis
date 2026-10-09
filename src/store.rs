//! Lists stored as one JSON array in the plugin state directory (ideas, run targets).

use serde::{de::DeserializeOwned, Serialize};
use std::path::Path;

/// A missing file is an empty list. An unreadable one is copied to `<file>.bad`, so the next save cannot lose it.
pub fn load<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(back_up(path, e)),
    };
    let text = std::str::from_utf8(&bytes).map_err(|e| back_up(path, e))?;
    // Notepad's "UTF-8 with BOM".
    serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| back_up(path, e))
}

fn back_up(path: &Path, e: impl std::fmt::Display) -> String {
    let bad = path.with_extension("json.bad");
    match std::fs::copy(path, &bad) {
        Ok(_) => format!(
            "{} unreadable ({e}); kept a copy in {}",
            path.display(),
            bad.display()
        ),
        Err(c) => format!(
            "{} unreadable ({e}); could not back it up ({c})",
            path.display()
        ),
    }
}

/// Rewrites the whole file through a temporary one, so a crash never leaves half a file.
pub fn save<T: Serialize>(path: &Path, items: &[T]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(items)?)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ideas::Idea;
    use crate::run::Target;
    use std::fs;

    fn idea(name: &str) -> Idea {
        Idea {
            project_key: "/home/u/alpha".into(),
            project_name: "alpha".into(),
            name: name.into(),
            description: format!("about {name}"),
            status: Default::default(),
        }
    }

    #[test]
    fn missing_file_is_an_empty_list() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            load::<Idea>(&tmp.path().join("ideas.json")).unwrap(),
            vec![]
        );
    }

    #[test]
    fn save_then_load_round_trips_without_leftovers() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ideas.json");
        save(&path, &[idea("a"), idea("Zażółć")]).unwrap();
        assert_eq!(
            load::<Idea>(&path).unwrap(),
            vec![idea("a"), idea("Zażółć")]
        );
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn ideas_saved_before_statuses_load_as_todo() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ideas.json");
        fs::write(
            &path,
            r#"[{"project_key":"k","project_name":"p","name":"n","description":""}]"#,
        )
        .unwrap();
        assert_eq!(
            load::<Idea>(&path).unwrap()[0].status,
            crate::ideas::Status::Todo
        );
    }

    #[test]
    fn file_that_is_not_utf8_is_backed_up_too() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("targets.json");
        fs::write(&path, b"[\xff\xfe]").unwrap();
        let err = load::<Target>(&path).unwrap_err();
        assert!(err.contains("targets.json.bad"), "{err}");
        assert_eq!(
            fs::read(path.with_extension("json.bad")).unwrap(),
            b"[\xff\xfe]"
        );
    }

    #[test]
    fn utf8_bom_is_accepted() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ideas.json");
        fs::write(&path, "\u{feff}[]").unwrap();
        assert_eq!(load::<Idea>(&path).unwrap(), vec![]);
    }

    #[test]
    fn unreadable_file_is_backed_up_and_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ideas.json");
        fs::write(&path, "{not json").unwrap();
        let err = load::<Idea>(&path).unwrap_err();
        assert!(err.contains("ideas.json.bad"), "{err}");
        assert_eq!(
            fs::read_to_string(path.with_extension("json.bad")).unwrap(),
            "{not json"
        );
    }

    #[test]
    fn targets_round_trip_and_env_is_optional() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("targets.json");
        fs::write(
            &path,
            r#"[{"project_key": "/home/u/alpha", "name": "web", "command": "pnpm dev", "cwd": ""}]"#,
        )
        .unwrap();
        let mut targets = load::<Target>(&path).unwrap();
        assert_eq!(targets[0].name, "web");
        assert!(targets[0].env.is_empty());
        targets[0].env.insert("PORT".into(), "3001".into());
        save(&path, &targets).unwrap();
        assert_eq!(load::<Target>(&path).unwrap(), targets);
    }
}
