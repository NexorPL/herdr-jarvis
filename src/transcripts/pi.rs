//! Pi: `~/.pi/agent/sessions/--<cwd>--/<timestamp>_<id>.jsonl`, a session header then entries.

use super::{add_usage, note_prompt, note_ts, text_of, timestamp, FileState};
use crate::pricing::Usage;
use serde_json::Value;

pub(super) fn apply(state: &mut FileState, v: &Value) {
    let ts = timestamp(v);
    let t = &mut state.thread;
    note_ts(t, ts);
    match v["type"].as_str() {
        Some("session") => {
            if let Some(id) = v["id"].as_str() {
                t.session_id = id.to_string();
            }
            t.cwd = v["cwd"].as_str().map(str::to_string);
        }
        Some("session_info") => {
            if let Some(name) = v["name"].as_str().filter(|n| !n.is_empty()) {
                t.title = Some(name.to_string());
            }
        }
        Some("message") => {
            let m = &v["message"];
            match m["role"].as_str() {
                Some("user") => {
                    if let Some(text) = text_of(&m["content"]) {
                        note_prompt(t, &text);
                    }
                }
                Some("assistant") => {
                    if let Some(text) = text_of(&m["content"]) {
                        t.last_reply = Some(super::snippet(&text));
                    }
                    if let (Some(model), Some(ts)) = (m["model"].as_str(), ts) {
                        add_usage(t, ts, model, usage_of(&m["usage"]));
                    }
                }
                _ => {}
            }
        }
        // Usage outside the conversation, such as cache warming.
        Some("usage") => {
            if let (Some(model), Some(ts)) = (v["model"].as_str(), ts) {
                add_usage(t, ts, model, usage_of(&v["usage"]));
            }
        }
        _ => {}
    }
}

/// ponytail: Pi also stores its own `cost`; Jarvis prices tokens itself so every agent's cost
/// comes from one table. Use `cost.total` if Pi's providers drift from those prices.
fn usage_of(u: &Value) -> Usage {
    let n = |k: &str| u[k].as_u64().unwrap_or(0);
    Usage {
        input: n("input"),
        output: n("output"),
        cache_read: n("cacheRead"),
        cache_write_5m: n("cacheWrite"),
        cache_write_1h: 0,
    }
}

#[cfg(test)]
mod tests {
    use crate::transcripts::test_util::{index, ts};
    use crate::transcripts::{local_day, Kind};

    #[test]
    fn reads_a_session() {
        let threads = index(
            Kind::Pi,
            "--home-u-alpha--/2026-10-08T10-00-00-000Z_pi-1.jsonl",
            &[
                r#"{"type":"session","version":3,"id":"pi-1","timestamp":"2026-10-08T10:00:00.000Z","cwd":"/home/u/alpha"}"#,
                r#"{"type":"message","id":"a1","parentId":null,"timestamp":"2026-10-08T10:00:01.000Z","message":{"role":"user","content":"Fix the build","timestamp":1}}"#,
                r#"{"type":"message","id":"a2","parentId":"a1","timestamp":"2026-10-08T10:00:02.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hm"},{"type":"text","text":"Fixed."}],"provider":"anthropic","model":"claude-sonnet-5","usage":{"input":10,"output":20,"cacheRead":300,"cacheWrite":40,"totalTokens":370,"cost":{"total":0.01}},"stopReason":"stop","timestamp":2}}"#,
                r#"{"type":"message","id":"a3","parentId":"a2","timestamp":"2026-10-08T10:00:03.000Z","message":{"role":"toolResult","toolCallId":"c1","content":[{"type":"text","text":"ok"}],"timestamp":3}}"#,
                r#"{"type":"usage","id":"a4","parentId":"a3","timestamp":"2026-10-08T10:00:04.000Z","kind":"cache_warm","provider":"anthropic","model":"claude-sonnet-5","usage":{"input":0,"output":0,"cacheRead":50,"cacheWrite":0}}"#,
                r#"{"type":"session_info","id":"a5","parentId":"a4","timestamp":"2026-10-08T10:00:05.000Z","name":"Build fix"}"#,
            ],
        );
        let t = &threads[0];
        assert_eq!((t.session_id.as_str(), t.agent.as_str()), ("pi-1", "pi"));
        assert_eq!(t.title.as_deref(), Some("Build fix"));
        assert_eq!((t.turns, t.last_reply.as_deref()), (1, Some("Fixed.")));
        assert_eq!(t.cwd.as_deref(), Some("/home/u/alpha"));
        let u = t.usage[&local_day(ts("2026-10-08T10:00:02Z"))]["claude-sonnet-5"];
        assert_eq!(
            (u.input, u.output, u.cache_read, u.cache_write_5m),
            (10, 20, 350, 40)
        );
    }
}
