//! Agent identity/revisions are independent of runtime installation and model preferences.
use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Analyst,
    Architect,
    Developer,
    Tester,
    Reviewer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkType {
    Discussion,
    Design,
    Implementation,
    Testing,
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPolicy {
    ReadOnly,
    TaskWrite,
    TestWrite,
}

/// Typed extension of an immutable Agent Definition revision. Definition.body owns instructions;
/// Definition.id/version/supersedes preserve the existing reviewer identity-root semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProfile {
    #[serde(default)]
    pub permission_overrides: Option<PermissionOverrides>,
    pub role: AgentRole,
    pub skill_revisions: Vec<Id>,
    pub tool_policy: ToolPolicy,
    pub allowed_work: Vec<WorkType>,
    pub output_schemas: Vec<String>,
}

impl AgentProfile {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.allowed_work.is_empty() && !self.output_schemas.is_empty(),
            "角色必须指定工作类型和输出 schema",
        )?;
        require(
            self.role != AgentRole::Reviewer || self.tool_policy == ToolPolicy::ReadOnly,
            "评审角色必须只读",
        )?;
        require(
            !self.allowed_work.contains(&WorkType::Implementation)
                || self.tool_policy == ToolPolicy::TaskWrite,
            "实施角色需要明确 Task 写权限",
        )?;
        require(
            !self.allowed_work.contains(&WorkType::Review) || self.role == AgentRole::Reviewer,
            "正式 Agent Review 必须由评审角色执行",
        )
    }
}
