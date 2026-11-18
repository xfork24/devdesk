//! Durable manager sessions and native workflow tool calls.
use crate::*;

pub const CONTROL_TOOLS: &[&str] = &[
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
];

pub(crate) fn control_query(method: &str) -> bool {
    matches!(
        method,
        "workflow.inspect"
            | "workflow.events.read"
            | "workflow.outputs.read"
            | "workflow.execution.inspect"
            | "workflow.change.preview"
            | "workflow.rework.preview"
    )
}

pub(crate) fn native_reply_value(method: &str, call_id: &str, mut value: Value) -> Value {
    if method.starts_with("workflow.step.")
        && let Some(execution) = value.get_mut("execution").and_then(Value::as_object_mut)
    {
        let inputs = execution.remove("inputs").is_some();
        let context = execution.remove("runtime_context").is_some();
        if inputs {
            execution.insert("inputs_omitted".into(), json!(true));
        }
        if context {
            execution.insert("runtime_context_omitted".into(), json!(true));
        }
        if inputs || context {
            value["receipt_ref"] = json!(call_id);
        }
    }
    if !control_query(method) && value.to_string().len() > 128 * 1024 {
        json!({"id":value["id"],"status":value["status"],"receipt_ref":call_id,"result_digest":configuration_hash(&value),"accepted":true,"next":"Use workflow.inspect to read the current bounded projection."})
    } else {
        value
    }
}

pub(crate) fn operation(method: &str) -> &str {
    match method {
        "workflow.events.read" | "workflow.outputs.read" | "workflow.execution.inspect" => {
            "inspect"
        }
        "workflow.materials.import" => "import",
        "workflow.decision.request" => "request_decision",
        "workflow.segment.dispatch" => "segment",
        "workflow.controller.wait" | "workflow.controller.notify" => "inspect",
        _ => method.rsplit('.').next().unwrap_or(""),
    }
}

pub(crate) fn binding<'a>(s: &'a EngineeringState, sid: &str) -> Option<&'a ConfigurationRecord> {
    s.governance
        .iter()
        .find(|r| r.r#type == "controller_session" && r.definition_id == sid)
}
pub(crate) fn authorized_send(s: &EngineeringState, c: &Command) -> bool {
    let Some(target) = c.target.as_deref() else {
        return false;
    };
    let Some(binding) = binding(s, target) else {
        return false;
    };
    let Ok(row) = s.configuration(binding.data["controller_id"].as_str().unwrap_or("")) else {
        return false;
    };
    row.r#type == "workflow_controller"
        && row.data["status"] == "active"
        && row.data["session_id"] == target
        && row.data["epoch"] == binding.data["epoch"]
        && c.payload["controller_epoch"] == row.data["epoch"]
        && s.governance.iter().any(|r| {
            r.r#type == "workflow_intent"
                && r.definition_id == row.id
                && r.data["method"] == "conversation.send"
                && r.data["command"] == json!(c)
        })
}

#[cfg(test)]
fn controller_platform_wait_ms(state: &EngineeringState, turn: &str, start: u64, end: u64) -> u64 {
    let mut intervals: Vec<_> = state
        .governance
        .iter()
        .filter(|row| row.r#type == "controller_tool_call" && row.data["turn_id"] == turn)
        .map(|call| {
            let reply = state
                .governance
                .iter()
                .find(|row| row.r#type == "controller_tool_reply" && row.definition_id == call.id);
            (
                call.created_at.max(start).min(end),
                reply.map_or(end, |row| row.created_at).max(start).min(end),
            )
        })
        .filter(|(a, b)| a < b)
        .collect();
    intervals.sort_unstable();
    let (mut total, mut last) = (0u64, start);
    for (a, b) in intervals {
        total = total.saturating_add(b.saturating_sub(a.max(last)));
        last = last.max(b);
    }
    total
}

#[cfg(test)]
mod reply_tests {
    use super::*;
    use crate::governance::record;

    #[test]
    fn platform_wait_counts_overlaps_once_and_clamps_to_the_turn() {
        let mut state = EngineeringState::default();
        for (id, start, finish) in [("a", 10, Some(50)), ("b", 40, Some(70)), ("c", 80, None)] {
            let mut call = record(
                "controller_tool_call",
                "manager".into(),
                Scope::global(),
                id.into(),
                1,
                None,
                json!({"turn_id":"turn"}),
                false,
            );
            call.id = id.into();
            call.created_at = start;
            state.governance.push(call);
            if let Some(finish) = finish {
                let mut reply = record(
                    "controller_tool_reply",
                    id.into(),
                    Scope::global(),
                    "reply".into(),
                    1,
                    None,
                    json!({}),
                    true,
                );
                reply.created_at = finish;
                state.governance.push(reply);
            }
        }
        assert_eq!(controller_platform_wait_ms(&state, "turn", 20, 100), 70);
        assert_eq!(controller_platform_wait_ms(&state, "turn", 50, 60), 10);
        assert_eq!(controller_platform_wait_ms(&state, "another", 20, 100), 0);
    }

    #[test]
    fn step_receipts_keep_control_identity_without_repeating_frozen_inputs() {
        let original = json!({"id":"root","revision":4,"plan_revision_id":"plan","control_version":7,"step_revision":3,"execution":{"id":"execution","step":"implementation","status":"running","input_digest":"exact-input","outputs":{"result":"exact-output"},"round":2,"inputs":{"task":"x".repeat(400_000)},"runtime_context":{"instructions":"y".repeat(400_000)}}});
        let projected =
            native_reply_value("workflow.step.dispatch", "original-call", original.clone());
        assert!(serde_json::to_vec(&projected).unwrap().len() < 4096);
        for field in [
            "id",
            "revision",
            "plan_revision_id",
            "control_version",
            "step_revision",
        ] {
            assert_eq!(projected[field], original[field]);
        }
        for field in ["id", "step", "status", "input_digest", "outputs", "round"] {
            assert_eq!(projected["execution"][field], original["execution"][field]);
        }
        assert!(projected["execution"].get("inputs").is_none());
        assert_eq!(projected["execution"]["inputs_omitted"], true);
        assert_eq!(projected["receipt_ref"], "original-call");
        assert_eq!(
            original["execution"]["inputs"]["task"]
                .as_str()
                .unwrap()
                .len(),
            400_000
        );
    }

    #[test]
    fn large_inspection_retains_real_gate_conditions_in_the_native_reply() {
        let result = json!({"steps":[{"step":"review","human_satisfied":true}],"contract":"x".repeat(160 * 1024)});
        assert_eq!(
            native_reply_value("workflow.inspect", "source", result.clone()),
            result
        );
        let mutation = native_reply_value("workflow.change.apply", "source", result);
        assert_eq!(mutation["receipt_ref"], "source");
        assert_eq!(mutation["accepted"], true);
    }
}
