//! Agent identity/revisions are independent of runtime installation and model preferences.
use crate::*;

/// Business identities are supplied by Role configuration, not an engine enum.
pub type AgentRole = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkType {
    Workflow,
    Discussion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPolicy {
    ReadOnly,
    WorkspaceWrite,
    ScopedWrite,
}
impl ToolPolicy {
    pub fn permits(self, requested: Self) -> bool {
        self == Self::WorkspaceWrite || requested == Self::ReadOnly || self == requested
    }
}

/// Typed extension of an immutable Agent Definition revision. Definition.body owns instructions;
/// Definition.id/version/supersedes preserve the existing reviewer identity-root semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProfile {
    pub role: AgentRole,
    pub skill_revisions: Vec<Id>,
    pub tool_policy: ToolPolicy,
}

impl AgentProfile {
    pub fn validate(&self) -> Result<()> {
        require(!self.role.trim().is_empty(), "Agent 必须明确业务 Role 身份")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentWorkProfile {
    pub tool_policy: ToolPolicy,
}
impl Default for AgentWorkProfile {
    fn default() -> Self {
        Self {
            tool_policy: ToolPolicy::ReadOnly,
        }
    }
}
