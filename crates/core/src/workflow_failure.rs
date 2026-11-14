//! Resolve failure provenance and attempt counts from durable execution facts.
use crate::*;

pub(crate) fn retry_count(state: &EngineeringState, execution: &StepExecution) -> u64 {
    state
        .governance
        .iter()
        .filter(|r| {
            r.r#type == "workflow_execution_history"
                && r.data["workflow_instance_id"] == execution.workflow_instance_id
                && r.data["node_id"] == execution.node_id
                && r.data["round"] == execution.round
        })
        .map(|r| &r.definition_id)
        .collect::<std::collections::BTreeSet<_>>()
        .len() as u64
}
fn from_value(value: &Value, source: &str, message: &str) -> WorkflowFailure {
    let mut value = value.clone();
    if value.is_object() && value.get("message").is_none() {
        value["message"] = json!(message);
    }
    match serde_json::from_value::<Error>(value) {
        Ok(error) => {
            WorkflowFailure::from_error(&error, source.into(), ExecutionFailureKind::Unknown)
        }
        Err(_) => WorkflowFailure {
            code: "unclassified_failure".into(),
            message: message.into(),
            source_ref: source.into(),
            kind: ExecutionFailureKind::Unknown,
            effect: ExecutionFailureEffect::Unknown,
        },
    }
}
pub(crate) fn observed_failure(
    state: &EngineeringState,
    execution: &StepExecution,
) -> WorkflowFailure {
    if let Some(call) = execution
        .tool_call_id
        .as_deref()
        .and_then(|id| state.configuration(id).ok())
    {
        if call.data["status"] == "succeeded" {
            return WorkflowFailure {
                code: "output_invalid".into(),
                message: execution.reason.clone(),
                source_ref: call.id.clone(),
                kind: ExecutionFailureKind::Business,
                effect: ExecutionFailureEffect::Applied,
            };
        }
        let error = call
            .data
            .get("error")
            .or_else(|| call.data.get("receipt").and_then(|r| r.get("error")))
            .unwrap_or(&Value::Null);
        let mut failure = from_value(error, &call.id, &execution.reason);
        let effect = call.data.get("remote_effect").or_else(|| {
            call.data
                .get("receipt")
                .and_then(|r| r.get("remote_effect"))
        });
        if let Some(effect) = effect {
            failure.effect = match effect.as_str() {
                Some("not_applied") => ExecutionFailureEffect::NotApplied,
                Some("applied") => ExecutionFailureEffect::Applied,
                _ => ExecutionFailureEffect::Unknown,
            };
        }
        return failure;
    }
    if let Some(intent) = state.governance.iter().rev().find(|r| {
        r.r#type == "workflow_intent"
            && r.definition_id == execution.id
            && r.data["status"] == "failed"
            && (r.data["method"] != "conversation.send"
                || r.data["command"]["payload"]["client_message_id"]
                    == format!("{}:{}", execution.id, execution.sequence))
    }) {
        return from_value(&intent.data["error"], &intent.id, &execution.reason);
    }
    if let Some(turn) = execution
        .turn_id
        .as_deref()
        .and_then(|id| state.turns.iter().find(|t| t.id == id))
    {
        if turn.status == TurnStatus::Completed {
            return WorkflowFailure {
                code: "output_invalid".into(),
                message: execution.reason.clone(),
                source_ref: turn.id.clone(),
                kind: ExecutionFailureKind::Business,
                effect: ExecutionFailureEffect::Applied,
            };
        }
        if let Some(event) = state.conversation_events.iter().find(|event| {
            event.turn_id == turn.id
                && event.deduplication_key == format!("{}:execution-failure", turn.id)
        }) {
            let value = serde_json::from_str::<Value>(&event.content).unwrap_or(Value::Null);
            let mut failure = from_value(&value, &event.id, &execution.reason);
            if turn.consumed {
                failure.effect = ExecutionFailureEffect::Unknown;
            }
            return failure;
        }
    }
    WorkflowFailure {
        code: "unclassified_failure".into(),
        message: execution.reason.clone(),
        source_ref: execution.id.clone(),
        kind: ExecutionFailureKind::Unknown,
        effect: ExecutionFailureEffect::Unknown,
    }
}

pub(crate) fn retry_supplemental_input(
    state: &EngineeringState,
    execution: &StepExecution,
) -> Result<Option<String>> {
    let mut predecessor = execution.retry_of.clone();
    let mut seen = std::collections::BTreeSet::new();
    while let Some(id) = predecessor {
        require(seen.insert(id.clone()), "技术重试来源形成环")?;
        require(seen.len() <= 32, "技术重试来源超过边界")?;
        let old: StepExecution = state
            .governance
            .iter()
            .find(|r| r.r#type == "workflow_execution_history" && r.definition_id == id)
            .ok_or_else(|| err("storage", "技术重试来源缺失"))?
            .decode()?;
        require(
            old.workflow_instance_id == execution.workflow_instance_id
                && old.node_id == execution.node_id
                && old.round == execution.round,
            "技术重试来源不匹配",
        )?;
        if let Some(intent) = state.governance.iter().find(|r| {
            r.r#type == "workflow_intent"
                && r.definition_id == old.id
                && r.data["method"] == "conversation.send"
                && r.data["command"]["payload"]["client_message_id"]
                    == format!("{}:{}", old.id, old.sequence)
        }) {
            let task: Value = serde_json::from_str(
                intent.data["command"]["payload"]["input"]
                    .as_str()
                    .ok_or_else(|| err("storage", "原任务正文缺失"))?,
            )
            .map_err(|e| err("storage", e.to_string()))?;
            return Ok(task["supplemental_input"].as_str().map(str::to_owned));
        }
        predecessor = old.retry_of;
    }
    Ok(None)
}

pub(crate) fn failure_message(value: &Value, fallback: &str) -> String {
    value
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| value.as_str())
        .unwrap_or(fallback)
        .to_owned()
}
