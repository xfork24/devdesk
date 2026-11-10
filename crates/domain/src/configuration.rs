//! Independent configuration resources and explicit workflow specifications.
use crate::*;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DefinitionVisibility {
    #[default]
    Global,
    Projects {
        project_ids: Vec<Id>,
    },
}
impl DefinitionVisibility {
    pub fn validate(&self) -> Result<()> {
        if let Self::Projects { project_ids } = self {
            require(
                !project_ids.is_empty() && project_ids.len() <= 1000,
                "请选择明确的项目范围",
            )?;
            let unique: std::collections::BTreeSet<_> = project_ids.iter().collect();
            require(
                unique.len() == project_ids.len() && project_ids.iter().all(|id| !id.is_empty()),
                "项目范围不得重复或为空",
            )?;
        }
        Ok(())
    }
    pub fn permits(&self, project: Option<&str>) -> bool {
        match self {
            Self::Global => true,
            Self::Projects { project_ids } => {
                project.is_some_and(|id| project_ids.iter().any(|p| p == id))
            }
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDefinition {
    pub model_ref: Id,
    pub harness_ref: Id,
    #[serde(default)]
    pub tools: Vec<Id>,
    #[serde(default)]
    pub skills: Vec<Id>,
}
