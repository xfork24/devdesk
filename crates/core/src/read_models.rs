//! Bounded display projections. These values never authorize workflow actions.
use devdesk_domain::{Result, err};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowDetailView {
    Status,
    Detail,
    History,
}

#[derive(Debug, Clone, Copy)]
pub enum WorkflowReadingView {
    Materials,
    Contexts,
    Executions,
    Record,
}

#[derive(Debug, Clone)]
pub struct WorkflowDetailQuery {
    pub id: Option<String>,
    pub owner_id: Option<String>,
    pub view: WorkflowDetailView,
    pub history_before: Option<u64>,
    pub child_offset: u64,
    pub step_offset: u64,
    pub step_limit: u64,
    pub focus_instance_id: Option<String>,
    pub focus_execution_id: Option<String>,
}
impl WorkflowDetailQuery {
    pub fn parse(params: &Value) -> Result<Self> {
        fields(
            params,
            &[
                "project_id",
                "id",
                "owner_id",
                "view",
                "history_before",
                "child_offset",
                "step_offset",
                "step_limit",
                "focus_instance_id",
                "focus_execution_id",
            ],
        )?;
        let id = params
            .get("id")
            .map(|_| identifier(params, "id"))
            .transpose()?;
        let owner_id = params
            .get("owner_id")
            .map(|_| identifier(params, "owner_id"))
            .transpose()?;
        if id.is_none() && owner_id.is_none() {
            return Err(err("invalid_params", "需要流程或工作归属"));
        }
        let view = match params.get("view").and_then(Value::as_str) {
            None if params.get("view").is_none() => WorkflowDetailView::Detail,
            Some("status") => WorkflowDetailView::Status,
            Some("detail") => WorkflowDetailView::Detail,
            Some("history") => WorkflowDetailView::History,
            _ => return Err(err("invalid_params", "无效的流程读取视图")),
        };
        let history_before = params
            .get("history_before")
            .map(|v| {
                v.as_u64()
                    .filter(|n| *n > 0 && *n <= i64::MAX as u64)
                    .ok_or_else(|| err("invalid_params", "无效的历史游标"))
            })
            .transpose()?;
        let child_offset = params
            .get("child_offset")
            .map(|v| {
                v.as_u64()
                    .filter(|n| *n <= 1_000_000)
                    .ok_or_else(|| err("invalid_params", "无效的子流程偏移"))
            })
            .transpose()?
            .unwrap_or(0);
        Ok(Self {
            focus_instance_id: params
                .get("focus_instance_id")
                .map(|_| identifier(params, "focus_instance_id"))
                .transpose()?,
            focus_execution_id: params
                .get("focus_execution_id")
                .map(|_| identifier(params, "focus_execution_id"))
                .transpose()?,
            id,
            owner_id,
            view,
            history_before,
            child_offset,
            step_offset: params
                .get("step_offset")
                .map(|v| {
                    v.as_u64()
                        .filter(|n| *n <= 1_000_000)
                        .ok_or_else(|| err("invalid_params", "无效的步骤偏移"))
                })
                .transpose()?
                .unwrap_or(0),
            step_limit: params
                .get("step_limit")
                .map(|v| {
                    v.as_u64()
                        .filter(|n| (1..=100).contains(n))
                        .ok_or_else(|| err("invalid_params", "无效的步骤分页大小"))
                })
                .transpose()?
                .unwrap_or(20),
        })
    }
}

/// Actual facts for one selected instance, never a partial EngineeringState.
/// `record.data` is a display projection for Status/History and the exact row
/// for Detail. These DTOs cannot be used as an action baseline internally.
pub struct WorkflowDetailFacts {
    pub step_offset: u64,
    pub child_offset: u64,
    pub focus: Value,
    pub cursor: u64,
    pub record: devdesk_domain::ConfigurationRecord,
    pub plan_document: Value,
    pub step_revisions: std::collections::BTreeMap<String, u64>,
    pub rule_hints: std::collections::BTreeMap<String, Option<bool>>,
    pub outputs: Vec<devdesk_domain::ConfigurationRecord>,
    pub confirmations: Vec<devdesk_domain::ConfigurationRecord>,
    pub waivers: Vec<devdesk_domain::ConfigurationRecord>,
    pub materials: Vec<devdesk_domain::ConfigurationRecord>,
    pub actors: std::collections::BTreeMap<String, WorkflowSessionActor>,
    pub children: Vec<Value>,
    pub children_more: bool,
    pub next_child_offset: Option<u64>,
    pub step_total: u64,
    pub next_step_offset: Option<u64>,
    pub history: Vec<Value>,
    pub next_history_before: Option<u64>,
}

pub struct WorkflowSessionActor {
    pub session_id: String,
    pub agent_ref: String,
    pub agent_instance_id: Option<String>,
}

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
    pub revision: u64,
    pub plan_revision_id: String,
    pub control_version: u64,
    pub step_revision: u64,
    pub feature_id: Option<String>,
    pub task_id: Option<String>,
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
    pub runtime_approval_pending: bool,
    pub workflow_approval_pending: bool,
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

#[derive(Debug, Deserialize)]
pub struct DecisionExecutionState {
    pub revision: u64,
    pub status: devdesk_domain::StepExecutionStatus,
    pub input_digest: String,
    pub outputs: std::collections::BTreeMap<String, String>,
}
/// Display qualification from one committed snapshot. It is never a write witness.
#[derive(Debug)]
pub struct DecisionContext {
    pub active_plan_revision_id: String,
    pub control_version: u64,
    pub execution: Option<DecisionExecutionState>,
    pub ancestor_instance_ids: Vec<String>,
}

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
    pub owner_id: String,
    pub status: String,
    pub dependency_wait_count: u64,
    pub delivery: Option<WorkflowDeliverySummary>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct WorkflowDeliverySummary {
    pub pull_request_url: Option<String>,
    pub pull_request_number: Option<u64>,
    pub source_revision: Option<String>,
    pub target_revision: Option<String>,
    pub candidate_revision: Option<String>,
    pub merged_revision: Option<String>,
    pub merged: bool,
    pub synchronized: bool,
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
    pub delivery: Option<WorkflowDeliverySummary>,
    pub child_slots: Vec<WorkflowChildSlots>,
}
#[derive(Debug, Serialize)]
pub struct WorkflowChildSlots {
    pub execution_id: String,
    pub limit: u64,
    pub active: u64,
    pub available: u64,
}

#[derive(Debug, Serialize)]
pub struct WorkflowStepCounts {
    pub total: u64,
    pub completed: u64,
    pub running: u64,
    pub waiting: u64,
    pub failed: u64,
}
#[derive(Debug, Serialize)]
pub struct WorkflowStageProgress {
    pub key: String,
    pub name: String,
    pub counts: WorkflowStepCounts,
}
#[derive(Debug, Serialize)]
pub struct WorkflowProgress {
    pub project_id: String,
    pub cursor: u64,
    pub record: RuntimeSummary<WorkflowProgressState>,
    pub counts: WorkflowStepCounts,
    pub stages: Vec<WorkflowStageProgress>,
    pub next_stage_offset: Option<u64>,
    pub observed_only: bool,
}
#[derive(Debug, Serialize)]
pub struct WorkflowProgressState {
    pub owner_id: String,
    pub root_instance_id: String,
    pub status: String,
    pub paused: bool,
    pub cancel_requested: bool,
}

#[derive(Debug)]
pub struct ExperienceHeader {
    pub id: String,
    pub revision: u64,
}
#[derive(Debug)]
pub struct ExperienceHeaderPage {
    pub records: Vec<ExperienceHeader>,
    pub next_after: Option<String>,
}
/// A display projection from one authoritative source snapshot; never execution
/// inputs or a replacement for the source's complete snapshot history.
#[derive(Debug)]
pub struct ExperienceDirectorySource {
    pub id: String,
    pub revision: u64,
    pub scope: devdesk_domain::Scope,
    pub content_hash: String,
    pub name: String,
    pub metadata: Value,
}

#[derive(Debug)]
pub struct ExecutionWorkspaceQuery {
    pub feature_id: Option<String>,
    pub feature_offset: u64,
    pub task_offset: u64,
    pub limit: u64,
    pub work_filter: String,
}
impl ExecutionWorkspaceQuery {
    pub fn parse(params: &Value) -> Result<Self> {
        fields(
            params,
            &[
                "project_id",
                "feature_id",
                "offset",
                "task_offset",
                "limit",
                "work_filter",
                "project_offset",
                "github_only",
            ],
        )?;
        fn offset(params: &Value, name: &str, default: u64, maximum: u64) -> Result<u64> {
            params
                .get(name)
                .map(|value| {
                    value
                        .as_u64()
                        .filter(|n| *n <= maximum)
                        .ok_or_else(|| err("invalid_params", format!("无效的 {name}")))
                })
                .transpose()
                .map(|value| value.unwrap_or(default))
        }
        // Directory pagination is validated even though it is read by the Host.
        offset(params, "project_offset", 0, 1_000_000)?;
        if params
            .get("github_only")
            .is_some_and(|value| !value.is_boolean())
        {
            return Err(err("invalid_params", "github_only 需要布尔值"));
        }
        let limit = offset(params, "limit", 50, 100)?;
        if limit == 0 {
            return Err(err("invalid_params", "limit 必须大于 0"));
        }
        let work_filter = params
            .get("work_filter")
            .map(|value| {
                value
                    .as_str()
                    .filter(|value| {
                        ["", "tasks", "running", "waiting_confirmation", "blocked"].contains(value)
                    })
                    .map(str::to_owned)
                    .ok_or_else(|| err("invalid_params", "无效的工作筛选"))
            })
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            feature_id: params
                .get("feature_id")
                .map(|_| identifier(params, "feature_id"))
                .transpose()?,
            feature_offset: offset(params, "offset", 0, 1_000_000)?,
            task_offset: offset(params, "task_offset", 0, 1_000_000)?,
            limit,
            work_filter,
        })
    }
}
#[derive(Debug, Serialize)]
pub struct FeatureBrief {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub title_truncated: bool,
    pub status: String,
    pub revision: u64,
    pub input_version: u64,
}
#[derive(Debug, Serialize)]
pub struct TaskBrief {
    pub id: String,
    pub feature_id: String,
    pub title: String,
    pub title_truncated: bool,
    pub status: String,
    pub revision: u64,
    pub dependency_count: u64,
}
#[derive(Debug, Serialize)]
pub struct ExecutionFeatureDetail {
    pub feature: FeatureBrief,
    pub tasks: Vec<TaskBrief>,
    pub total_tasks: u64,
}
/// Display metadata only. Editing requires an explicit management/configuration read.
#[derive(Debug, Serialize)]
pub struct WorkRootMetadata {
    pub id: String,
    pub name: String,
    pub revision: u64,
    pub r#type: &'static str,
    pub work_root: bool,
    pub metadata_only: bool,
    pub data: WorkRootState,
}
#[derive(Debug, Serialize)]
pub struct WorkRootState {
    pub owner_id: String,
    pub root_instance_id: String,
    pub parent_execution_id: Option<String>,
    pub active_plan_revision_id: Option<String>,
    pub workflow_ref: Option<String>,
    pub status: String,
    pub plan: PlanSelectionSummary,
}
#[derive(Debug, Serialize)]
pub struct ExecutionWorkspace {
    pub kind: &'static str,
    pub project_id: String,
    pub cursor: u64,
    pub active_project: devdesk_domain::Project,
    pub repository_ready: bool,
    pub features: Vec<FeatureBrief>,
    pub total_features: u64,
    pub detail: Option<ExecutionFeatureDetail>,
    pub configurations: Vec<WorkRootMetadata>,
    pub observed_only: bool,
}

#[derive(Debug)]
pub struct SchedulingHints {
    pub stages: Vec<crate::SchedulingStage>,
    pub pending: bool,
    pub live: bool,
    pub running_conversations: usize,
}
#[derive(Debug)]
pub struct TailEvent {
    pub event: devdesk_domain::Event,
    /// A large persisted event is a reset boundary, never a silently clipped event.
    pub omitted: bool,
}
