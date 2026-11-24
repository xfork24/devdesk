//! Managed engineering operations. Handler identifiers are protocols, not business actions.
use crate::*;

pub(crate) const PLATFORM_HANDLERS: &[&str] = &[
    "spec.lint",
    "domain.read",
    "project.overview",
    "project.records.list",
    "project.record.read",
    "project.artifact.read",
    "project.resources",
    "configuration.catalog",
    "configuration.read",
    "configuration.usage",
    "configuration.change.preview",
    "configuration.change.apply",
    "configuration.transfer.stage",
    "configuration.transfer.apply",
    "configuration.transfer.cancel",
    "configuration.transfer.status",
    "workflow.start",
    "workflow.inspect",
    "workflow.events.read",
    "workflow.outputs.read",
    "workflow.execution.inspect",
    "workflow.step.dispatch",
    "workflow.step.accept",
    "workflow.step.input",
    "workflow.step.retry",
    "workflow.step.rework",
    "workflow.rework.preview",
    "workflow.execution.stop",
    "workflow.pause",
    "workflow.resume",
    "workflow.cancel",
    "workflow.change.preview",
    "workflow.change.apply",
    "workflow.materials.import",
    "workflow.decision.request",
    "workflow.execution.reconcile",
    "workflow.complete",
    "workflow.segment.dispatch",
    "workflow.controller.wait",
    "workflow.controller.notify",
    "workspace.current",
    "workspace.snapshot",
    "workspace.checkpoint",
    "workspace.target",
    "tasks.create",
    "tasks.complete",
    "materials.capture",
    "tests.run",
    "tests.project",
    "git.source.publish",
    "git.delivery.open",
    "git.delivery.merge",
    "git.candidate",
    "git.advance",
    "git.publish",
];
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformToolRequest {
    #[serde(default)]
    pub definition_ref: Option<Id>,
    pub execution_id: Id,
    pub instance_id: Id,
    pub owner_id: Id,
    pub project_id: Id,
    pub definition: ToolDefinition,
    pub arguments: Value,
    pub runtime: Option<ExecutionInstance>,
    pub permissions: Option<VersionedPermissionLock>,
    #[serde(default)]
    pub caller: Option<AgentPlatformCaller>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPlatformCaller {
    pub execution_id: Id,
    pub session_id: Id,
    pub turn_id: Id,
    pub operation_id: Id,
    pub request_id: Value,
    pub call_id: String,
    pub tool_ref: Id,
}
impl PlatformToolRequest {
    pub(crate) fn step_execution_id(&self) -> &str {
        self.caller
            .as_ref()
            .map_or(&self.execution_id, |caller| &caller.execution_id)
    }
}

/// Common call-history projection; the stored platform request and receipt retain their own protocol.
pub(crate) fn project_platform_call(
    state: &EngineeringState,
    record: &ConfigurationRecord,
) -> ConfigurationRecord {
    let mut projected = record.clone();
    let request = &record.data["request"];
    let caller = &request["caller"];
    let session = caller["session_id"]
        .as_str()
        .and_then(|id| state.sessions.iter().find(|session| session.id == id));
    let agent_instance = session
        .and_then(|session| session.runtime_binding.as_ref())
        .and_then(|binding| binding.agent_configuration.as_ref())
        .map(|configuration| &configuration.agent_instance_id);
    let definition = caller["tool_ref"].as_str().or_else(|| {
        state
            .governance
            .iter()
            .find(|definition| {
                definition.r#type == "tool" && definition.data == request["definition"]
            })
            .map(|definition| definition.id.as_str())
    });
    let task = state
        .tasks
        .iter()
        .find(|task| request["owner_id"] == task.id);
    let feature = task.map(|task| task.feature_id.as_str()).or_else(|| {
        state
            .features
            .iter()
            .find(|feature| request["owner_id"] == feature.id)
            .map(|feature| feature.id.as_str())
    });
    projected.data["invocation"] = json!({"id":record.id,"project_id":request["project_id"],"feature_id":feature,"task_id":task.map(|task|&task.id),"agent_instance_id":agent_instance,"session_id":caller["session_id"],"turn_id":caller["turn_id"],"operation_id":caller["operation_id"].as_str().unwrap_or(&record.id),"tool":{"revision_id":definition,"definition":request["definition"]},"arguments":request["arguments"],"input_hash":configuration_hash(&request["arguments"])});
    projected.data["platform_receipt"] = record.data["receipt"].clone();
    projected.data["receipt"]=state.governance.iter().find(|reply|reply.r#type=="platform_tool_reply"&&reply.definition_id==record.id).map(|reply|reply.data["receipt"].clone()).unwrap_or_else(||json!({"success":record.data["status"]=="succeeded","structured_content":record.data["receipt"],"error":record.data["error"]}));
    projected
}
