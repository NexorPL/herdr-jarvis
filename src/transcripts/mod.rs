pub mod claude;

use crate::pricing::Usage;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One agent conversation (a Claude Code session).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Thread {
    pub session_id: String,
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
    /// Subagents the session spawned (Claude Code's Agent tool); they run inside its pane.
    #[serde(default)]
    pub subagents: Vec<SubAgent>,
}

/// One subagent, from `<session>/subagents/agent-<id>.jsonl` and its `.meta.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SubAgent {
    pub id: String,
    pub description: String,
    pub agent_type: String,
    pub last_ts: Option<DateTime<Utc>>,
    /// Its last reply ended its turn (`stop_reason: end_turn`).
    pub ended: bool,
}

impl SubAgent {
    /// ponytail: a subagent killed mid-turn never writes `end_turn`, so 15 minutes of silence
    /// counts as stopped; raise it if long tool calls make running subagents disappear.
    pub fn running(&self, now: DateTime<Utc>) -> bool {
        !self.ended
            && self
                .last_ts
                .is_some_and(|t| now - t < chrono::Duration::minutes(15))
    }
}
