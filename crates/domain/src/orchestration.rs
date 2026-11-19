//! Harness orchestration authority and immutable execution decisions.
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformPermission {
    #[serde(default)]
    pub allowed_operations: BTreeSet<String>,
    #[serde(default)]
    pub human_operations: BTreeSet<String>,
    #[serde(default)]
    pub waivable_requirements: BTreeSet<String>,
    #[serde(default)]
    pub allow_task_revision: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentSchedulingBudget {
    /// Legacy configuration metadata. Manager wakeups are no longer limited.
    pub max_turns_per_wakeup: u64,
    /// Legacy configuration metadata. Manager tool calls are no longer limited.
    pub max_tool_calls_per_turn: u64,
    pub timeout_seconds: u64,
}
impl Default for AgentSchedulingBudget {
    fn default() -> Self {
        Self {
            max_turns_per_wakeup: 8,
            max_tool_calls_per_turn: 64,
            timeout_seconds: 600,
        }
    }
}
pub fn profile_control_policy(
    state: &EngineeringState,
    reference: &str,
    budget: Option<&AgentSchedulingBudget>,
) -> Result<WorkflowControlPolicy> {
    let profile = state.configuration(reference)?.permission_profile()?;
    require(
        profile.target_kind == PermissionTargetKind::Harness,
        "平台操作需要 Harness Profile",
    )?;
    let platform = profile.policy.platform.unwrap_or_default();
    let budget = budget.cloned().unwrap_or_default();
    let policy = WorkflowControlPolicy {
        allowed_operations: platform.allowed_operations,
        human_operations: platform.human_operations,
        waivable_requirements: platform.waivable_requirements,
        allow_task_revision: platform.allow_task_revision,
        max_turns_per_wakeup: budget.max_turns_per_wakeup,
        max_tool_calls_per_turn: budget.max_tool_calls_per_turn,
        timeout_seconds: budget.timeout_seconds,
    };
    policy.validate()?;
    Ok(policy)
}

/// Renderer identity recorded on orchestration wakeups. Metadata only.
pub const ORCHESTRATION_RENDERER: &str = "orchestration-task/v2";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowControlPolicy {
    pub allowed_operations: BTreeSet<String>,
    #[serde(default)]
    pub human_operations: BTreeSet<String>,
    #[serde(default)]
    pub waivable_requirements: BTreeSet<String>,
    #[serde(default)]
    pub allow_task_revision: bool,
    #[serde(default)]
    pub max_turns_per_wakeup: u64,
    #[serde(default)]
    pub max_tool_calls_per_turn: u64,
    pub timeout_seconds: u64,
}
impl Default for WorkflowControlPolicy {
    fn default() -> Self {
        Self {
            allowed_operations: [
                "start",
                "inspect",
                "dispatch",
                "accept",
                "input",
                "retry",
                "rework",
                "stop",
                "pause",
                "resume",
                "cancel",
                "preview",
                "apply",
                "import",
                "request_decision",
                "reconcile",
                "complete",
                "segment",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            human_operations: BTreeSet::new(),
            waivable_requirements: [
                "completion",
                "confirmation",
                "tests",
                "independence",
                "coverage",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            allow_task_revision: false,
            max_turns_per_wakeup: 8,
            max_tool_calls_per_turn: 64,
            timeout_seconds: 600,
        }
    }
}
impl WorkflowControlPolicy {
    pub fn validate(&self) -> Result<()> {
        require(
            self.timeout_seconds > 0 && self.timeout_seconds <= 3600,
            "经理单次调用的故障检测超时超出允许范围",
        )
    }
    pub fn permits(&self, operation: &str) -> Result<()> {
        require(
            self.allowed_operations.contains(operation),
            format!("编排策略未允许 {operation}"),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowController {
    #[serde(default)]
    pub workspace_only: bool,
    pub project_id: Id,
    pub owner_id: Id,
    pub root_instance_id: Option<Id>,
    pub agent_revision_id: Id,
    pub role_ref: Id,
    pub harness_permission_ref: Id,
    pub policy: WorkflowControlPolicy,
    pub policy_digest: String,
    pub session_id: Option<Id>,
    #[serde(default)]
    pub predecessor_session_id: Option<Id>,
    #[serde(default)]
    pub snapshot_id: Option<Id>,
    pub epoch: u64,
    pub status: String,
    pub event_cursor: u64,
    pub wakeup_turns: u64,
    #[serde(default)]
    pub source_cursor: u64,
    #[serde(default)]
    pub acknowledged_cursor: u64,
    #[serde(default)]
    pub delivery_id: String,
    #[serde(default)]
    pub run_state: String,
    #[serde(default)]
    pub checkpoint: serde_json::Value,
    #[serde(default)]
    pub wait_deadline: Option<u64>,
    #[serde(default)]
    pub wait_turn_id: Option<Id>,
    #[serde(default)]
    pub bootstrap_epoch: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationWakeup {
    pub id: Id,
    pub controller_id: Id,
    pub event_cursor: u64,
    pub status: String,
    pub turn_id: Option<Id>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationLease {
    pub id: Id,
    pub controller_id: Id,
    pub owner_id: Id,
    pub root_instance_id: Option<Id>,
    pub epoch: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowActor {
    Human,
    Controller {
        controller_id: Id,
        session_id: Id,
        turn_id: Id,
        epoch: u64,
    },
    Driver {
        driver_id: Id,
        epoch: u64,
    },
    Segment {
        authorization_id: Id,
    },
    /// Kernel observation/continuation metadata. This origin cannot sign a
    /// business decision or invoke public workflow controls.
    Coordinator {
        source_id: Id,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDecision {
    pub root_instance_id: Id,
    pub instance_id: Id,
    pub plan_revision_id: Id,
    pub execution_id: Option<Id>,
    pub actor: WorkflowActor,
    pub operation: String,
    pub reason: String,
    pub input_digest: String,
    pub outputs: BTreeMap<String, Id>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypedMaterial {
    pub project_id: Id,
    pub root_instance_id: Id,
    pub delivery_ref: Id,
    pub value: serde_json::Value,
    pub value_digest: String,
    pub artifact_id: Id,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowActionPermit {
    pub instance_id: Id,
    pub plan_revision_id: Id,
    pub candidate_digest: String,
    pub target_revision: String,
    pub requirements: BTreeMap<String, String>,
    pub decision_refs: Vec<Id>,
}

/// Stable authoring identity. Published node IDs survive renames and runtime revisions.
pub fn workflow_node_id(definition: &str, kind: &str, key: &str) -> Id {
    format!(
        "node-{}",
        &configuration_hash(&(definition, kind, key))[..32]
    )
}
