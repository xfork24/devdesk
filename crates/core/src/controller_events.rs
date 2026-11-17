//! Commit-time workflow facts. Notifications never choose a business action.
use crate::*;
use std::collections::BTreeMap;

fn step_fact(step: &Value) -> Value {
    json!({"execution_id":step["id"],"step":step["step"],"status":step["status"],
        "outputs":step["outputs"],"input_digest":step["input_digest"],
        "session_id":step["session_id"],"turn_id":step["turn_id"],"reason":step["reason"].as_str().map(|v|v.chars().take(512).collect::<String>())})
}

/// Called by the store before committing accepted changes, in the same transaction.
/// Existing stores already retain the before image; no extra snapshot or graph hash.
pub fn append_workflow_events(before: &EngineeringState, state: &mut EngineeringState) {
    let old: BTreeMap<_, _> = before
        .governance
        .iter()
        .map(|r| (r.id.as_str(), r))
        .collect();
    let context = state
        .governance
        .iter()
        .rev()
        .find(|r| r.r#type == "command_context" && !old.contains_key(r.id.as_str()));
    let actor = context
        .map(|r| r.data["context"]["actor"].clone())
        .unwrap_or(Value::Null);
    let cause_call_id = context
        .and_then(|r| r.definition_id.strip_suffix(":action"))
        .map(str::to_owned);
    let mut facts = Vec::new();
    let target = |r: &ConfigurationRecord| json!({"instance_id":r.id,"root_instance_id":r.data["root_instance_id"],"source_revision":r.revision});
    for row in state.governance.iter().filter(|r| {
        matches!(
            r.r#type.as_str(),
            "workflow_instance" | "workflow_pending_decision" | "workflow_confirmation"
        )
    }) {
        let previous = old.get(row.id.as_str()).copied();
        if previous.is_some_and(|r| r.revision == row.revision) {
            continue;
        }
        if row.r#type == "workflow_instance" {
            let mut changes = Vec::new();
            for field in [
                "status",
                "paused",
                "cancel_requested",
                "active_plan_revision_id",
            ] {
                if previous.is_none_or(|r| r.data[field] != row.data[field]) {
                    changes.push(json!({"kind":field,"value":row.data[field]}));
                }
            }
            let prior: BTreeMap<_, _> = previous
                .into_iter()
                .flat_map(|r| r.data["steps"].as_array().into_iter().flatten())
                .filter_map(|s| s["id"].as_str().map(|id| (id, s)))
                .collect();
            for step in row.data["steps"].as_array().into_iter().flatten() {
                if prior
                    .get(step["id"].as_str().unwrap_or(""))
                    .is_none_or(|s| step_fact(s) != step_fact(step))
                {
                    changes.push(json!({"kind":"execution","execution":step_fact(step)}));
                }
            }
            if !changes.is_empty() {
                for batch in changes.chunks(16) {
                    facts.push((
                        target(row),
                        json!({"kind":"workflow.changed","changes":batch}),
                    ));
                }
            }
        } else if row.r#type == "workflow_pending_decision"
            && previous.is_none_or(|r| r.data["status"] != row.data["status"])
        {
            if let Some(instance) = state
                .governance
                .iter()
                .find(|i| i.id == row.definition_id && i.r#type == "workflow_instance")
            {
                facts.push((target(instance), json!({"kind":"decision.changed","decision_id":row.id,"status":row.data["status"],"decision_ref":row.data["decision_ref"]})));
            }
        } else if row.r#type == "workflow_confirmation"
            && previous.is_none()
            && let Some(instance) = state.governance.iter().find(|i| {
                i.r#type == "workflow_instance"
                    && i.data["steps"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|s| s["id"] == row.definition_id)
            })
        {
            facts.push((target(instance), json!({"kind":"human.confirmed","execution_id":row.definition_id,"confirmation_ref":row.id})));
        }
    }
    for worker in &state.executions {
        if worker.writer_revoked
            && before
                .executions
                .iter()
                .any(|w| w.operation_id == worker.operation_id && !w.writer_revoked)
        {
            for instance in state
                .governance
                .iter()
                .filter(|r| r.r#type == "workflow_instance")
            {
                if instance.data["steps"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|s| s["session_id"] == worker.session_id)
                {
                    facts.push((target(instance), json!({"kind":"execution.stopped","session_id":worker.session_id,"operation_id":worker.operation_id})));
                }
            }
        }
    }
    for (instance, change) in facts {
        event(
            state,
            instance["instance_id"].as_str().unwrap(),
            "workflow.runtime",
            &json!({
                "root_instance_id":instance["root_instance_id"],"instance_id":instance["instance_id"],
                "source_revision":instance["source_revision"],"origin_turn_id":actor["turn_id"],"cause_call_id":cause_call_id,
                "change":change
            })
            .to_string(),
        );
    }
}

#[cfg(test)]
pub(crate) fn pending_events(
    state: &EngineeringState,
    controller: &WorkflowController,
) -> Vec<Value> {
    let mut bytes = 2;
    state.events[state
        .events
        .partition_point(|e| e.id <= controller.source_cursor)..]
        .iter()
        .filter(|e| e.kind == "workflow.runtime")
        .filter_map(|e| {
            serde_json::from_str::<Value>(&e.detail)
                .ok()
                .map(|data| (e, data))
        })
        .filter(|(_, data)| {
            data["root_instance_id"].as_str() == controller.root_instance_id.as_deref()
        })
        .take(32)
        .map(|(e, mut data)| {
            // Large notifications retain their exact source; details remain in
            // the authoritative instance and event, rather than filling a Turn.
            if e.detail.len() > 16 * 1024 {
                let has_outputs = data["change"]["changes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|c| c["execution"]["outputs"].as_object().is_some_and(|v| !v.is_empty()));
                let executions: Vec<_> = data["change"]["changes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|c| c["execution"].as_object())
                    .take(16)
                    .map(|e| json!({"execution_id":e.get("execution_id"),"step":e.get("step"),"status":e.get("status")}))
                    .collect();
                data["change"] = json!({"kind":data["change"]["kind"],
                    "details_omitted":true,"record_ref":data["instance_id"],"executions":executions,"has_outputs":has_outputs});
            }
            json!({"id":e.id,"source":data})
        })
        .take_while(|item| {
            let size = item.to_string().len() + 1;
            if bytes + size > 64 * 1024 {
                return false;
            }
            bytes += size;
            true
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn wait(
    state: &mut EngineeringState,
    id: &str,
    turn: &str,
    args: &Value,
) -> Result<Value> {
    let row = state.configuration(id)?.clone();
    let mut controller: WorkflowController = row.decode()?;
    require(
        controller.status == "active" && controller.delivery_id == args["delivery_id"],
        "等待必须绑定当前经理投递",
    )?;
    let summary = text(args, "summary")?;
    require(summary.len() <= 8192, "经理工作摘要过大")?;
    let waiting: Vec<String> =
        serde_json::from_value(args.get("waiting_for").cloned().unwrap_or(json!([])))
            .map_err(|e| err("invalid_params", e.to_string()))?;
    require(waiting.len() <= 32, "等待对象过多")?;
    for reference in &waiting {
        let r = state.configuration(reference)?;
        let root = if r.r#type == "workflow_instance" {
            r.data["root_instance_id"].as_str()
        } else {
            state
                .configuration(&r.definition_id)
                .ok()
                .and_then(|i| i.data["root_instance_id"].as_str())
        };
        require(
            root == controller.root_instance_id.as_deref(),
            "等待对象不属于当前根实例",
        )?;
    }
    let seconds = args["timeout_seconds"].as_u64();
    require(
        seconds.is_none_or(|s| s > 0 && s <= controller.policy.timeout_seconds),
        "等待期限超出预算",
    )?;
    // Acknowledge own actions only when their actual native reply has been delivered.
    // Never advance over an unrelated concurrent result.
    let mut cursor = controller.source_cursor;
    for item in pending_events(state, &controller) {
        if item["source"]["origin_turn_id"] != turn
            || !state.governance.iter().any(|r| {
                r.r#type == "controller_tool_reply"
                    && r.definition_id == item["source"]["cause_call_id"]
                    && r.data["status"] == "delivered"
            })
        {
            break;
        }
        cursor = item["id"].as_u64().unwrap();
    }
    controller.source_cursor = cursor;
    controller.acknowledged_cursor = cursor;
    let pending = pending_events(state, &controller).len();
    controller.run_state = if pending == 0 { "waiting" } else { "runnable" }.into();
    controller.checkpoint = json!({"summary":summary,"waiting_for":waiting,"turn_id":turn});
    controller.wait_deadline = seconds.map(|s| now() + s * 1000);
    controller.wait_turn_id = Some(turn.into());
    crate::governance::update_record(
        state.governance.iter_mut().find(|r| r.id == id).unwrap(),
        json!(controller),
    );
    Ok(
        json!({"status":if pending == 0 {"waiting"}else{"pending_events"},"pending":pending,"cursor":cursor}),
    )
}

/// Exact runtime event page and head are sampled together by the adapter.
pub(crate) fn native_events(
    engine: &Engine,
    root: &str,
    cursor: u64,
) -> Result<(Vec<Value>, crate::concurrency::ResourceSnapshot)> {
    let selected = crate::platform_facts::one(
        engine.store.as_ref(),
        "controller_runtime_events",
        &json!([root, cursor]).to_string(),
    )?;
    let mut items = Vec::new();
    let mut bytes = 2;
    for event in selected
        .value
        .as_ref()
        .and_then(|v| v["events"].as_array())
        .into_iter()
        .flatten()
    {
        let detail = event["detail"]
            .as_str()
            .ok_or_else(|| err("storage", "经理事件缺少原始详情"))?;
        let mut data: Value =
            serde_json::from_str(detail).map_err(|e| err("storage", e.to_string()))?;
        require(data["root_instance_id"] == root, "经理事件跨流程根")?;
        if detail.len() > 16 * 1024 {
            let has_outputs = data["change"]["changes"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|c| {
                    c["execution"]["outputs"]
                        .as_object()
                        .is_some_and(|v| !v.is_empty())
                });
            let executions:Vec<_>=data["change"]["changes"].as_array().into_iter().flatten().filter_map(|c|c["execution"].as_object()).take(16).map(|e|json!({"execution_id":e.get("execution_id"),"step":e.get("step"),"status":e.get("status")})).collect();
            data["change"] = json!({"kind":data["change"]["kind"],"details_omitted":true,"record_ref":data["instance_id"],"executions":executions,"has_outputs":has_outputs});
        }
        let item = json!({"id":event["id"],"source":data});
        let size = item.to_string().len() + 1;
        if bytes + size > 64 * 1024 {
            break;
        }
        bytes += size;
        items.push(item);
    }
    Ok((items, selected))
}
