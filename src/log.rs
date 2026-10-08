use std::io::Write;
use std::path::Path;

const MAX_BYTES: u64 = 1_000_000;

/// Appends one timestamped line to `STATE_DIR/jarvis.log`. Never fails; logging must not break the UI.
pub fn log(msg: impl std::fmt::Display) {
    let dir = crate::paths::state_dir();
    let _ = std::fs::create_dir_all(&dir);
    append(&dir.join("jarvis.log"), &msg.to_string());
}

fn append(path: &Path, msg: &str) {
    if std::fs::metadata(path)
        .map(|m| m.len() > MAX_BYTES)
        .unwrap_or(false)
    {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(
            f,
            "{} {msg}",
            chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jarvis.log");
        append(&path, "one");
        append(&path, "two");
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.lines().nth(1).unwrap().ends_with(" two"));
    }

    #[test]
    fn rotates_past_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jarvis.log");
        std::fs::write(&path, vec![b'x'; (MAX_BYTES + 1) as usize]).unwrap();
        append(&path, "fresh");
        assert!(dir.path().join("jarvis.log.1").exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 1);
    }
}
