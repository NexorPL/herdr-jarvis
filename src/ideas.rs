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
}
