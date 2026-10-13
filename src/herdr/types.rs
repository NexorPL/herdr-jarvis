use serde::{Deserialize, Serialize};

/// Agent lifecycle state. Declaration order is urgency order: most urgent first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Blocked,
    Done,
    Working,
    Idle,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AgentSession {
    pub value: String,
}

/// A pane, or an agent entry, from `session.snapshot` (agents share the pane shape).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Pane {
    pub pane_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    #[serde(default)]
    pub cwd: Option<String>,
    pub agent_status: AgentStatus,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub agent_session: Option<AgentSession>,
    #[serde(default)]
    pub terminal_title_stripped: Option<String>,
    /// Pane label; plugin panes carry their manifest title (Jarvis: "Jarvis").
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub focused: bool,
}

impl Pane {
    pub fn title(&self) -> &str {
        self.terminal_title_stripped.as_deref().unwrap_or("")
    }

    /// Pi's integration reports the session file (`<timestamp>_<id>.jsonl`) rather than the id.
    pub fn session_id(&self) -> Option<&str> {
        let v = self.agent_session.as_ref()?.value.as_str();
        Some(match v.strip_suffix(".jsonl") {
            Some(path) => path.rsplit('_').next().unwrap_or(v),
            None => v,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Workspace {
    pub workspace_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub number: u32,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub protocol: u32,
    #[serde(default)]
    pub focused_pane_id: Option<String>,
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    #[serde(default)]
    pub panes: Vec<Pane>,
    #[serde(default)]
    pub agents: Vec<Pane>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pi_session_path_gives_its_id() {
        let pane = |value: &str| Pane {
            agent_session: Some(AgentSession {
                value: value.into(),
            }),
            ..serde_json::from_value(serde_json::json!({
                "pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t1", "agent_status": "idle"
            }))
            .unwrap()
        };
        let path =
            r"C:\Users\u\.pi\agent\sessions\--C--my_app--\2026-10-08T10-00-00-000Z_9f2c.jsonl";
        assert_eq!(pane(path).session_id(), Some("9f2c"));
        assert_eq!(pane("sess-a").session_id(), Some("sess-a"));
    }
}
