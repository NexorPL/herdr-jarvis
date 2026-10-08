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
}
