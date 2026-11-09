//! Versioned configuration and executable workflow contracts. No I/O.
use crate::*;
use serde_json::json;
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Compatibility metadata only: experience is restored context, not an executable Tool.
    pub fn is_legacy_experience_tool(&self) -> bool {
        self.kind == "tool" && self.data["implementation"] == "experience.lookup"
    }
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
            "role",
            "capability",
            "integration",
            "integration_binding",
            "sandbox",
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
    pub qualified_name: String,
    pub implementation: String,
    pub input_schema: Value,
    pub output_schema: Value,
    #[serde(default)]
    pub capabilities: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentComposition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_selection: Option<AgentPermissionSelection>,
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
    #[serde(default)]
    pub permissions: Vec<Id>,
    #[serde(default)]
    pub approval: ApprovalPolicy,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleDefinition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<DefinitionVisibility>,
    pub execution_role: AgentRole,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvanceMode {
    #[default]
    Automatic,
    HumanConfirmation,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowCondition {
    #[default]
    Always,
    All {
        conditions: Vec<WorkflowCondition>,
    },
    Any {
        conditions: Vec<WorkflowCondition>,
    },
    OutputPresent {
        step: String,
    },
    Accepted {
        kind: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStage {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub key: String,
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowSlot {
    pub role: AgentRole,
    #[serde(default)]
    pub agent_ref: Option<Id>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InteractionContract {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub required_topics: Vec<String>,
    #[serde(default)]
    pub output_kind: String,
    #[serde(default)]
    pub max_turns: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowPortReference {
    pub step: String,
    pub port: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStep {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::agent_task::supplied_task"
    )]
    pub task_instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_ref: Option<Id>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spec_refs: Vec<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subworkflow_ref: Option<Id>,
    pub key: String,
    pub name: String,
    pub stage: String,
    pub kind: String,
    #[serde(default = "one")]
    pub contract_version: u64,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub participant_slot: Option<String>,
    #[serde(default)]
    pub inputs: BTreeMap<String, WorkflowPortReference>,
    #[serde(default)]
    pub interaction: InteractionContract,
    #[serde(default)]
    pub advance_mode: AdvanceMode,
    #[serde(default)]
    pub condition: WorkflowCondition,
    #[serde(default)]
    pub parameters: BTreeMap<String, Value>,
    #[serde(default)]
    pub execution_recipe: Vec<WorkflowRunNode>,
    #[serde(default)]
    pub rework_scope: Vec<String>,
    #[serde(default = "three")]
    pub retry_limit: u64,
}
fn one() -> u64 {
    1
}
fn three() -> u64 {
    3
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowRunNode {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::agent_task::supplied_task"
    )]
    pub task_instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_ref: Option<Id>,
    pub key: String,
    pub role: AgentRole,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub writes: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDocument {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_ports: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub output_ports: BTreeMap<String, WorkflowPortReference>,
    pub schema_version: u64,
    pub process_kind: String,
    pub stages: Vec<WorkflowStage>,
    #[serde(default)]
    pub binding_slots: BTreeMap<String, WorkflowSlot>,
    pub steps: Vec<WorkflowStep>,
    #[serde(default)]
    pub subworkflow_refs: BTreeMap<String, Id>,
}
pub fn workflow_action_catalog() -> Value {
    json!([
        "agent.interact",
        "proposal.adopt",
        "attempt.execute",
        "delivery.review",
        "merge.prepare",
        "merge.review",
        "merge.apply",
        "snapshot.capture",
        "resource.cleanup",
        "tasks.foreach",
        "branch",
        "join",
        "workflow.call"
    ])
}
pub fn validate_workflow(doc: &WorkflowDocument) -> Result<Vec<String>> {
    if doc.schema_version == 3 {
        return validate_workflow_v3(doc);
    }
    validate_workflow_tasks(doc)?;
    if doc.schema_version == 2 {
        return crate::configuration::validate_workflow_v2(doc);
    }
    require(doc.schema_version == 1, "不支持的 Workflow Schema")?;
    require(
        ["project_intake", "feature", "task"].contains(&doc.process_kind.as_str()),
        "无效的流程用途",
    )?;
    require(
        !doc.steps.is_empty() && doc.steps.len() <= 200,
        "流程必须包含 1—200 个步骤",
    )?;
    let canonical = [
        ("analyst", AgentRole::Analyst),
        ("architect", AgentRole::Architect),
        ("developer", AgentRole::Developer),
        ("tester", AgentRole::Tester),
        ("reviewer", AgentRole::Reviewer),
    ];
    for (key, slot) in &doc.binding_slots {
        require(
            canonical
                .iter()
                .any(|(name, role)| *name == key && *role == slot.role),
            format!("{key}: 当前编译器要求五种规范职责 slot，不支持别名或职责替换"),
        )?;
    }
    if doc.process_kind != "project_intake" {
        for (key, _) in canonical {
            require(
                doc.binding_slots.contains_key(key),
                format!("缺少工程职责 {key}"),
            )?;
        }
    }
    let stages: BTreeSet<_> = doc.stages.iter().map(|x| x.key.as_str()).collect();
    require(stages.len() == doc.stages.len(), "阶段 key 重复")?;
    let keys: BTreeSet<_> = doc.steps.iter().map(|x| x.key.as_str()).collect();
    require(keys.len() == doc.steps.len(), "步骤 key 重复")?;
    let catalog = workflow_action_catalog();
    for step in &doc.steps {
        require(
            !step.key.is_empty()
                && step.key.len() <= 80
                && step
                    .key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c)),
            "步骤 key 格式错误",
        )?;
        require(
            stages.contains(step.stage.as_str()),
            format!("{}: 阶段不存在", step.key),
        )?;
        require(
            step.contract_version == 1 && catalog.as_array().unwrap().contains(&json!(step.kind)),
            format!("{}: 未知动作或契约版本", step.key),
        )?;
        require(
            step.dependencies
                .iter()
                .all(|x| keys.contains(x.as_str()) && x != &step.key),
            format!("{}: 前置步骤不存在或自引用", step.key),
        )?;
        if let Some(slot) = &step.participant_slot {
            require(
                doc.binding_slots.contains_key(slot),
                format!("{}: 角色 slot 不存在", step.key),
            )?;
        }
        let expected = match step.kind.as_str() {
            "agent.interact" => match step.interaction.output_kind.as_str() {
                "requirement" | "feature" => Some("analyst"),
                "design" | "task_plan" => Some("architect"),
                "test_plan" => Some("tester"),
                _ => None,
            },
            "delivery.review" | "merge.review" => Some("reviewer"),
            "merge.prepare" => Some("tester"),
            "attempt.execute" => match step.parameters.get("phase").and_then(Value::as_str) {
                Some("requirement_definition") => Some("analyst"),
                Some("solution_planning") => Some("architect"),
                Some("implementation") => Some("developer"),
                Some("closure") => Some("tester"),
                _ => {
                    return Err(err(
                        "invalid_workflow",
                        format!("{}: 不支持的正式阶段", step.key),
                    ));
                }
            },
            _ => None,
        };
        if let Some(expected) = expected {
            require(
                step.participant_slot.as_deref() == Some(expected),
                format!("{}: 此动作必须使用 {expected} 完整 Agent", step.key),
            )?;
        }
        require(step.retry_limit <= 10, "自动重试上限不能大于 10")?;
        require(
            step.rework_scope.iter().all(|x| keys.contains(x.as_str())),
            "返工范围包含未知步骤",
        )?;
        validate_condition(&step.condition, &keys)?;
        if step.kind == "agent.interact" {
            require(
                step.participant_slot.is_some() && !step.interaction.goal.trim().is_empty(),
                format!("{}: 讨论需要角色和目标", step.key),
            )?;
        }
        if step.kind == "agent.interact" {
            require(
                (1..=100).contains(&step.interaction.max_turns)
                    && ["requirement", "feature", "design", "task_plan", "test_plan"]
                        .contains(&step.interaction.output_kind.as_str()),
                format!("{}: 讨论轮数或输出契约不支持", step.key),
            )?;
        }
        if !step.execution_recipe.is_empty() {
            let nodes: Vec<_> = step
                .execution_recipe
                .iter()
                .map(|n| (n.key.clone(), n.dependencies.clone()))
                .collect();
            valid_dag(&nodes)?;
            for node in &step.execution_recipe {
                require(
                    ["test_author", "red", "implement", "green", "docs", "verify"]
                        .contains(&node.key.as_str()),
                    format!("{}: 未知执行节点 {}", step.key, node.key),
                )?;
                require(
                    match node.key.as_str() {
                        "implement" => node.role == AgentRole::Developer && node.writes,
                        "test_author" => node.role == AgentRole::Tester && node.writes,
                        "docs" => node.role == AgentRole::Developer && !node.writes,
                        _ => node.role == AgentRole::Tester && !node.writes,
                    },
                    format!("{}: 节点职责或写权限不兼容", node.key),
                )?;
            }
        }
    }
    let order = topological(
        &doc.steps
            .iter()
            .map(|s| (s.key.clone(), s.dependencies.clone()))
            .collect::<Vec<_>>(),
    )?;
    if doc.process_kind == "feature" {
        for kind in [
            "agent.interact",
            "proposal.adopt",
            "attempt.execute",
            "delivery.review",
            "tasks.foreach",
            "merge.prepare",
            "merge.apply",
            "snapshot.capture",
        ] {
            require(
                doc.steps.iter().any(|s| s.kind == kind),
                format!("Feature 流程缺少必需动作 {kind}"),
            )?;
        }
    }
    if doc.process_kind == "task" {
        let recipes: Vec<_> = doc.steps.iter().flat_map(|s| &s.execution_recipe).collect();
        if !recipes.is_empty() {
            for node in ["test_author", "red", "implement", "green", "docs", "verify"] {
                require(
                    recipes.iter().any(|n| n.key == node),
                    format!("Task 配方缺少 {node}"),
                )?;
            }
            let implement = recipes.iter().find(|n| n.key == "implement").unwrap();
            require(
                implement.dependencies.iter().any(|d| d == "red"),
                "实现必须依赖 Red",
            )?;
            let verify = recipes.iter().find(|n| n.key == "verify").unwrap();
            require(
                ["green", "docs"]
                    .iter()
                    .all(|key| verify.dependencies.iter().any(|d| d == key)),
                "verify 必须依赖 Green 和文档",
            )?;
            let green = recipes.iter().find(|n| n.key == "green").unwrap();
            require(
                green.dependencies.iter().any(|d| d == "implement"),
                "Green 必须依赖实现",
            )?;
        }
    }
    crate::configuration::validate_workflow_ports(doc)?;
    validate_workflow_semantics(doc)?;
    Ok(order)
}
pub(crate) fn validate_condition(c: &WorkflowCondition, keys: &BTreeSet<&str>) -> Result<()> {
    match c {
        WorkflowCondition::All { conditions } | WorkflowCondition::Any { conditions } => {
            require(
                !conditions.is_empty() && conditions.len() <= 20,
                "条件组合为空或过大",
            )?;
            for c in conditions {
                validate_condition(c, keys)?;
            }
        }
        WorkflowCondition::OutputPresent { step } => {
            require(keys.contains(step.as_str()), "条件引用未知步骤")?
        }
        WorkflowCondition::Accepted { kind } => require(
            [
                "requirement",
                "plan",
                "implementation",
                "closure",
                "merge_candidate",
            ]
            .contains(&kind.as_str()),
            "未知交付条件",
        )?,
        WorkflowCondition::Always => {}
    }
    Ok(())
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureConfiguration {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub integration_locks: BTreeMap<Id, IntegrationLock>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub specification_snapshot: Vec<SpecificationBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_check: Option<BaselineCheck>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub role_agent_bindings: BTreeMap<Id, Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults_bundle_ref: Option<Id>,
    pub project_configuration_id: Id,
    pub workflow_ref: Id,
    pub standard_ref: Id,
    pub agent_refs: BTreeMap<String, Id>,
    pub definition_hashes: BTreeMap<String, String>,
    pub plan: WorkflowDocument,
    pub task_plan: WorkflowDocument,
    pub compiler_version: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStepRuntime {
    pub id: Id,
    pub key: String,
    pub round: u64,
    pub status: String,
    pub input_hash: String,
    #[serde(default)]
    pub outputs: BTreeMap<String, Id>,
    #[serde(default)]
    pub reason: String,
    pub revision: u64,
    #[serde(default)]
    pub confirmation: Option<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRuntime {
    pub owner_id: Id,
    pub feature_id: Id,
    pub task_id: Option<Id>,
    pub parent_id: Option<Id>,
    pub configuration_lock_id: Id,
    pub workflow_ref: Id,
    pub status: String,
    pub paused: bool,
    pub dirty: bool,
    pub steps: Vec<WorkflowStepRuntime>,
    #[serde(default)]
    pub task_members: Vec<Id>,
    #[serde(default)]
    pub task_plan_revision: Option<Id>,
    pub generation: u64,
}
impl EngineeringState {
    /// The accepted TaskPlan generation fixes closure membership; unrelated historical Tasks are excluded.
    pub fn required_tasks<'a>(&'a self, feature: &str) -> Vec<&'a Task> {
        if self.feature_configuration(feature).is_some() {
            let members = self
                .governance
                .iter()
                .find(|r| r.kind == "workflow_instance" && r.definition_id == feature)
                .and_then(|r| r.decode::<WorkflowRuntime>().ok())
                .map(|r| r.task_members)
                .unwrap_or_default();
            self.tasks
                .iter()
                .filter(|t| members.contains(&t.id))
                .collect()
        } else {
            self.tasks
                .iter()
                .filter(|t| t.feature_id == feature)
                .collect()
        }
    }
    pub fn workflow_paused(&self, owner: &str) -> bool {
        let feature = self
            .tasks
            .iter()
            .find(|t| t.id == owner)
            .map_or(owner, |t| t.feature_id.as_str());
        self.governance.iter().any(|r| {
            r.kind == "workflow_instance"
                && r.data["paused"] == true
                && (r.definition_id == owner || r.definition_id == feature)
        })
    }
    pub fn attempt_inputs_current(&self, a: &Attempt) -> bool {
        if self.feature_configuration(&a.feature_id).is_none() {
            return self
                .feature(&a.feature_id)
                .is_ok_and(|f| f.input_version == a.input_version);
        }
        if a.phase != "requirement_definition" {
            if let Some(plan) = self.documents.iter().rev().find(|d| {
                self.document_in_feature(d, &a.feature_id)
                    && d.task_id.is_none()
                    && d.kind == "doc"
                    && d.title == "Structured Task Plan"
            }) {
                if !a.documents.contains(&plan.id) {
                    return false;
                }
            }
        }
        let kinds: &[&str] = if a.phase == "requirement_definition" {
            &["requirement"]
        } else {
            &["requirement", "design", "test"]
        };
        kinds.iter().all(|kind| {
            let matching: Vec<_> = self
                .documents
                .iter()
                .filter(|d| {
                    self.document_in_feature(d, &a.feature_id)
                        && d.kind == *kind
                        && (if *kind == "test" && a.task_id.is_some() {
                            d.task_id == a.task_id
                        } else {
                            d.task_id.is_none()
                        })
                })
                .collect();
            matching
                .iter()
                .filter(|d| {
                    !matching
                        .iter()
                        .any(|new| new.supersedes.as_ref() == Some(&d.id))
                })
                .all(|d| a.documents.contains(&d.id) || a.outputs.contains(&d.id))
        })
    }
    pub fn delivery_inputs_current(&self, d: &Delivery) -> bool {
        if self.feature_configuration(&d.feature_id).is_none() {
            return self
                .feature(&d.feature_id)
                .is_ok_and(|f| f.input_version == d.input_version);
        }
        if let Some(a) = &d.attempt_id {
            return self
                .attempt(a)
                .is_ok_and(|a| self.attempt_inputs_current(a));
        }
        d.merge_id
            .as_ref()
            .and_then(|id| self.merge(id).ok())
            .and_then(|m| self.delivery(&m.source_delivery).ok())
            .is_some_and(|source| self.delivery_inputs_current(source))
    }
    pub fn configuration(&self, id: &str) -> Result<&ConfigurationRecord> {
        self.governance
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| err("not_found", format!("配置不存在: {id}")))
    }
    pub fn feature_configuration(&self, feature: &str) -> Option<FeatureConfiguration> {
        self.governance
            .iter()
            .find(|r| r.kind == "feature_lock" && r.definition_id == feature)
            .and_then(|r| r.decode().ok())
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
                    require(
                        serde_json::to_value(old).unwrap() == serde_json::to_value(r).unwrap(),
                        "已发布配置和锁不可更改",
                    )?;
                }
                if r.kind == "agent_instance" {
                    require(
                        old.data["agent_revision_id"] == r.data["agent_revision_id"]
                            && old.data["scope"] == r.data["scope"],
                        "AgentID 的组合和范围不可更改",
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn definition_visibility_does_not_determine_instance_scope() {
        let p = Scope::Project {
            project_id: "p".into(),
        };
        assert!(Scope::Global.visible_from(&p));
        assert!(!p.visible_from(&Scope::Global));
        assert!(!p.visible_from(&Scope::Project {
            project_id: "q".into()
        }));
    }
    #[test]
    fn workflow_rejects_cycles_and_missing_dependencies() {
        assert!(
            topological(&[
                ("a".into(), vec!["b".into()]),
                ("b".into(), vec!["a".into()])
            ])
            .is_err()
        );
        assert!(topological(&[("a".into(), vec!["missing".into()])]).is_err());
    }
    #[test]
    fn hashes_distinguish_model_parameters() {
        assert_ne!(
            configuration_hash(&json!({"model":"a","effort":"high"})),
            configuration_hash(&json!({"model":"a","effort":"low"}))
        );
    }
    #[test]
    fn published_action_schema_matches_the_compiler_catalog() {
        let rpc: Value =
            serde_json::from_str(include_str!("../../../schemas/rpc.schema.json")).unwrap();
        let action: Value =
            serde_json::from_str(include_str!("../../../schemas/workflow-action.schema.json"))
                .unwrap();
        assert_eq!(
            rpc["$defs"]["WorkflowStep"]["properties"]["kind"]["enum"],
            workflow_action_catalog()
        );
        assert_eq!(
            action["properties"]["kind"]["enum"],
            workflow_action_catalog()
        );
        assert_eq!(
            action["properties"]["contract_version"]["enum"],
            json!([1, 2])
        );
        for name in [
            "WorkflowDocument",
            "AgentComposition",
            "ModelDefinition",
            "ToolDefinition",
        ] {
            assert!(rpc["$defs"][name].is_object(), "missing {name}");
        }
    }
}

/// Engineering gates are applied to the expanded executable graph for both contracts.
pub fn validate_workflow_semantics(doc: &WorkflowDocument) -> Result<()> {
    if doc.process_kind == "feature" {
        for kind in [
            "agent.interact",
            "proposal.adopt",
            "attempt.execute",
            "delivery.review",
            "tasks.foreach",
            "merge.prepare",
            "merge.apply",
            "snapshot.capture",
        ] {
            require(
                doc.steps.iter().any(|s| s.kind == kind),
                format!("Feature 流程缺少必需动作 {kind}"),
            )?;
        }
    }
    if doc.process_kind == "task" {
        require(
            doc.steps.iter().any(|s| {
                s.kind == "attempt.execute"
                    && s.parameters.get("phase").and_then(Value::as_str) == Some("implementation")
            }),
            "Task 流程缺少正式实现步骤",
        )?;
    }

    let ancestors = |key: &str| -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        let mut queue = vec![key.to_owned()];
        while let Some(key) = queue.pop() {
            if let Some(step) = doc.steps.iter().find(|s| s.key == key) {
                for dep in &step.dependencies {
                    if found.insert(dep.clone()) {
                        queue.push(dep.clone());
                    }
                }
            }
        }
        found
    };
    let has_ancestor = |step: &WorkflowStep, kind: &str, phase: Option<&str>| {
        let all = ancestors(&step.key);
        doc.steps.iter().any(|candidate| {
            all.contains(&candidate.key)
                && candidate.kind == kind
                && phase.is_none_or(|p| {
                    candidate.parameters.get("phase").and_then(Value::as_str) == Some(p)
                })
        })
    };
    for step in &doc.steps {
        let all = ancestors(&step.key);
        if let Some(input) = step.inputs.get("merge") {
            if step.kind == "merge.apply" {
                require(
                    doc.steps
                        .iter()
                        .any(|s| s.key == input.step && s.kind == "merge.review"),
                    "合入输入必须来自已完成的候选评审步骤",
                )?;
            }
        }
        let reviewed_phase = |phase: &str| {
            doc.steps.iter().any(|review| {
                review.kind == "delivery.review"
                    && all.contains(&review.key)
                    && doc.steps.iter().any(|producer| {
                        ancestors(&review.key).contains(&producer.key)
                            && producer.kind == "attempt.execute"
                            && producer.parameters.get("phase").and_then(Value::as_str)
                                == Some(phase)
                    })
            })
        };
        if doc.process_kind == "feature" {
            if step.kind == "agent.interact"
                && ["design", "task_plan", "test_plan"]
                    .contains(&step.interaction.output_kind.as_str())
            {
                require(
                    reviewed_phase("requirement_definition"),
                    format!("{}: 方案讨论必须在需求交付评审之后", step.key),
                )?;
            }
            if step.kind == "tasks.foreach" {
                require(
                    reviewed_phase("solution_planning"),
                    "展开 Tasks 必须在方案交付评审之后",
                )?;
            }
            if step.kind == "attempt.execute"
                && step.parameters.get("phase").and_then(Value::as_str) == Some("closure")
            {
                require(
                    has_ancestor(step, "tasks.foreach", None),
                    "Feature 收尾必须在固定 Task 集合汇合之后",
                )?;
            }
            if step.kind == "merge.prepare" {
                require(
                    reviewed_phase("closure"),
                    "主干候选必须在 Feature 收尾评审之后",
                )?;
            }
        } else if doc.process_kind == "task" && step.kind == "merge.prepare" {
            require(
                reviewed_phase("implementation"),
                "Task 候选必须在实现交付评审之后",
            )?;
        }
        if step.kind == "merge.apply" {
            require(
                has_ancestor(step, "merge.review", None),
                format!("{}: 合入必须依赖候选评审", step.key),
            )?;
        }
        if step.kind == "merge.review" {
            require(
                has_ancestor(step, "merge.prepare", None),
                format!("{}: 评审必须依赖冻结候选", step.key),
            )?;
        }
        if step.kind == "merge.prepare" {
            require(
                has_ancestor(step, "delivery.review", None),
                format!("{}: 候选必须依赖交付评审", step.key),
            )?;
        }
        if step.kind == "attempt.execute"
            && step.parameters.get("phase").and_then(Value::as_str) == Some("solution_planning")
        {
            require(
                has_ancestor(step, "attempt.execute", Some("requirement_definition"))
                    && has_ancestor(step, "delivery.review", None),
                "方案规划必须在需求交付评审之后",
            )?;
        }
        if step.kind == "tasks.foreach" {
            require(
                has_ancestor(step, "attempt.execute", Some("solution_planning")),
                "Task 展开必须依赖方案规划",
            )?;
        }
        if step.kind == "branch" {
            require(
                doc.steps.iter().any(|s| s.dependencies.contains(&step.key)),
                format!("{}: 分支没有出口", step.key),
            )?;
        }
    }
    if doc.process_kind == "feature" {
        for phase in ["requirement_definition", "solution_planning", "closure"] {
            require(
                doc.steps.iter().any(|s| {
                    s.kind == "attempt.execute"
                        && s.parameters.get("phase").and_then(Value::as_str) == Some(phase)
                }),
                format!("缺少正式阶段 {phase}"),
            )?;
        }
    }
    Ok(())
}
