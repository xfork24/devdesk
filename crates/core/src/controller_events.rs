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
                let mut fact = json!({"kind":"decision.changed","decision_id":row.id,"status":row.data["status"],"decision_ref":row.data["decision_ref"]});
                if let Some(reference) = row.data.get("material_input_ref") {
                    fact["material_input_ref"] = reference.clone();
                }
                facts.push((target(instance), fact));
            }
        } else if row.r#type == "workflow_reconciliation"
            && previous.is_none_or(|r| r.data["status"] != row.data["status"])
        {
            if let Some(instance) = state
                .governance
                .iter()
                .find(|r| r.id == row.data["instance_id"] && r.r#type == "workflow_instance")
            {
                facts.push((target(instance),json!({"kind":"execution.reconciled","reconciliation_id":row.id,"execution_id":row.data["execution_id"],"status":row.data["status"],"error":row.data["error"]})));
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
    controller: Option<(&str, u64)>,
) -> Result<(Vec<Value>, crate::concurrency::ResourceSnapshot, u64)> {
    let selected = crate::platform_facts::one(
        engine.store.as_ref(),
        "controller_runtime_events",
        &json!([root, cursor]).to_string(),
    )?;
    let empty = Vec::new();
    let events = selected
        .value
        .as_ref()
        .and_then(|v| v["events"].as_array())
        .unwrap_or(&empty);
    let (items, consumed) = native_event_items(events, root, cursor, controller)?;
    Ok((items, selected, consumed))
}

fn native_event_items(
    events: &[Value],
    root: &str,
    cursor: u64,
    controller: Option<(&str, u64)>,
) -> Result<(Vec<Value>, u64)> {
    native_event_items_at(events, root, cursor, controller, now())
}

fn native_event_items_at(
    events: &[Value],
    root: &str,
    cursor: u64,
    controller: Option<(&str, u64)>,
    observed_at: u64,
) -> Result<(Vec<Value>, u64)> {
    let delegated = |change: &Value| {
        controller.is_some_and(|(id, epoch)| {
            change["kind"] == "execution"
                && change["delegated_progress"]["controller_id"] == id
                && change["delegated_progress"]["epoch"] == epoch
                && matches!(
                    change["execution"]["status"].as_str(),
                    Some("running" | "awaiting_acceptance" | "completed")
                )
        })
    };
    let mut items = Vec::new();
    let mut bytes = 2;
    let mut consumed = cursor;
    for event in events {
        let detail = event["detail"]
            .as_str()
            .ok_or_else(|| err("storage", "经理事件缺少原始详情"))?;
        let mut data: Value =
            serde_json::from_str(detail).map_err(|e| err("storage", e.to_string()))?;
        require(data["root_instance_id"] == root, "经理事件跨流程根")?;
        // Keep every event for SSE/audit. The manager need not run another model
        // turn solely because an already-running step gained session/turn IDs.
        // Never advance past a meaningful event the manager has not received.
        let stop_result = data["change"]["kind"] == "execution.stopped"
            && data["change"]["turn_id"].is_string()
            && events.iter().any(|later| {
                if later["id"].as_u64() <= event["id"].as_u64() {
                    return false;
                }
                let Some(raw) = later["detail"].as_str() else {
                    return false;
                };
                let Ok(next) = serde_json::from_str::<Value>(raw) else {
                    return false;
                };
                next["root_instance_id"] == root
                    && next["instance_id"] == data["instance_id"]
                    && next["change"]["kind"] == "workflow.changed"
                    && next["change"]["changes"].as_array().is_some_and(|changes| {
                        changes.iter().any(|change| {
                            let e = &change["execution"];
                            change["kind"] == "execution"
                                && e["session_id"] == data["change"]["session_id"]
                                && e["turn_id"] == data["change"]["turn_id"]
                                && matches!(
                                    e["status"].as_str(),
                                    Some(
                                        "awaiting_acceptance"
                                            | "waiting_confirmation"
                                            | "completed"
                                    )
                                )
                        })
                    })
            });
        // Give the immediately following terminal/result commit a bounded
        // chance to join this page. Never consume an unpaired stop or hold a
        // later attention event behind it. SSE/audit keep the original event.
        if !stop_result
            && data["change"]["kind"] == "execution.stopped"
            && data["change"]["turn_id"].is_string()
            && event["id"] == events.last().unwrap()["id"]
            && event["time"]
                .as_u64()
                .is_some_and(|time| observed_at >= time && observed_at < time.saturating_add(400))
        {
            break;
        }
        if stop_result
            || (data["change"]["kind"] == "workflow.changed"
                && data["change"]["changes"].as_array().is_some_and(|changes| {
                    !changes.is_empty()
                        && changes.iter().all(|change| {
                            (change["kind"] == "execution"
                                && ((change["metadata_only"] == true
                                    && change["execution"]["status"] == "running")
                                    || delegated(change)))
                                || (change["kind"] == "status"
                                    && change["value"] == "running"
                                    && changes.iter().any(delegated))
                        })
                }))
        {
            if items.is_empty() {
                consumed = event["id"]
                    .as_u64()
                    .ok_or_else(|| err("storage", "经理事件缺少原始游标"))?;
            }
            continue;
        }
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
    Ok((items, consumed))
}

#[cfg(test)]
#[test]
fn manager_metadata_coalescing_keeps_boundaries_and_advances_bounded_pages() {
    fn event(id: u64, change: Value) -> Value {
        json!({"id":id,"detail":json!({"root_instance_id":"root","change":change}).to_string()})
    }
    let metadata = |id| {
        event(
            id,
            json!({"kind":"workflow.changed","changes":[{"kind":"execution","metadata_only":true,"execution":{"status":"running"}}]}),
        )
    };
    let mut cursor = 0;
    for page in (1..=70).map(metadata).collect::<Vec<_>>().chunks(32) {
        let (items, consumed) = native_event_items(page, "root", cursor, None).unwrap();
        assert!(items.is_empty());
        assert!(consumed > cursor);
        cursor = consumed;
    }
    assert_eq!(cursor, 70);
    let human = event(
        72,
        json!({"kind":"human.confirmed","confirmation_ref":"actual"}),
    );
    let (items, consumed) = native_event_items(
        &[metadata(71), human.clone(), metadata(73)],
        "root",
        cursor,
        None,
    )
    .unwrap();
    assert_eq!(consumed, 71);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], 72);
    assert_eq!(
        native_event_items(&[metadata(73)], "root", 72, None)
            .unwrap()
            .1,
        73
    );
    for change in [
        json!({"kind":"decision.changed","status":"approved"}),
        json!({"kind":"execution.stopped"}),
        json!({"kind":"workflow.changed","changes":[{"kind":"execution","metadata_only":true,"execution":{"status":"waiting_confirmation"}}]}),
        json!({"kind":"workflow.changed","changes":[{"kind":"execution","metadata_only":true,"execution":{"status":"running"}},{"kind":"paused","value":true}]}),
        json!({"kind":"workflow.changed","changes":[{"kind":"execution","execution":{"status":"running"}}]}),
    ] {
        let (items, consumed) = native_event_items(&[event(74, change)], "root", 73, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(consumed, 73);
    }
    assert!(native_event_items(&[human], "foreign", 0, None).is_err());
}

#[cfg(test)]
#[test]
fn delegated_notifications_keep_foreign_epochs_and_every_attention_boundary() {
    let progress = |status: &str| json!({"kind":"execution","execution":{"status":status},"delegated_progress":{"controller_id":"manager","epoch":2,"segment_id":"grant"}});
    let event = |id, changes: Value| json!({"id":id,"detail":json!({"root_instance_id":"root","change":{"kind":"workflow.changed","changes":changes}}).to_string()});
    for status in ["running", "awaiting_acceptance", "completed"] {
        let raw = event(1, json!([progress(status)]));
        let (items, cursor) =
            native_event_items(std::slice::from_ref(&raw), "root", 0, Some(("manager", 2)))
                .unwrap();
        assert!(items.is_empty());
        assert_eq!(cursor, 1);
        for actor in [None, Some(("manager", 1)), Some(("other", 2))] {
            assert_eq!(
                native_event_items(std::slice::from_ref(&raw), "root", 0, actor)
                    .unwrap()
                    .0
                    .len(),
                1
            );
        }
    }
    for status in [
        "failed",
        "waiting_input",
        "waiting_confirmation",
        "cancelled",
        "needs_reconciliation",
    ] {
        assert_eq!(
            native_event_items(
                &[event(2, json!([progress(status)]))],
                "root",
                0,
                Some(("manager", 2))
            )
            .unwrap()
            .0
            .len(),
            1
        );
    }
    for boundary in [
        json!({"kind":"paused"}),
        json!({"kind":"active_plan_revision_id"}),
        json!({"kind":"control_version"}),
        json!({"kind":"execution","execution":{"status":"completed"}}),
    ] {
        let (items, cursor) = native_event_items(
            &[event(2, json!([progress("running"), boundary]))],
            "root",
            0,
            Some(("manager", 2)),
        )
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(cursor, 0);
    }
    for kind in [
        "human.confirmed",
        "decision.changed",
        "segment.changed",
        "execution.stopped",
    ] {
        let raw = json!({"id":2,"detail":json!({"root_instance_id":"root","change":{"kind":kind}}).to_string()});
        assert_eq!(
            native_event_items(&[raw], "root", 0, Some(("manager", 2)))
                .unwrap()
                .0
                .len(),
            1
        );
    }
}

#[cfg(test)]
#[test]
fn stop_result_batching_is_exact_and_an_unpaired_stop_always_expires() {
    let event = |id, change: Value| json!({"id":id,"time":1000,"detail":json!({"root_instance_id":"root","instance_id":"instance","change":change}).to_string()});
    let stop = event(
        1,
        json!({"kind":"execution.stopped","session_id":"session","turn_id":"turn","operation_id":"worker"}),
    );
    let result = |status: &str, turn: &str| {
        event(
            2,
            json!({"kind":"workflow.changed","changes":[{"kind":"execution","execution":{"status":status,"session_id":"session","turn_id":turn}}]}),
        )
    };
    let (items, cursor) =
        native_event_items_at(std::slice::from_ref(&stop), "root", 0, None, 1399).unwrap();
    assert!(items.is_empty());
    assert_eq!(cursor, 0, "a deferred stop must not be acknowledged");
    assert_eq!(
        native_event_items_at(std::slice::from_ref(&stop), "root", 0, None, 1400)
            .unwrap()
            .0
            .len(),
        1
    );
    for status in ["awaiting_acceptance", "waiting_confirmation", "completed"] {
        let (items, cursor) = native_event_items_at(
            &[stop.clone(), result(status, "turn")],
            "root",
            0,
            None,
            1050,
        )
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], 2);
        assert_eq!(cursor, 1);
    }
    for status in [
        "failed",
        "waiting_input",
        "cancelled",
        "needs_reconciliation",
    ] {
        assert_eq!(
            native_event_items_at(
                &[stop.clone(), result(status, "turn")],
                "root",
                0,
                None,
                1050
            )
            .unwrap()
            .0
            .len(),
            2
        );
    }
    assert_eq!(
        native_event_items_at(
            &[stop.clone(), result("completed", "other-turn")],
            "root",
            0,
            None,
            1050
        )
        .unwrap()
        .0
        .len(),
        2
    );
    let control = event(
        2,
        json!({"kind":"workflow.changed","changes":[{"kind":"paused","value":true}]}),
    );
    assert_eq!(
        native_event_items_at(&[stop, control], "root", 0, None, 1050)
            .unwrap()
            .0
            .len(),
        2
    );
}
