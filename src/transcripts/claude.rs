//! Claude Code: `~/.claude/projects/<dir>/<session>.jsonl` plus `<session>/subagents/agent-<id>.jsonl`.

use super::{note_prompt, note_ts, timestamp, FileState, Thread};
use crate::pricing::Usage;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) fn files(claude_dir: &Path) -> Vec<PathBuf> {
    let jsonl = |p: &PathBuf| p.extension().is_some_and(|x| x == "jsonl");
    let mut files = Vec::new();
    for project in super::list(&claude_dir.join("projects"))
        .into_iter()
        .filter(|p| p.is_dir())
    {
        for entry in super::list(&project) {
            if entry.is_dir() {
                files.extend(
                    super::list(&entry.join("subagents"))
                        .into_iter()
                        .filter(jsonl),
                );
            } else if jsonl(&entry) {
                files.push(entry);
            }
        }
    }
    files
}

/// A subagent file belongs to the session named by its grandparent directory.
pub(super) fn fresh(path: &Path) -> FileState {
    let stem = |p: Option<&Path>| {
        p.and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    };
    let (id, parent) = if is_subagent(path) {
        let id = stem(Some(path)).trim_start_matches("agent-").to_string();
        (id, Some(stem(path.parent().and_then(Path::parent))))
    } else {
        (stem(Some(path)), None)
    };
    FileState {
        parent,
        thread: Thread {
            session_id: id,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Subagent description and type from `agent-<id>.meta.json`, which may appear after the transcript.
pub(super) fn read_meta_once(path: &Path, state: &mut FileState) {
    if state.meta.is_some() || !is_subagent(path) {
        return;
    }
    state.meta = read_meta(&path.with_extension("meta.json"));
}

fn read_meta(path: &Path) -> Option<(String, String)> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
    Some((s("description"), s("agentType")))
}

fn is_subagent(path: &Path) -> bool {
    path.parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n == "subagents")
}

pub(super) fn apply(state: &mut FileState, v: &Value) {
    let t = &mut state.thread;
    match v["type"].as_str() {
        Some("ai-title") => {
            if let Some(s) = v["aiTitle"].as_str() {
                t.title = Some(s.to_string());
            }
        }
        Some("user") => {
            note_common(t, v);
            // A subagent's handback tool (`SubagentHandback`) ends its turn; any other user line continues it.
            state.ended = v["toolEndsTurn"].as_bool() == Some(true);
            if state.ended {
                state.activity.clear();
            }
            if v["isMeta"].as_bool() == Some(true) || v["isSidechain"].as_bool() == Some(true) {
                return;
            }
            if let Some(text) = prompt_text(&v["message"]["content"]) {
                note_prompt(t, &text);
            }
        }
        Some("assistant") => {
            note_common(t, v);
            let msg = &v["message"];
            // `tool_use` and `pause_turn` continue the turn; any other stop reason ends it.
            state.ended = (msg["stop_reason"].as_str())
                .is_some_and(|r| !matches!(r, "tool_use" | "pause_turn"));
            if state.ended {
                state.activity.clear();
            } else if state.parent.is_some() {
                let blocks = msg["content"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                state.activity.extend(blocks.iter().filter_map(activity_of));
                let over = state.activity.len().saturating_sub(ACTIVITY_LEN);
                state.activity.drain(..over);
            }
            if let Some(text) = msg["content"].as_array().map(|b| super::join_text(b)) {
                if !text.is_empty() {
                    t.last_reply = Some(super::snippet(&text));
                }
            }
            let id = msg["id"].as_str().map(str::to_string);
            // Copies of one message repeat its usage, and later copies carry the final output count:
            // replace the previous copy's contribution instead of adding to it.
            if id.is_some() && id == state.last_message_id {
                if let Some((day, model, prev)) = state.last_usage.take() {
                    if let Some(u) = t.usage.get_mut(&day).and_then(|m| m.get_mut(&model)) {
                        *u -= prev;
                    }
                }
            }
            state.last_message_id = id;
            let (Some(model), Some(ts)) = (msg["model"].as_str(), timestamp(v)) else {
                return;
            };
            let usage = usage_of(&msg["usage"]);
            if usage.total() == 0 {
                return;
            }
            super::add_usage(t, ts, model, usage);
            state.last_usage = Some((super::local_day(ts), model.to_string(), usage));
        }
        _ => {}
    }
}

/// Entries a running subagent keeps in its log.
const ACTIVITY_LEN: usize = 30;

/// A log entry for a content block: `Tool  what it works on`, or `› ` and a reply's first line.
fn activity_of(block: &Value) -> Option<String> {
    let first_line = |s: &str| {
        super::snippet(
            s.lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or(""),
        )
    };
    match block["type"].as_str()? {
        "tool_use" => {
            let name = block["name"].as_str()?;
            let input = &block["input"];
            let keys = [
                "description",
                "file_path",
                "pattern",
                "command",
                "path",
                "query",
                "url",
                "skill",
            ];
            Some(match keys.iter().find_map(|k| input[*k].as_str()) {
                Some(what) => format!("{name}  {}", first_line(what)),
                None => name.to_string(),
            })
        }
        "text" => {
            let text = first_line(block["text"].as_str()?);
            (!text.is_empty()).then(|| format!("› {text}"))
        }
        _ => None,
    }
}

fn note_common(t: &mut Thread, v: &Value) {
    note_ts(t, timestamp(v));
    if let Some(cwd) = v["cwd"].as_str() {
        t.cwd = Some(cwd.to_string());
    }
    if let Some(branch) = v["gitBranch"].as_str().filter(|b| !b.is_empty()) {
        t.branch = Some(branch.to_string());
    }
}

/// A real prompt is a string, or text blocks without tool results.
fn prompt_text(content: &Value) -> Option<String> {
    if content
        .as_array()
        .is_some_and(|b| b.iter().any(|b| b["type"] == "tool_result"))
    {
        return None;
    }
    super::text_of(content)
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
    use crate::transcripts::{local_day, Kind, Source};
    use chrono::{DateTime, Utc};
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
        let mut src = Source::new(Kind::Claude, &claude, tmp.path().join("index.json"));
        let threads = src.refresh();
        assert_eq!(threads.len(), 1);
        expect_full(&threads[0]);
    }

    #[test]
    fn incremental_reads_match_full_read() {
        let lines: Vec<&str> = FIXTURE.lines().collect();
        let first = lines[..5].join("\n") + "\n";
        let (tmp, claude, file) = setup(&first);
        let mut src = Source::new(Kind::Claude, &claude, tmp.path().join("index.json"));
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
        let mut src = Source::new(Kind::Claude, &claude, tmp.path().join("index.json"));
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
        Source::new(Kind::Claude, &claude, index.clone()).refresh();
        let mut again = Source::new(Kind::Claude, &claude, index);
        assert!(again.files.values().all(|f| f.offset > 0));
        expect_full(&again.refresh()[0]);
    }

    #[test]
    fn counts_corrupt_lines() {
        let (tmp, claude, _) = setup(FIXTURE);
        let mut src = Source::new(Kind::Claude, &claude, tmp.path().join("index.json"));
        src.refresh();
        assert_eq!(src.files.values().map(|f| f.bad_lines).sum::<u64>(), 1);
    }

    #[test]
    fn subagent_usage_merges_into_parent_session() {
        let (tmp, claude, _) = setup(FIXTURE);
        let sub = claude.join("projects/-home-u-alpha/sess-a/subagents");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            sub.join("agent-1.jsonl"),
            concat!(
                r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"Explore"},"timestamp":"2026-10-08T10:03:00.000Z","cwd":"/home/u/alpha","sessionId":"sess-a"}"#,
                "\n",
                r#"{"type":"assistant","isSidechain":true,"message":{"id":"msg_s1","model":"claude-sonnet-5","content":[{"type":"text","text":"found it"}],"usage":{"input_tokens":7,"output_tokens":70}},"timestamp":"2026-10-08T10:03:30.000Z","cwd":"/home/u/alpha","sessionId":"sess-a"}"#,
                "\n"
            ),
        )
        .unwrap();
        let mut src = Source::new(Kind::Claude, &claude, tmp.path().join("index.json"));
        let threads = src.refresh();
        assert_eq!(threads.len(), 1);
        let t = &threads[0];
        assert_eq!(t.turns, 2);
        assert_eq!(t.last_reply.as_deref(), Some("Done: parser fixed."));
        let day = &t.usage[&local_day(ts("2026-10-08T10:00:05Z"))];
        assert_eq!(day["claude-sonnet-5"].input, 5 + 7);
        assert_eq!(day["claude-sonnet-5"].output, 50 + 70);
        // No meta file and no end_turn: listed, still running.
        let s = &t.subagents[0];
        assert_eq!(
            (s.id.as_str(), s.description.as_str(), s.ended),
            ("1", "", false)
        );

        std::fs::write(
            sub.join("agent-1.meta.json"),
            r#"{"agentType":"Explore","description":"Find the parser"}"#,
        )
        .unwrap();
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(sub.join("agent-1.jsonl"))
            .unwrap();
        writeln!(f, r#"{{"type":"assistant","isSidechain":true,"message":{{"id":"msg_s2","stop_reason":"end_turn","content":[{{"type":"text","text":"done"}}]}},"timestamp":"2026-10-08T10:04:00.000Z"}}"#).unwrap();
        let t = &src.refresh()[0];
        let s = &t.subagents[0];
        assert_eq!(
            (s.description.as_str(), s.agent_type.as_str()),
            ("Find the parser", "Explore")
        );
        assert!(s.ended);
        assert_eq!(s.last_ts, Some(ts("2026-10-08T10:04:00Z")));
    }

    /// A subagent of `sess-a` that hands its result back with `SubagentHandback`.
    fn with_handback() -> (tempfile::TempDir, PathBuf) {
        let (tmp, claude, _) = setup(FIXTURE);
        let sub = claude.join("projects/-home-u-alpha/sess-a/subagents");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            sub.join("agent-1.jsonl"),
            concat!(
                r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"Explore"},"timestamp":"2026-10-08T10:03:00.000Z"}"#,
                "\n",
                r#"{"type":"assistant","isSidechain":true,"message":{"id":"msg_s1","stop_reason":"tool_use","content":[{"type":"tool_use","id":"toolu_1","name":"SubagentHandback","input":{}}]},"timestamp":"2026-10-08T10:03:30.000Z"}"#,
                "\n",
                r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":[{"tool_use_id":"toolu_1","type":"tool_result","content":"ok"}]},"toolEndsTurn":true,"timestamp":"2026-10-08T10:03:31.000Z"}"#,
                "\n"
            ),
        )
        .unwrap();
        (tmp, claude)
    }

    #[test]
    fn subagent_ends_with_a_handback_tool() {
        let (tmp, claude) = with_handback();
        let mut src = Source::new(Kind::Claude, &claude, tmp.path().join("index.json"));
        assert!(src.refresh()[0].subagents[0].ended);
    }

    #[test]
    fn unfinished_subagents_are_read_again_on_start() {
        let (tmp, claude) = with_handback();
        let index = tmp.path().join("index.json");
        let mut src = Source::new(Kind::Claude, &claude, index.clone());
        src.refresh();
        // As an older Jarvis left it: read to the end without seeing the end.
        for f in src.files.values_mut() {
            f.ended = false;
        }
        src.save();
        let mut again = Source::new(Kind::Claude, &claude, index);
        assert!(again.refresh()[0].subagents[0].ended);
    }

    #[test]
    fn log_entries_skip_leading_blank_lines() {
        let text = serde_json::json!({"type": "text", "text": "\n\n  Found it\nmore"});
        assert_eq!(activity_of(&text).as_deref(), Some("› Found it"));
        let bash = serde_json::json!({"type": "tool_use", "name": "Bash", "input": {"command": "\ncargo test"}});
        assert_eq!(activity_of(&bash).as_deref(), Some("Bash  cargo test"));
        let blank = serde_json::json!({"type": "text", "text": " \n "});
        assert_eq!(activity_of(&blank), None);
    }

    #[test]
    fn subagent_logs_its_tool_calls_and_replies_until_it_ends() {
        let (tmp, claude, _) = setup(FIXTURE);
        let sub = claude.join("projects/-home-u-alpha/sess-a/subagents");
        std::fs::create_dir_all(&sub).unwrap();
        let tool = |name: &str, input: &str| {
            format!(
                r#"{{"type":"assistant","isSidechain":true,"message":{{"id":"m-{name}","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t","name":"{name}","input":{input}}}]}},"timestamp":"2026-10-08T10:03:30.000Z"}}"#
            )
        };
        let mut lines = vec![
            r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"Explore"},"timestamp":"2026-10-08T10:03:00.000Z"}"#.to_string(),
            tool("Bash", r#"{"command":"cargo test","description":"Run the tests"}"#),
            tool("Grep", r#"{"pattern":"ChangeScene","path":"src"}"#),
            r#"{"type":"assistant","isSidechain":true,"message":{"id":"m-x","content":[{"type":"thinking","thinking":"hmm"}]},"timestamp":"2026-10-08T10:03:31.000Z"}"#.to_string(),
            r#"{"type":"assistant","isSidechain":true,"message":{"id":"m-y","content":[{"type":"text","text":"Found it\nin scene.rs"}]},"timestamp":"2026-10-08T10:03:32.000Z"}"#.to_string(),
        ];
        for n in 0..40 {
            lines.push(tool("Read", &format!(r#"{{"file_path":"src/f{n}.rs"}}"#)));
        }
        std::fs::write(sub.join("agent-1.jsonl"), lines.join("\n") + "\n").unwrap();
        let mut src = Source::new(Kind::Claude, &claude, tmp.path().join("index.json"));
        let activity = src.refresh()[0].subagents[0].activity.clone();
        assert_eq!(activity.len(), 30);
        assert_eq!(activity.last().unwrap(), "Read  src/f39.rs");

        let (tmp, claude, _) = setup(FIXTURE);
        let sub = claude.join("projects/-home-u-alpha/sess-a/subagents");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("agent-1.jsonl"), lines[..5].join("\n") + "\n").unwrap();
        let mut src = Source::new(Kind::Claude, &claude, tmp.path().join("index.json"));
        assert_eq!(
            src.refresh()[0].subagents[0].activity,
            ["Bash  Run the tests", "Grep  ChangeScene", "› Found it"]
        );
        // The parent session keeps no log, and an ended subagent drops its own.
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(sub.join("agent-1.jsonl"))
            .unwrap();
        writeln!(f, r#"{{"type":"assistant","isSidechain":true,"message":{{"id":"m-z","stop_reason":"end_turn","content":[{{"type":"text","text":"done"}}]}},"timestamp":"2026-10-08T10:04:00.000Z"}}"#).unwrap();
        let t = &src.refresh()[0];
        assert!(t.subagents[0].activity.is_empty());
        assert!(src.files.values().all(|f| f.activity.is_empty()));
    }

    #[test]
    fn streamed_copies_keep_the_final_usage() {
        let line = |out: u32| {
            format!(
                r#"{{"type":"assistant","message":{{"id":"msg_9","model":"claude-sonnet-5","content":[],"usage":{{"input_tokens":5,"output_tokens":{out}}}}},"timestamp":"2026-10-08T10:00:05.000Z","cwd":"/home/u/alpha"}}"#
            )
        };
        let (tmp, claude, file) = setup(&format!("{}\n", line(100)));
        let mut src = Source::new(Kind::Claude, &claude, tmp.path().join("index.json"));
        src.refresh();
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .unwrap();
        writeln!(f, "{}", line(300)).unwrap();
        let t = &src.refresh()[0];
        let u = t.usage[&local_day(ts("2026-10-08T10:00:05Z"))]["claude-sonnet-5"];
        assert_eq!((u.input, u.output), (5, 300));
    }

    #[test]
    fn missing_claude_dir_gives_no_threads() {
        let tmp = tempfile::tempdir().unwrap();
        let mut src = Source::new(
            Kind::Claude,
            &tmp.path().join("nope"),
            tmp.path().join("index.json"),
        );
        assert!(src.refresh().is_empty());
    }
}
