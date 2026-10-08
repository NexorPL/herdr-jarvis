//! Incremental index over `~/.claude/projects/<dir>/<session>.jsonl`.

use super::Thread;
use crate::pricing::Usage;
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const SNIPPET_CHARS: usize = 300;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct FileState {
    len: u64,
    offset: u64,
    /// Claude Code writes one line per content block, each repeating the message's usage.
    /// ponytail: duplicates are consecutive, so remembering the last id is enough; a full id set if that changes.
    last_message_id: Option<String>,
    #[serde(default)]
    bad_lines: u64,
    thread: Thread,
}

pub struct ClaudeSource {
    projects_dir: PathBuf,
    index_path: PathBuf,
    files: HashMap<String, FileState>,
    dirty: bool,
}

impl ClaudeSource {
    pub fn new(claude_dir: &Path, index_path: PathBuf) -> ClaudeSource {
        let files = std::fs::read_to_string(&index_path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        ClaudeSource {
            projects_dir: claude_dir.join("projects"),
            index_path,
            files,
            dirty: false,
        }
    }

    /// Reads whatever changed since the last call and returns every known thread.
    pub fn refresh(&mut self) -> Vec<Thread> {
        for path in self.transcript_files() {
            if let Err(e) = self.refresh_file(&path) {
                crate::log::log(format!("transcripts: {}: {e}", path.display()));
            }
        }
        if self.dirty {
            self.save();
        }
        self.files
            .values()
            .map(|f| f.thread.clone())
            .filter(|t| t.first_ts.is_some())
            .collect()
    }

    /// ponytail: top-level session files only; subagent transcripts in nested dirs are not counted yet.
    fn transcript_files(&self) -> Vec<PathBuf> {
        let Ok(dirs) = std::fs::read_dir(&self.projects_dir) else {
            return Vec::new();
        };
        dirs.flatten()
            .filter(|d| d.path().is_dir())
            .flat_map(|d| std::fs::read_dir(d.path()).into_iter().flatten().flatten())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
            .collect()
    }

    fn refresh_file(&mut self, path: &Path) -> std::io::Result<()> {
        let len = std::fs::metadata(path)?.len();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let fresh = || FileState {
            thread: Thread {
                session_id: stem.clone(),
                ..Default::default()
            },
            ..Default::default()
        };
        let state = self
            .files
            .entry(path.to_string_lossy().to_string())
            .or_insert_with(&fresh);
        if len == state.len {
            return Ok(());
        }
        if len < state.offset {
            *state = fresh();
        }
        let mut file = std::fs::File::open(path)?;
        file.seek(SeekFrom::Start(state.offset))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        state.len = len;
        self.dirty = true;
        let Some(end) = buf.iter().rposition(|&b| b == b'\n') else {
            return Ok(());
        };
        for line in buf[..end].split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
            match std::str::from_utf8(line) {
                Ok(text) => apply_line(state, text),
                Err(_) => state.bad_lines += 1,
            }
        }
        state.offset += end as u64 + 1;
        Ok(())
    }

    fn save(&mut self) {
        let tmp = self.index_path.with_extension("json.tmp");
        if let Ok(text) = serde_json::to_string(&self.files) {
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, &self.index_path);
            }
        }
        self.dirty = false;
    }
}

/// Local calendar day of a timestamp, `YYYY-MM-DD`.
pub fn local_day(ts: DateTime<Utc>) -> String {
    ts.with_timezone(&Local).format("%Y-%m-%d").to_string()
}

fn apply_line(state: &mut FileState, line: &str) {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        state.bad_lines += 1;
        return;
    };
    let t = &mut state.thread;
    match v["type"].as_str() {
        Some("ai-title") => {
            if let Some(s) = v["aiTitle"].as_str() {
                t.title = Some(s.to_string());
            }
        }
        Some("user") => {
            note_common(t, &v);
            if v["isMeta"].as_bool() == Some(true) || v["isSidechain"].as_bool() == Some(true) {
                return;
            }
            if let Some(text) = prompt_text(&v["message"]["content"]) {
                t.turns += 1;
                t.last_prompt = Some(snippet(&text));
            }
        }
        Some("assistant") => {
            note_common(t, &v);
            let msg = &v["message"];
            if let Some(text) = reply_text(&msg["content"]) {
                t.last_reply = Some(snippet(&text));
            }
            let id = msg["id"].as_str().map(str::to_string);
            if id.is_some() && id == state.last_message_id {
                return;
            }
            state.last_message_id = id;
            let (Some(model), Some(ts)) = (msg["model"].as_str(), timestamp(&v)) else {
                return;
            };
            let usage = usage_of(&msg["usage"]);
            if usage.total() == 0 {
                return;
            }
            *t.usage
                .entry(local_day(ts))
                .or_default()
                .entry(model.to_string())
                .or_default() += usage;
        }
        _ => {}
    }
}

fn timestamp(v: &Value) -> Option<DateTime<Utc>> {
    v["timestamp"].as_str()?.parse().ok()
}

fn note_common(t: &mut Thread, v: &Value) {
    if let Some(ts) = timestamp(v) {
        t.first_ts.get_or_insert(ts);
        t.last_ts = Some(t.last_ts.map_or(ts, |last| last.max(ts)));
    }
    if let Some(cwd) = v["cwd"].as_str() {
        t.cwd = Some(cwd.to_string());
    }
    if let Some(branch) = v["gitBranch"].as_str().filter(|b| !b.is_empty()) {
        t.branch = Some(branch.to_string());
    }
}

/// A real prompt is a string, or text blocks without tool results.
fn prompt_text(content: &Value) -> Option<String> {
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    let blocks = content.as_array()?;
    if blocks.iter().any(|b| b["type"] == "tool_result") {
        return None;
    }
    let text = join_text(blocks);
    (!text.is_empty()).then_some(text)
}

fn reply_text(content: &Value) -> Option<String> {
    let text = join_text(content.as_array()?);
    (!text.is_empty()).then_some(text)
}

fn join_text(blocks: &[Value]) -> String {
    blocks
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn snippet(text: &str) -> String {
    text.chars().take(SNIPPET_CHARS).collect()
}

/// Splits cache writes by TTL when Claude Code reports it; otherwise counts them as 5-minute writes.
fn usage_of(u: &Value) -> Usage {
    let n = |k: &str| u[k].as_u64().unwrap_or(0);
    let split = &u["cache_creation"];
    let (w5m, w1h) = match (
        split["ephemeral_5m_input_tokens"].as_u64(),
        split["ephemeral_1h_input_tokens"].as_u64(),
    ) {
        (Some(a), Some(b)) => (a, b),
        _ => (n("cache_creation_input_tokens"), 0),
    };
    Usage {
        input: n("input_tokens"),
        output: n("output_tokens"),
        cache_read: n("cache_read_input_tokens"),
        cache_write_5m: w5m,
        cache_write_1h: w1h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const FIXTURE: &str = include_str!("../../tests/fixtures/claude-session.jsonl");

    fn ts(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    /// Creates `<tmp>/claude/projects/-home-u-alpha/sess-a.jsonl` with `content`.
    fn setup(content: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("claude");
        let dir = claude.join("projects/-home-u-alpha");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("sess-a.jsonl");
        std::fs::write(&file, content).unwrap();
        (tmp, claude, file)
    }

    fn expect_full(t: &Thread) {
        assert_eq!(t.session_id, "sess-a");
        assert_eq!(t.title.as_deref(), Some("Parser fix"));
        assert_eq!(t.turns, 2);
        assert_eq!(t.cwd.as_deref(), Some("/home/u/alpha"));
        assert_eq!(t.branch.as_deref(), Some("feat-x"));
        assert_eq!(t.last_prompt.as_deref(), Some("Thanks, now add tests"));
        assert_eq!(t.last_reply.as_deref(), Some("Done: parser fixed."));
        assert_eq!(t.first_ts, Some(ts("2026-10-08T10:00:00Z")));
        assert_eq!(t.last_ts, Some(ts("2026-10-08T10:02:00Z")));
        let day = &t.usage[&local_day(ts("2026-10-08T10:00:05Z"))];
        assert_eq!(
            day["claude-opus-5-5"],
            Usage {
                input: 10,
                output: 100,
                cache_read: 1000,
                cache_write_5m: 0,
                cache_write_1h: 2000
            }
        );
        assert_eq!(
            day["claude-sonnet-5"],
            Usage {
                input: 5,
                output: 50,
                cache_read: 0,
                cache_write_5m: 300,
                cache_write_1h: 0
            }
        );
    }

    #[test]
    fn parses_a_session_and_dedupes_usage() {
        let (tmp, claude, _) = setup(FIXTURE);
        let mut src = ClaudeSource::new(&claude, tmp.path().join("index.json"));
        let threads = src.refresh();
        assert_eq!(threads.len(), 1);
        expect_full(&threads[0]);
    }

    #[test]
    fn incremental_reads_match_full_read() {
        let lines: Vec<&str> = FIXTURE.lines().collect();
        let first = lines[..5].join("\n") + "\n";
        let (tmp, claude, file) = setup(&first);
        let mut src = ClaudeSource::new(&claude, tmp.path().join("index.json"));
        assert_eq!(src.refresh()[0].turns, 1);
        // half-written line: not counted yet
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .unwrap();
        write!(f, "{}", &lines[5][..20]).unwrap();
        src.refresh();
        // finish it and append the rest
        writeln!(f, "{}", &lines[5][20..]).unwrap();
        for l in &lines[6..] {
            writeln!(f, "{l}").unwrap();
        }
        expect_full(&src.refresh()[0]);
    }

    #[test]
    fn shrunk_file_is_reread() {
        let (tmp, claude, file) = setup(FIXTURE);
        let mut src = ClaudeSource::new(&claude, tmp.path().join("index.json"));
        src.refresh();
        let first_two: String = FIXTURE.lines().take(3).map(|l| format!("{l}\n")).collect();
        std::fs::write(&file, first_two).unwrap();
        let t = &src.refresh()[0];
        assert_eq!(t.turns, 1);
        assert!(t.usage.is_empty());
    }

    #[test]
    fn index_survives_restart() {
        let (tmp, claude, _) = setup(FIXTURE);
        let index = tmp.path().join("index.json");
        ClaudeSource::new(&claude, index.clone()).refresh();
        let mut again = ClaudeSource::new(&claude, index);
        assert!(again.files.values().all(|f| f.offset > 0));
        expect_full(&again.refresh()[0]);
    }

    #[test]
    fn counts_corrupt_lines() {
        let (tmp, claude, _) = setup(FIXTURE);
        let mut src = ClaudeSource::new(&claude, tmp.path().join("index.json"));
        src.refresh();
        assert_eq!(src.files.values().map(|f| f.bad_lines).sum::<u64>(), 1);
    }

    #[test]
    fn missing_claude_dir_gives_no_threads() {
        let tmp = tempfile::tempdir().unwrap();
        let mut src = ClaudeSource::new(&tmp.path().join("nope"), tmp.path().join("index.json"));
        assert!(src.refresh().is_empty());
    }
}
