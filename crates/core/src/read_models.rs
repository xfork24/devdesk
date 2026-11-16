//! Bounded display projections. These values never authorize workflow actions.
use devdesk_domain::{Result, err};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone)]
pub enum ControllerSelector {
    Id(String),
    Root(String),
    Owner(String),
}
#[derive(Debug, Clone)]
pub struct ControllerQuery {
    pub selector: ControllerSelector,
    pub workspace_only: Option<bool>,
    pub decision_before: Option<u64>,
}
pub fn identifier(params: &Value, key: &str) -> Result<String> {
    params[key]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 1024)
        .map(str::to_owned)
        .ok_or_else(|| err("invalid_params", format!("需要有效的 {key}")))
}
pub fn fields(params: &Value, allowed: &[&str]) -> Result<()> {
    if !params
        .as_object()
        .is_some_and(|fields| fields.keys().all(|key| allowed.contains(&key.as_str())))
    {
        return Err(err("invalid_params", "摘要请求包含未知字段"));
    }
    Ok(())
}
impl ControllerQuery {
    pub fn parse(params: &Value) -> Result<Self> {
        fields(
            params,
            &[
                "project_id",
                "id",
                "root_instance_id",
                "owner_id",
                "workspace_only",
                "decision_before",
            ],
        )?;
        let mut selectors = vec![];
        for (key, kind) in [("id", 0), ("root_instance_id", 1), ("owner_id", 2)] {
            if params.get(key).is_some() {
                let id = identifier(params, key)?;
                selectors.push(match kind {
                    0 => ControllerSelector::Id(id),
                    1 => ControllerSelector::Root(id),
                    _ => ControllerSelector::Owner(id),
                });
            }
        }
        if selectors.len() != 1 {
            return Err(err(
                "invalid_params",
                "经理摘要需要唯一的对象、根实例或工作归属",
            ));
        }
        let workspace_only = params
            .get("workspace_only")
            .map(|value| {
                value
                    .as_bool()
                    .ok_or_else(|| err("invalid_params", "workspace_only 需要布尔值"))
            })
            .transpose()?;
        let decision_before = params
            .get("decision_before")
            .map(|value| {
                value
                    .as_u64()
                    .filter(|n| *n > 0 && *n <= i64::MAX as u64)
                    .ok_or_else(|| err("invalid_params", "无效的待决定项游标"))
            })
            .transpose()?;
        Ok(Self {
            selector: selectors.remove(0),
            workspace_only,
            decision_before,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct RuntimeSummary<T> {
    pub id: String,
    pub name: String,
    pub revision: u64,
    pub data: T,
}
#[derive(Debug, Serialize)]
pub struct ControllerState {
    pub project_id: String,
    pub owner_id: String,
    pub root_instance_id: Option<String>,
    pub role_ref: String,
    pub agent_revision_id: String,
    pub harness_permission_ref: String,
    pub session_id: Option<String>,
    pub epoch: u64,
    pub status: String,
    pub reason: String,
    pub reason_truncated: bool,
}
#[derive(Debug, Serialize)]
pub struct MessagePreview {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub sequence: u64,
    pub content_excerpt: String,
    pub truncated: bool,
}
#[derive(Debug, Serialize)]
pub struct CallSummary {
    pub id: String,
    pub method: String,
    pub status: String,
    pub receipt_status: Option<String>,
    pub error: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct DecisionState {
    pub status: String,
    pub plan_revision_id: String,
    pub operation: String,
}
#[derive(Debug, Serialize)]
pub struct MoreControllerItems {
    pub messages: bool,
    pub calls: bool,
    pub decisions: bool,
}
#[derive(Debug, Serialize)]
pub struct OrchestrationSummary {
    pub project_id: String,
    pub cursor: u64,
    pub record: Option<RuntimeSummary<ControllerState>>,
    pub messages: Vec<MessagePreview>,
    pub calls: Vec<CallSummary>,
    pub decisions: Vec<RuntimeSummary<DecisionState>>,
    pub more: MoreControllerItems,
    pub next_decision_before: Option<u64>,
}
#[derive(Debug, Serialize)]
pub struct DriverState {
    pub mode: String,
    pub epoch: u64,
    pub reason: String,
    pub reason_truncated: bool,
}
#[derive(Debug, Serialize)]
pub struct WaitingStep {
    pub instance_id: String,
    pub owner_id: String,
    pub execution_id: String,
    pub step: String,
    pub path: String,
    pub status: String,
    pub reason: String,
    pub reason_truncated: bool,
}
#[derive(Debug, Serialize)]
pub struct WorkflowDriverSummary {
    pub project_id: String,
    pub cursor: u64,
    pub root_instance_id: String,
    pub instance_revision: u64,
    pub driver: Option<RuntimeSummary<DriverState>>,
    pub waiting: Vec<WaitingStep>,
    pub waiting_more: bool,
    pub observed_only: bool,
}
#[derive(Debug, Serialize)]
pub struct DecisionRequest {
    pub status: String,
    pub plan_revision_id: String,
    /// Exact persisted JSON, including integers wider than browser Number precision.
    pub request_json: String,
}
pub type DecisionDetail = RuntimeSummary<DecisionRequest>;

#[derive(Debug, Serialize, Deserialize)]
pub struct PlanSelectionSummary {
    pub standard_ref: Option<String>,
    pub harness_permission_ref: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct InstanceManagementState {
    pub project_id: String,
    pub owner_id: String,
    pub root_instance_id: String,
    pub parent_execution_id: Option<String>,
    pub workflow_ref: Option<String>,
    pub active_plan_revision_id: Option<String>,
    pub control_version: u64,
    pub status: String,
    pub paused: bool,
    pub cancel_requested: bool,
    pub plan: PlanSelectionSummary,
    pub role_agent_bindings: std::collections::BTreeMap<String, String>,
    pub configuration_truncated: bool,
}
#[derive(Debug, Serialize)]
pub struct WorkflowChildSummary {
    pub id: String,
    pub name: String,
    pub revision: u64,
}
#[derive(Debug, Serialize)]
pub struct WorkflowManagementSummary {
    pub project_id: String,
    pub cursor: u64,
    pub record: RuntimeSummary<InstanceManagementState>,
    pub children: Vec<WorkflowChildSummary>,
    pub children_more: bool,
    pub next_child_offset: Option<u64>,
    pub observed_only: bool,
}
