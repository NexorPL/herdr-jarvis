pub mod claude;
pub mod codex;
pub mod copilot;
pub mod pi;

use crate::pricing::Usage;
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const SNIPPET_CHARS: usize = 300;
const TITLE_CHARS: usize = 80;

/// One agent conversation (a Claude Code, Codex, Copilot CLI or Pi session).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Thread {
    pub session_id: String,
    /// herdr's agent name: `claude`, `codex`, `copilot` or `pi`.
    #[serde(default)]
    pub agent: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub branch: Option<String>,
    /// Prompts typed by the user (tool results and meta messages excluded).
    pub turns: u32,
    pub first_ts: Option<DateTime<Utc>>,
    pub last_ts: Option<DateTime<Utc>>,
    pub last_prompt: Option<String>,
    pub last_reply: Option<String>,
    /// Local day (`YYYY-MM-DD`) -> model id -> usage.
    pub usage: BTreeMap<String, BTreeMap<String, Usage>>,
    /// Subagents the session spawned; they run inside its pane.
    #[serde(default)]
    pub subagents: Vec<SubAgent>,
}

impl Thread {
    /// The command that reopens this session in its agent.
    pub fn resume_command(&self) -> String {
        let id = &self.session_id;
        match self.agent.as_str() {
            "codex" => format!("codex resume {id}"),
            "copilot" => format!("copilot --resume={id}"),
            "pi" => format!("pi --session {id}"),
            _ => format!("claude --resume {id}"),
        }
    }
}

/// One subagent: a Claude Code `subagents/agent-<id>.jsonl`, a Codex spawned thread or a Copilot
/// `subagent.started` event.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SubAgent {
    pub id: String,
    pub description: String,
    pub agent_type: String,
    pub last_ts: Option<DateTime<Utc>>,
    /// It finished its task.
    pub ended: bool,
    /// Its latest tool calls and replies, oldest first; kept only while it runs (Claude Code).
    #[serde(default)]
    pub activity: Vec<String>,
}

impl SubAgent {
    /// A subagent that never wrote its end runs while its parent agent is busy (a long tool call or
    /// a permission prompt can keep it silent), or, for background subagents of an idle parent,
    /// while it wrote something in the last 15 minutes.
    /// ponytail: a silent background subagent drops off after 15 minutes; read the parent's
    /// task notifications if that shows up in practice.
    pub fn running(&self, now: DateTime<Utc>, parent_busy: bool) -> bool {
        !self.ended
            && (parent_busy
                || self
                    .last_ts
                    .is_some_and(|t| now - t < chrono::Duration::minutes(15)))
    }
}

/// Where an agent keeps its session files and how to read them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Claude,
    Codex,
    Copilot,
    Pi,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Claude => "claude",
            Kind::Codex => "codex",
            Kind::Copilot => "copilot",
            Kind::Pi => "pi",
        }
    }

    fn files(self, root: &Path) -> Vec<PathBuf> {
        match self {
            Kind::Claude => claude::files(root),
            // sessions/YYYY/MM/DD/rollout-*.jsonl
            Kind::Codex => jsonl_files(&root.join("sessions"), 4),
            Kind::Copilot => list(&root.join("session-state"))
                .into_iter()
                .map(|d| d.join("events.jsonl"))
                .filter(|f| f.is_file())
                .collect(),
            // sessions/--<cwd>--/<timestamp>_<id>.jsonl
            Kind::Pi => jsonl_files(root, 2),
        }
    }

    fn apply(self, state: &mut FileState, v: &Value) {
        match self {
            Kind::Claude => claude::apply(state, v),
            Kind::Codex => codex::apply(state, v),
            Kind::Copilot => copilot::apply(state, v),
            Kind::Pi => pi::apply(state, v),
        }
    }
}

/// What the index keeps per session file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct FileState {
    len: u64,
    offset: u64,
    #[serde(default)]
    bad_lines: u64,
    /// Claude Code writes one line per content block, each repeating the message's usage.
    /// ponytail: copies are consecutive (verified on real transcripts), so the last id is enough.
    #[serde(default)]
    last_message_id: Option<String>,
    /// Day, model and usage counted for `last_message_id`.
    #[serde(default)]
    last_usage: Option<(String, String, Usage)>,
    /// The session finished its last task; only read for subagents.
    #[serde(default)]
    ended: bool,
    /// Subagent description and type.
    #[serde(default)]
    meta: Option<(String, String)>,
    /// A running subagent's latest tool calls and replies; not saved, unfinished subagents are
    /// read again on start.
    #[serde(skip)]
    activity: Vec<String>,
    /// Session id of the session that spawned this one as a subagent.
    #[serde(default)]
    parent: Option<String>,
    /// Model of the turns that follow (Codex, Copilot).
    #[serde(default)]
    model: Option<String>,
    /// Codex: the session's running token total, so repeated counts add only what is new.
    #[serde(default)]
    total: Usage,
    thread: Thread,
}

/// Incremental index over one agent's session files: each refresh reads only appended lines.
pub struct Source {
    kind: Kind,
    root: PathBuf,
    index_path: PathBuf,
    files: HashMap<String, FileState>,
    dirty: bool,
}

impl Source {
    pub fn new(kind: Kind, root: &Path, index_path: PathBuf) -> Source {
        let mut files: HashMap<String, FileState> = std::fs::read_to_string(&index_path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        // Unfinished Claude Code subagents are read again: that restores their logs, and catches
        // ends an older Jarvis did not recognise. A deleted file keeps its entry and its usage.
        if kind == Kind::Claude {
            files.retain(|path, f| f.parent.is_none() || f.ended || !Path::new(path).is_file());
        }
        Source {
            kind,
            root: root.to_path_buf(),
            index_path,
            files,
            dirty: false,
        }
    }

    /// Reads whatever changed since the last call and returns every known thread.
    pub fn refresh(&mut self) -> Vec<Thread> {
        for path in self.kind.files(&self.root) {
            if let Err(e) = self.refresh_file(&path) {
                crate::log::log(format!("transcripts: {}: {e}", path.display()));
            }
        }
        if self.dirty {
            self.save();
        }
        // Subagent sessions add usage and activity to their parent session, and are listed in it.
        let mut threads: HashMap<String, Thread> = HashMap::new();
        let mut subagents = Vec::new();
        for f in self.files.values() {
            match &f.parent {
                Some(parent) => subagents.push((parent, f)),
                None => {
                    threads.insert(f.thread.session_id.clone(), f.thread.clone());
                }
            }
        }
        subagents.sort_by_key(|(_, f)| f.thread.first_ts);
        for (parent, f) in subagents {
            let sub = &f.thread;
            let Some(t) = threads.get_mut(parent) else {
                continue;
            };
            let (description, agent_type) = f.meta.clone().unwrap_or_default();
            t.subagents.push(SubAgent {
                id: sub.session_id.clone(),
                description,
                agent_type,
                last_ts: sub.last_ts,
                ended: f.ended,
                activity: f.activity.clone(),
            });
            t.last_ts = t.last_ts.max(sub.last_ts);
            for (day, models) in &sub.usage {
                for (model, u) in models {
                    *t.usage
                        .entry(day.clone())
                        .or_default()
                        .entry(model.clone())
                        .or_default() += *u;
                }
            }
        }
        threads
            .into_values()
            .filter(|t| t.first_ts.is_some())
            .map(|mut t| {
                t.agent = self.kind.name().to_string();
                t
            })
            .collect()
    }

    fn refresh_file(&mut self, path: &Path) -> std::io::Result<()> {
        let len = std::fs::metadata(path)?.len();
        let kind = self.kind;
        let fresh = || match kind {
            Kind::Claude => claude::fresh(path),
            _ => FileState {
                thread: Thread {
                    session_id: session_from_path(kind, path),
                    ..Default::default()
                },
                ..Default::default()
            },
        };
        let state = self
            .files
            .entry(path.to_string_lossy().to_string())
            .or_insert_with(&fresh);
        if kind == Kind::Claude {
            claude::read_meta_once(path, state);
        }
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
            match serde_json::from_slice::<Value>(line) {
                Ok(v) => kind.apply(state, &v),
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

/// One index per agent that keeps sessions on disk, each at its default place unless its
/// environment variable moves it.
pub fn sources(claude_dir: Option<PathBuf>, state: &Path) -> Vec<Source> {
    use crate::paths::{env_path, home_dir};
    // claude-index-v2 predates subagents keyed by their own id; v3 rebuilds it once.
    let _ = std::fs::remove_file(state.join("claude-index-v2.json"));
    let home = |rel: &str| home_dir().map(|h| h.join(rel));
    let pi = env_path("PI_CODING_AGENT_SESSION_DIR").or_else(|| {
        env_path("PI_CODING_AGENT_DIR")
            .or_else(|| home(".pi/agent"))
            .map(|d| d.join("sessions"))
    });
    [
        (Kind::Claude, claude_dir),
        (
            Kind::Codex,
            env_path("CODEX_HOME").or_else(|| home(".codex")),
        ),
        (
            Kind::Copilot,
            env_path("COPILOT_HOME").or_else(|| home(".copilot")),
        ),
        (Kind::Pi, pi),
    ]
    .into_iter()
    .filter_map(|(kind, root)| {
        // The index is the only record of sessions whose transcripts the agent has since deleted:
        // a rebuild or a new file name must carry those entries over. When parsing changes,
        // re-reading the affected entries in place is simplest (see `Source::new`).
        let index = state.join(format!("{}-index-v3.json", kind.name()));
        Some(Source::new(kind, &root?, index))
    })
    .collect()
}

/// The session id a file name carries before its first line is read.
fn session_from_path(kind: Kind, path: &Path) -> String {
    match kind {
        // session-state/<id>/events.jsonl
        Kind::Copilot => path.parent().and_then(Path::file_name),
        _ => path.file_stem(),
    }
    .map(|s| s.to_string_lossy().to_string())
    .unwrap_or_default()
}

fn list(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect()
}

/// `.jsonl` files at most `depth` directories below `dir`.
fn jsonl_files(dir: &Path, depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in list(dir) {
        if p.is_dir() {
            if depth > 1 {
                out.extend(jsonl_files(&p, depth - 1));
            }
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            out.push(p);
        }
    }
    out
}

/// Local calendar day of a timestamp, `YYYY-MM-DD`.
pub fn local_day(ts: DateTime<Utc>) -> String {
    ts.with_timezone(&Local).format("%Y-%m-%d").to_string()
}

fn timestamp(v: &Value) -> Option<DateTime<Utc>> {
    v["timestamp"].as_str()?.parse().ok()
}

fn note_ts(t: &mut Thread, ts: Option<DateTime<Utc>>) {
    if let Some(ts) = ts {
        t.first_ts.get_or_insert(ts);
        t.last_ts = Some(t.last_ts.map_or(ts, |last| last.max(ts)));
    }
}

fn add_usage(t: &mut Thread, ts: DateTime<Utc>, model: &str, u: Usage) {
    if u.total() > 0 {
        *t.usage
            .entry(local_day(ts))
            .or_default()
            .entry(model.to_string())
            .or_default() += u;
    }
}

/// Counts a typed prompt; agents without generated titles take the first one as title.
fn note_prompt(t: &mut Thread, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    t.turns += 1;
    t.last_prompt = Some(snippet(text));
    if text.starts_with('<') {
        return;
    }
    t.title.get_or_insert_with(|| {
        text.lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(TITLE_CHARS)
            .collect()
    });
}

/// A string, or the `text` blocks of a content array.
fn text_of(content: &Value) -> Option<String> {
    if let Some(s) = content.as_str() {
        return Some(s.trim().to_string()).filter(|s| !s.is_empty());
    }
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

#[cfg(test)]
pub(crate) mod test_util {
    use super::*;

    pub fn ts(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    /// Writes `lines` to `<tmp>/<rel>` and indexes `<tmp>/root` with `kind`.
    pub fn index(kind: Kind, rel: &str, lines: &[&str]) -> Vec<Thread> {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("root").join(rel);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, lines.join("\n") + "\n").unwrap();
        Source::new(
            kind,
            &tmp.path().join("root"),
            tmp.path().join("index.json"),
        )
        .refresh()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_commands_per_agent() {
        let t = |agent: &str| Thread {
            session_id: "s1".into(),
            agent: agent.into(),
            ..Default::default()
        };
        assert_eq!(t("claude").resume_command(), "claude --resume s1");
        assert_eq!(t("codex").resume_command(), "codex resume s1");
        assert_eq!(t("copilot").resume_command(), "copilot --resume=s1");
        assert_eq!(t("pi").resume_command(), "pi --session s1");
    }

    #[test]
    fn subagent_runs_while_its_parent_is_busy() {
        let now = test_util::ts("2026-10-08T12:00:00Z");
        let sub = |ago: i64, ended| SubAgent {
            last_ts: Some(now - chrono::Duration::minutes(ago)),
            ended,
            ..Default::default()
        };
        assert!(sub(40, false).running(now, true));
        assert!(!sub(40, false).running(now, false));
        assert!(sub(5, false).running(now, false));
        assert!(!sub(0, true).running(now, true));
    }

    #[test]
    fn prompt_gives_title_once() {
        let mut t = Thread::default();
        note_prompt(&mut t, "  first line\nmore");
        note_prompt(&mut t, "second");
        let mut cmd = Thread::default();
        note_prompt(&mut cmd, "<command-name>/model</command-name>");
        assert_eq!((cmd.turns, cmd.title.as_deref()), (1, None));
        assert_eq!(t.title.as_deref(), Some("first line"));
        assert_eq!((t.turns, t.last_prompt.as_deref()), (2, Some("second")));
    }
}
