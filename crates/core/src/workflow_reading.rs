//! Scope-aware readers backed by bounded native projections.
use crate::read_models::WorkflowReadingView;
use crate::*;

pub(crate) fn unstarted_execution_actor(
    catalog: &EngineeringState,
    execution: &Value,
    step: &Value,
    definitions: &Value,
) -> Value {
    let kind = step["executor"]["type"].as_str().unwrap_or("unknown");
    let tool_ref = step["executor"]["tool_ref"].as_str().unwrap_or("");
    let name = definitions[tool_ref]["name"]
        .as_str()
        .or_else(|| {
            catalog
                .configuration(tool_ref)
                .ok()
                .map(|r| r.name.as_str())
        })
        .unwrap_or(match kind {
            "agent" => "尚无实际 Agent 会话",
            "workflow" => "子流程",
            "control" => "流程控制",
            "tool" => tool_ref,
            _ => "未记录执行方式",
        });
    json!({"kind":kind,"name":name,"assigned_agent_ref":execution["agent_revision_id"],"role_ref":step["executor"]["role_ref"]})
}
impl Engine {
    pub(crate) fn workflow_contexts(&self, p: &Value) -> Result<Value> {
        self.store
            .workflow_reading(WorkflowReadingView::Contexts, p)
    }
    pub(crate) fn workflow_executions(&self, p: &Value) -> Result<Value> {
        self.store
            .workflow_reading(WorkflowReadingView::Executions, p)
    }
    pub(crate) fn workflow_record(&self, p: &Value) -> Result<Value> {
        self.store.workflow_reading(WorkflowReadingView::Record, p)
    }
}
