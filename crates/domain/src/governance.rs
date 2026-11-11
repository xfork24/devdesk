//! Versioned configuration and executable workflow contracts. No I/O.
use crate::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scope {
    #[default]
    Global,
    Project {
        project_id: Id,
    },
    Feature {
        project_id: Id,
        feature_id: Id,
    },
}
impl Scope {
    pub fn project(&self) -> Option<&str> {
        match self {
            Self::Global => None,
            Self::Project { project_id } | Self::Feature { project_id, .. } => Some(project_id),
        }
    }
    pub fn visible_from(&self, target: &Self) -> bool {
        match (self, target) {
            (Self::Global, _) => true,
            (
                Self::Project { project_id: a },
                Self::Project { project_id: b } | Self::Feature { project_id: b, .. },
            ) => a == b,
            (
                Self::Feature {
                    project_id: a,
                    feature_id: x,
                },
                Self::Feature {
                    project_id: b,
                    feature_id: y,
                },
            ) => a == b && x == y,
            _ => false,
        }
    }
}
pub fn configuration_hash<T: Serialize>(value: &T) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).expect("serializable configuration"))
    )
}

/// Typed envelope shared by configuration stores. Mutable kinds are explicitly allowlisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigurationRecord {
    pub id: Id,
    pub kind: String,
    pub definition_id: Id,
    pub scope: Scope,
    pub name: String,
    pub version: u64,
    pub supersedes: Option<Id>,
    pub content_hash: String,
    pub data: Value,
    pub immutable: bool,
    pub revision: u64,
    pub created_at: u64,
}
impl ConfigurationRecord {
    pub fn decode<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_value(self.data.clone())
            .map_err(|e| err("invalid_configuration", format!("{}: {e}", self.id)))
    }
    pub fn is_definition(&self) -> bool {
        [
            "model",
            "tool",
            "harness",
            "agent",
            "workflow",
            "standard",
            "skill",
            "spec",
            "delivery_type",
            "role",
            "capability",
            "integration",
            "integration_binding",
            "sandbox",
            "permission_context",
            "permission_profile",
            "permission_binding",
            "permission_assignment",
        ]
        .contains(&self.kind.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDefinition {
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub release: Option<String>,
    #[serde(default)]
    pub parameters: BTreeMap<String, Value>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessDefinition {
    pub provider: String,
    pub runtime_version: String,
    /// Exact managed build inputs; its first verified image is permanently bound separately.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_digest: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub model_providers: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<PlatformToolRuntime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_provider: Option<String>,
    pub qualified_name: String,
    pub implementation: String,
    pub input_schema: Value,
    pub output_schema: Value,
    #[serde(default)]
    pub capabilities: Vec<String>,
}
impl ToolDefinition {
    /// Provider function names use portable identifier characters; handler IDs remain unchanged.
    pub fn call_name(&self) -> String {
        if self.implementation == "platform" {
            format!("devdesk_{}", self.qualified_name.replace('.', "_"))
        } else {
            self.qualified_name.clone()
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformToolRuntime {
    pub harness_ref: Id,
    pub sandbox_ref: Id,
}
#[derive(Debug, Clone, Serialize)]
pub struct AgentComposition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_ref: Option<Id>,
    pub role: AgentRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_ref: Option<Id>,
    pub instructions: String,
    pub model_ref: Id,
    pub harness_ref: Id,
    #[serde(default)]
    pub skills: Vec<Id>,
    #[serde(default)]
    pub tools: Vec<Id>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleDefinition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<DefinitionVisibility>,
    pub instructions: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentInstance {
    pub agent_revision_id: Id,
    pub scope: Scope,
    #[serde(default)]
    pub source_instance_id: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentExecutionConfiguration {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integration_lock: Option<IntegrationLock>,
    pub agent_instance_id: Id,
    pub agent_revision_id: Id,
    pub composition_hash: String,
    pub model_revision_id: Id,
    pub harness_revision_id: Id,
    pub model_provider: String,
    pub model_parameters: BTreeMap<String, Value>,
    pub tool_revisions: Vec<Id>,
}

pub fn topological(nodes: &[(String, Vec<String>)]) -> Result<Vec<String>> {
    valid_dag(nodes)?;
    let mut done = Vec::new();
    while done.len() < nodes.len() {
        let next = nodes
            .iter()
            .find(|(key, deps)| !done.contains(key) && deps.iter().all(|d| done.contains(d)))
            .ok_or_else(|| err("invalid_workflow", "流程含环或缺失依赖"))?;
        done.push(next.0.clone());
    }
    Ok(done)
}

impl EngineeringState {
    pub fn workflow_paused(&self, owner: &str) -> bool {
        self.governance.iter().any(|r| {
            r.kind == "workflow_instance" && r.data["paused"] == true && r.data["owner_id"] == owner
        })
    }
    pub fn configuration(&self, id: &str) -> Result<&ConfigurationRecord> {
        self.governance
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| err("not_found", format!("配置不存在: {id}")))
    }
    pub fn validate_governance(&self, before: &Self) -> Result<()> {
        let mut ids = BTreeSet::new();
        let mut versions = BTreeSet::new();
        for r in &self.governance {
            require(ids.insert(&r.id), "配置 ID 重复")?;
            if r.is_definition() || r.kind == "project_configuration" {
                require(
                    versions.insert((&r.kind, &r.definition_id, r.version)),
                    "配置修订版本重复",
                )?;
            }
            if let Some(old) = before.governance.iter().find(|x| x.id == r.id) {
                require(
                    old.kind == r.kind
                        && old.definition_id == r.definition_id
                        && old.scope == r.scope,
                    "配置身份和 Scope 不可更改",
                )?;
                if old.immutable {
                    require(old == r, "已发布配置和锁不可更改")?;
                }
                if r.kind == "agent_instance" {
                    require(
                        old.data["agent_revision_id"] == r.data["agent_revision_id"]
                            && old.data["scope"] == r.data["scope"],
                        "Agent 的组合和范围不可更改",
                    )?;
                }
            }
            require(
                r.content_hash == configuration_hash(&r.data),
                "配置内容摘要不一致",
            )?;
        }
        require(
            before
                .governance
                .iter()
                .all(|r| self.governance.iter().any(|x| x.id == r.id)),
            "配置历史不能删除",
        )?;
        Ok(())
    }
}
