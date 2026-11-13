//! Versioned configuration and executable workflow contracts. No I/O.
use crate::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Hierarchical scope locator: `global`, `project:<id>` or `feature:<pid>:<fid>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Scope(pub String);
impl Default for Scope {
    fn default() -> Self {
        Self::global()
    }
}
impl<'de> Deserialize<'de> for Scope {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        let parts: Vec<_> = value.split(':').collect();
        let valid_id =
            |id: &str| !id.is_empty() && !id.chars().any(|c| c.is_whitespace() || c.is_control());
        let valid = match parts.as_slice() {
            ["global"] => true,
            ["project", project] => valid_id(project),
            ["feature", project, feature] => valid_id(project) && valid_id(feature),
            _ => false,
        };
        if !valid {
            return Err(serde::de::Error::custom(
                "Scope 必须为 global、project:<id> 或 feature:<pid>:<fid>",
            ));
        }
        Ok(Self(value))
    }
}
impl Scope {
    pub fn global() -> Self {
        Self("global".into())
    }
    pub fn project(project_id: impl Into<String>) -> Self {
        Self(format!("project:{}", project_id.into()))
    }
    pub fn feature(project_id: impl Into<String>, feature_id: impl Into<String>) -> Self {
        Self(format!(
            "feature:{}:{}",
            project_id.into(),
            feature_id.into()
        ))
    }
    pub fn is_global(&self) -> bool {
        self.0 == "global"
    }
    pub fn is_feature(&self) -> bool {
        self.0.starts_with("feature:")
    }
    /// Owning project for project and feature scopes.
    pub fn project_id(&self) -> Option<&str> {
        let rest = self
            .0
            .strip_prefix("project:")
            .or_else(|| self.0.strip_prefix("feature:"))?;
        rest.split(':').next()
    }
    pub fn visible_from(&self, target: &Self) -> bool {
        if self.is_global() {
            return true;
        }
        if let Some(feature) = self.0.strip_prefix("feature:") {
            return target.0.strip_prefix("feature:") == Some(feature);
        }
        self.0.strip_prefix("project:").is_some_and(|project| {
            target.0.strip_prefix("project:") == Some(project)
                || target
                    .0
                    .strip_prefix("feature:")
                    .is_some_and(|f| f.split(':').next() == Some(project))
        })
    }
}
pub fn configuration_hash<T: Serialize>(value: &T) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).expect("serializable configuration"))
    )
}

/// Flattened reference fields live directly on the envelope, one `identity:revision`
/// string (or array of them) per key, allowlisted per definition type.
pub type References = BTreeMap<String, Value>;
/// Root-level reference keys owned by each definition type; unlisted types carry none.
pub fn reference_fields(kind: &str) -> &'static [&'static str] {
    match kind {
        "capability" => &["harness_ref", "model_ref", "skills", "tools"],
        "agent" => &["role_ref", "capability_ref"],
        "delivery_type" => &["spec_refs"],
        "harness" => &[
            "model_providers",
            "model_catalog_ref",
            "runtime_resource_ref",
        ],
        "permission_context" => &["sandbox_ref"],
        "standard" => &["root_workflow_refs"],
        _ => &[],
    }
}
/// The content digest covers flattened references as well as the remaining payload.
pub fn content_digest(refs: &References, data: &Value) -> String {
    configuration_hash(&(refs, data))
}
/// Missing keys read as JSON null, mirroring `Value` indexing on the payload map.
pub fn reference_value<'a>(refs: &'a References, key: &str) -> &'a Value {
    refs.get(key).unwrap_or(&Value::Null)
}
pub fn reference_id(refs: &References, key: &str) -> Result<Id> {
    refs.get(key)
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| err("invalid_source", format!("缺少引用字段 {key}")))
}
pub fn reference_list(refs: &References, key: &str) -> Result<Vec<Id>> {
    refs.get(key)
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .map(|v| v.as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .ok_or_else(|| err("invalid_source", format!("引用字段 {key} 必须为列表")))
}

/// Typed envelope shared by configuration stores. Mutable kinds are explicitly allowlisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigurationRecord {
    pub id: Id,
    pub r#type: String,
    pub definition_id: Id,
    pub scope: Scope,
    pub name: String,
    pub version: u64,
    pub supersedes: Option<Id>,
    pub content_hash: String,
    #[serde(flatten)]
    pub refs: References,
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
            "model_provider",
            "model_catalog",
            "runtime_resource",
            "orchestration_policy",
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
        .contains(&self.r#type.as_str())
    }
    /// Column stores nest references under a dedicated key instead of the wire flattening.
    pub fn to_storage(&self) -> Value {
        let mut value = serde_json::to_value(self).expect("serializable configuration");
        let object = value.as_object_mut().expect("record is an object");
        let mut refs = serde_json::Map::new();
        for key in reference_fields(&self.r#type) {
            if let Some(reference) = object.remove(*key) {
                refs.insert((*key).into(), reference);
            }
        }
        object.insert("refs".into(), Value::Object(refs));
        value
    }
    pub fn from_storage(value: Value) -> Result<Self> {
        let mut value = value;
        let object = value
            .as_object_mut()
            .ok_or_else(|| err("invalid_configuration", "存储记录必须为对象"))?;
        if let Some(Value::Object(refs)) = object.remove("refs") {
            for (key, reference) in refs {
                object.insert(key, reference);
            }
        }
        serde_json::from_value(value).map_err(|e| {
            err(
                "invalid_configuration",
                format!("存储记录反序列化失败: {e}"),
            )
        })
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
            r.r#type == "workflow_instance"
                && r.data["paused"] == true
                && r.data["owner_id"] == owner
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
            if r.r#type == "delivery_type" && !before.governance.iter().any(|old| old == r) {
                self.validate_delivery_specs(&r.refs, &r.decode()?, &r.scope)?;
            }
            if r.is_definition() || r.r#type == "project_configuration" {
                require(
                    versions.insert((&r.r#type, &r.definition_id, r.version)),
                    "配置修订版本重复",
                )?;
            }
            if let Some(old) = before.governance.iter().find(|x| x.id == r.id) {
                require(
                    old.r#type == r.r#type
                        && old.definition_id == r.definition_id
                        && old.scope == r.scope,
                    "配置身份和 Scope 不可更改",
                )?;
                if old.immutable {
                    require(old == r, "已发布配置和锁不可更改")?;
                }
                if r.r#type == "agent_instance" {
                    require(
                        old.data["agent_revision_id"] == r.data["agent_revision_id"]
                            && old.data["scope"] == r.data["scope"],
                        "Agent 的组合和范围不可更改",
                    )?;
                }
            }
            if r.r#type == "standard" {
                require(!r.scope.is_feature(), "Standard 不具有 Feature 归属")?;
                r.decode::<ConfiguredStandard>()?.validate()?;
                if !before.governance.iter().any(|old| old.id == r.id) {
                    standard_snapshot(self, &r.id, &r.scope)?;
                }
            }
            require(
                r.content_hash == content_digest(&r.refs, &r.data),
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
