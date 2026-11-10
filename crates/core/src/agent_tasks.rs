//! Exact Session associations for configuration-owned Workflow executions.
use crate::governance::{record, update_record};
use crate::*;
pub(crate) fn session_binding<'a>(
    state: &'a EngineeringState,
    session: &str,
) -> Option<&'a ConfigurationRecord> {
    state
        .governance
        .iter()
        .find(|r| r.kind == "configured_session" && r.definition_id == session)
}
pub(crate) fn session_workflow_context(state: &EngineeringState, session: &str) -> Option<Value> {
    let source = session_binding(state, session)?;
    Some(
        json!({"step_instance_id":source.data["execution_id"],"step":source.data["step"],"instance_id":source.data["instance_id"]}),
    )
}
pub(crate) fn session_matches_context(
    state: &EngineeringState,
    session: &Session,
    step: &str,
) -> bool {
    session_workflow_context(state, &session.id).is_some_and(|v| v["step_instance_id"] == step)
}
pub(crate) fn formal_session(state: &EngineeringState, session: &str) -> bool {
    session_binding(state, session).is_some()
}
impl Engine {
    pub(crate) fn inherit_task_binding(
        &self,
        state: &mut EngineeringState,
        previous: &str,
        next: &str,
    ) -> Result<()> {
        let Some(source) = session_binding(state, previous).cloned() else {
            return Ok(());
        };
        let row = state
            .configuration(
                source.data["instance_id"]
                    .as_str()
                    .ok_or_else(|| err("task_contract_mismatch", "任务来源缺失"))?,
            )?
            .clone();
        let mut instance: ConfiguredWorkflowInstance = row.decode()?;
        let execution = instance
            .steps
            .iter_mut()
            .find(|e| Some(e.id.as_str()) == source.data["execution_id"].as_str())
            .ok_or_else(|| err("task_contract_mismatch", "执行已失效"))?;
        require(
            execution.session_id.as_deref() == Some(previous),
            "后继会话的任务来源不匹配",
        )?;
        execution.session_id = Some(next.into());
        update_record(
            state
                .governance
                .iter_mut()
                .find(|r| r.id == row.id)
                .unwrap(),
            json!(instance),
        );
        state.governance.push(record(
            "configured_session",
            next.into(),
            source.scope,
            source.name,
            1,
            None,
            source.data,
            true,
        ));
        Ok(())
    }
}
