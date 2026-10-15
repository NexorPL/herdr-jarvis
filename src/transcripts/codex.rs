//! Codex: `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<time>-<thread id>.jsonl`, one
//! `{timestamp, type, payload}` per line.

use super::{add_usage, note_prompt, note_ts, text_of, timestamp, FileState};
use crate::pricing::Usage;
use serde_json::Value;

pub(super) fn apply(state: &mut FileState, v: &Value) {
    let ts = timestamp(v);
    let t = &mut state.thread;
    note_ts(t, ts);
    let p = &v["payload"];
    match v["type"].as_str() {
        Some("session_meta") => {
            if let Some(id) = p["id"].as_str() {
                t.session_id = id.to_string();
            }
            set(&mut t.cwd, &p["cwd"]);
            set(&mut t.branch, &p["git"]["branch"]);
            // A thread spawned by another one is that thread's subagent.
            let spawn = &p["source"]["subagent"]["thread_spawn"];
            let parent = p["parent_thread_id"]
                .as_str()
                .or(spawn["parent_thread_id"].as_str());
            if let Some(parent) = parent {
                state.parent = Some(parent.to_string());
                let s = |k: &str| {
                    p[k].as_str()
                        .or(spawn[k].as_str())
                        .unwrap_or_default()
                        .to_string()
                };
                state.meta = Some((s("agent_nickname"), s("agent_role")));
            }
        }
        Some("turn_context") => {
            set(&mut state.model, &p["model"]);
            set(&mut t.cwd, &p["cwd"]);
        }
        Some("event_msg") => match p["type"].as_str() {
            Some("user_message") => {
                state.ended = false;
                if let Some(text) = text_of(&p["message"]) {
                    note_prompt(t, &text);
                }
            }
            Some("agent_message") => {
                if let Some(text) = text_of(&p["message"]) {
                    t.last_reply = Some(super::snippet(&text));
                }
            }
            Some("task_complete" | "turn_complete") => state.ended = true,
            // ponytail: counts are running totals for the thread and may repeat; only growth is
            // added, attributed to the current model. A model switch mid-turn lands on the new one.
            Some("token_count") => {
                let total = usage_of(&p["info"]["total_token_usage"]);
                if total.total() > state.total.total() {
                    let mut delta = total;
                    delta -= state.total;
                    if let Some(ts) = ts {
                        let model = state.model.as_deref().unwrap_or("codex");
                        add_usage(t, ts, model, delta);
                    }
                    state.total = total;
                }
            }
            _ => {}
        },
        _ => {}
    }
}

fn set(field: &mut Option<String>, v: &Value) {
    if let Some(s) = v.as_str().filter(|s| !s.is_empty()) {
        *field = Some(s.to_string());
    }
}

/// OpenAI counts cached tokens inside `input_tokens`.
fn usage_of(u: &Value) -> Usage {
    let n = |k: &str| u[k].as_u64().unwrap_or(0);
    let (cached, written) = (n("cached_input_tokens"), n("cache_write_input_tokens"));
    Usage {
        input: n("input_tokens").saturating_sub(cached + written),
        output: n("output_tokens"),
        cache_read: cached,
        cache_write_5m: written,
        cache_write_1h: 0,
    }
}

#[cfg(test)]
mod tests {
    use crate::transcripts::test_util::{index, ts};
    use crate::transcripts::{local_day, Kind};

    const ROLLOUT: &[&str] = &[
        r#"{"timestamp":"2026-10-08T10:00:00.000Z","type":"session_meta","payload":{"id":"th-1","timestamp":"2026-10-08T10:00:00Z","cwd":"/home/u/alpha","originator":"codex_cli_rs","cli_version":"0.50.0","git":{"branch":"main"}}}"#,
        r#"{"timestamp":"2026-10-08T10:00:01.000Z","type":"turn_context","payload":{"cwd":"/home/u/alpha","model":"gpt-5.5-codex"}}"#,
        r#"{"timestamp":"2026-10-08T10:00:02.000Z","type":"event_msg","payload":{"type":"user_message","message":"Add a retry\nto the client"}}"#,
        r#"{"timestamp":"2026-10-08T10:00:03.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[]}}"#,
        r#"{"timestamp":"2026-10-08T10:00:04.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":800,"output_tokens":50,"reasoning_output_tokens":10,"total_tokens":1050},"last_token_usage":{}}}}"#,
        r#"{"timestamp":"2026-10-08T10:00:04.500Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":800,"output_tokens":50,"reasoning_output_tokens":10,"total_tokens":1050}}}}"#,
        r#"{"timestamp":"2026-10-08T10:00:05.000Z","type":"event_msg","payload":{"type":"agent_message","message":"Retry added."}}"#,
        r#"{"timestamp":"2026-10-08T10:00:06.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1500,"cached_input_tokens":1000,"output_tokens":80,"total_tokens":1580}}}}"#,
        r#"not json"#,
    ];

    #[test]
    fn reads_a_rollout() {
        let threads = index(
            Kind::Codex,
            "sessions/2026/10/08/rollout-x-th-1.jsonl",
            ROLLOUT,
        );
        let t = &threads[0];
        assert_eq!((t.session_id.as_str(), t.agent.as_str()), ("th-1", "codex"));
        assert_eq!(t.title.as_deref(), Some("Add a retry"));
        assert_eq!(
            (t.cwd.as_deref(), t.branch.as_deref()),
            (Some("/home/u/alpha"), Some("main"))
        );
        assert_eq!(
            (t.turns, t.last_reply.as_deref()),
            (1, Some("Retry added."))
        );
        assert_eq!(t.last_ts, Some(ts("2026-10-08T10:00:06Z")));
        // Repeated total counted once; growth added on top.
        let u = t.usage[&local_day(ts("2026-10-08T10:00:04Z"))]["gpt-5.5-codex"];
        assert_eq!((u.input, u.cache_read, u.output), (500, 1000, 80));
    }

    #[test]
    fn spawned_thread_is_a_subagent_of_its_parent() {
        let child = r#"{"timestamp":"2026-10-08T10:01:00.000Z","type":"session_meta","payload":{"id":"th-2","cwd":"/home/u/alpha","source":{"subagent":{"thread_spawn":{"parent_thread_id":"th-1","depth":1,"agent_nickname":"Scout","agent_role":"explorer"}}}}}"#;
        let tmp = tempfile::tempdir().unwrap();
        let day = tmp.path().join("root/sessions/2026/10/08");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(day.join("a.jsonl"), ROLLOUT.join("\n") + "\n").unwrap();
        std::fs::write(day.join("b.jsonl"), format!("{child}\n")).unwrap();
        let threads = crate::transcripts::Source::new(
            Kind::Codex,
            &tmp.path().join("root"),
            tmp.path().join("i.json"),
        )
        .refresh();
        assert_eq!(threads.len(), 1);
        let s = &threads[0].subagents[0];
        assert_eq!(
            (s.id.as_str(), s.description.as_str(), s.agent_type.as_str()),
            ("th-2", "Scout", "explorer")
        );
        assert!(!s.ended);
    }
}
