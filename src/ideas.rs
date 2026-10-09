//! Per-project ideas, stored as one JSON array in the plugin state directory.

use serde::{Deserialize, Serialize};

/// An idea belongs to a project, not to an agent: agents come and go, ideas stay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Idea {
    pub project_key: String,
    /// Kept so ideas of projects without live agents still show a readable name.
    pub project_name: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: Status,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    Todo,
    Doing,
    Done,
}

impl Status {
    pub fn next(self) -> Status {
        match self {
            Status::Todo => Status::Doing,
            Status::Doing => Status::Done,
            Status::Done => Status::Todo,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::Todo => "○ todo",
            Status::Doing => "◐ doing",
            Status::Done => "✓ done",
        }
    }
}
