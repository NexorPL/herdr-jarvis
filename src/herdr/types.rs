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

    pub fn session_id(&self) -> Option<&str> {
        self.agent_session.as_ref().map(|s| s.value.as_str())
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
