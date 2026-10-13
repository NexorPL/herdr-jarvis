//! GitHub Copilot CLI: `$COPILOT_HOME/session-state/<id>/events.jsonl`, one `{type, data, timestamp}`
//! event per line.

use super::{add_usage, note_prompt, note_ts, text_of, timestamp, FileState, SubAgent};
use crate::pricing::Usage;
use serde_json::Value;

pub(super) fn apply(state: &mut FileState, v: &Value) {
    let ts = timestamp(v);
    let t = &mut state.thread;
    note_ts(t, ts);
    let d = &v["data"];
    match v["type"].as_str() {
        Some("session.start" | "session.resume") => {
            let c = &d["context"];
            if let Some(cwd) = c["cwd"].as_str() {
                t.cwd = Some(cwd.to_string());
            }
            if let Some(branch) = c["branch"].as_str().filter(|b| !b.is_empty()) {
                t.branch = Some(branch.to_string());
            }
        }
        // Skills and system notes arrive as user messages with a `source`; typed prompts have none.
        Some("user.message") if d["source"].is_null() => {
            if let Some(text) = text_of(&d["content"]) {
                note_prompt(t, &text);
            }
        }
        Some("assistant.message") => match d["parentToolCallId"].as_str() {
            Some(call) => {
                if let Some(s) = t.subagents.iter_mut().find(|s| s.id == call) {
                    s.last_ts = s.last_ts.max(ts);
                }
            }
            None => {
                if let Some(text) = text_of(&d["content"]) {
                    t.last_reply = Some(super::snippet(&text));
                }
            }
        },
        Some("subagent.started") => {
            let s = |k: &str| d[k].as_str().unwrap_or_default().to_string();
            let description = Some(s("agentDescription"))
                .filter(|x| !x.is_empty())
                .unwrap_or_else(|| s("agentDisplayName"));
            t.subagents.push(SubAgent {
                id: s("toolCallId"),
                description,
                agent_type: s("agentName"),
                last_ts: ts,
                ended: false,
            });
        }
        Some("subagent.completed" | "subagent.failed") => {
            let id = d["toolCallId"].as_str();
            if let Some(s) = t.subagents.iter_mut().find(|s| Some(s.id.as_str()) == id) {
                s.ended = true;
                s.last_ts = s.last_ts.max(ts);
            }
        }
        // ponytail: Copilot writes token counts only when a run of the session exits, one total per
        // model for that run, so a live session shows its usage after it closes.
        Some("session.shutdown") => {
            let (Some(metrics), Some(ts)) = (d["modelMetrics"].as_object(), ts) else {
                return;
            };
            for (model, m) in metrics {
                // Copilot names Claude models `claude-opus-4.6`; the price table uses dashes.
                let model = match model.starts_with("claude-") {
                    true => model.replace('.', "-"),
                    false => model.clone(),
                };
                add_usage(t, ts, &model, usage_of(&m["usage"]));
            }
        }
        _ => {}
    }
}

/// `inputTokens` includes cache reads and writes.
fn usage_of(u: &Value) -> Usage {
    let n = |k: &str| u[k].as_u64().unwrap_or(0);
    let (read, write) = (n("cacheReadTokens"), n("cacheWriteTokens"));
    Usage {
        input: n("inputTokens").saturating_sub(read + write),
        output: n("outputTokens"),
        cache_read: read,
        cache_write_5m: write,
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
            Kind::Copilot,
            "session-state/cp-1/events.jsonl",
            &[
                r#"{"type":"session.start","data":{"sessionId":"cp-1","context":{"cwd":"C:\\p\\alpha","branch":"main"}},"timestamp":"2026-10-08T10:00:00.000Z"}"#,
                r#"{"type":"user.message","data":{"content":"Fix the login","parentAgentTaskId":"x"},"timestamp":"2026-10-08T10:00:01.000Z"}"#,
                r#"{"type":"user.message","data":{"content":"<skill>","source":"skill-brainstorming"},"timestamp":"2026-10-08T10:00:02.000Z"}"#,
                r#"{"type":"subagent.started","data":{"toolCallId":"call-1","agentName":"explore","agentDisplayName":"Explore","agentDescription":"Find login code"},"timestamp":"2026-10-08T10:00:03.000Z"}"#,
                r#"{"type":"assistant.message","data":{"content":"inside","parentToolCallId":"call-1"},"timestamp":"2026-10-08T10:00:04.000Z"}"#,
                r#"{"type":"subagent.completed","data":{"toolCallId":"call-1"},"timestamp":"2026-10-08T10:00:05.000Z"}"#,
                r#"{"type":"assistant.message","data":{"content":"Login fixed.","model":"claude-opus-4.6"},"timestamp":"2026-10-08T10:00:06.000Z"}"#,
                r#"{"type":"session.shutdown","data":{"modelMetrics":{"claude-opus-4.6":{"usage":{"inputTokens":1000,"outputTokens":50,"cacheReadTokens":700,"cacheWriteTokens":200}}}},"timestamp":"2026-10-08T10:00:07.000Z"}"#,
            ],
        );
        let t = &threads[0];
        assert_eq!(
            (t.session_id.as_str(), t.agent.as_str()),
            ("cp-1", "copilot")
        );
        assert_eq!((t.turns, t.title.as_deref()), (1, Some("Fix the login")));
        assert_eq!(t.last_reply.as_deref(), Some("Login fixed."));
        assert_eq!(t.branch.as_deref(), Some("main"));
        let s = &t.subagents[0];
        assert_eq!(
            (s.description.as_str(), s.agent_type.as_str(), s.ended),
            ("Find login code", "explore", true)
        );
        let u = t.usage[&local_day(ts("2026-10-08T10:00:07Z"))]["claude-opus-4-6"];
        assert_eq!(
            (u.input, u.output, u.cache_read, u.cache_write_5m),
            (100, 50, 700, 200)
        );
    }
}
