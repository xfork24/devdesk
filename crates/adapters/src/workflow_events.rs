//! Runtime notifications are committed with the exact changed resources. This
//! reads only affected instances; it never restores an EngineeringState.
use crate::*;
use devdesk_core::concurrency::PreparedChangeSet;
use rusqlite::{OptionalExtension, params};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct Observed {
    instances: BTreeMap<String, Value>,
    records: BTreeMap<String, Option<Value>>,
    stopped: BTreeMap<String, (String, String, Value)>,
}
fn row(conn: &Connection, id: &str) -> Result<Option<Value>> {
    Ok(load_table_filtered_params::<Value>(conn, "governance_records", "id=?1", &[&id])?.pop())
}
fn session_instances(conn: &Connection, session: &str) -> Result<Vec<String>> {
    let mut q=conn.prepare_cached("SELECT DISTINCT i.id FROM governance_records i,json_each(i.data,'$.steps') s WHERE i.type='workflow_instance' AND json_extract(s.value,'$.session_id')=?1").map_err(storage)?;
    q.query_map([session], |r| r.get(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)
}
fn record_instance(conn: &Connection, record: &Value) -> Result<Option<String>> {
    match record["type"].as_str() {
        Some("workflow_instance") => Ok(record["id"].as_str().map(str::to_owned)),
        Some("workflow_pending_decision" | "workflow_segment" | "workflow_change") => Ok(record["definition_id"].as_str().map(str::to_owned)),
        Some("workflow_reconciliation") => Ok(record["data"]["instance_id"].as_str().map(str::to_owned)),
        Some("workflow_confirmation") => conn.query_row("SELECT i.id FROM governance_records i,json_each(i.data,'$.steps') s WHERE i.type='workflow_instance' AND json_extract(s.value,'$.id')=?1 LIMIT 1",[record["definition_id"].as_str().unwrap_or("")],|r|r.get(0)).optional().map_err(storage),
        _=>Ok(None),
    }
}
pub(super) fn capture(conn: &Connection, prepared: &PreparedChangeSet) -> Result<Observed> {
    let mut seen = Observed::default();
    let mut ids = BTreeSet::new();
    for change in &prepared.changes {
        let key = &change.resource;
        match key.kind.as_str() {
            "workflow_header" => {
                ids.insert(key.id.clone());
            }
            kind if kind.starts_with("workflow_step") => {
                let (id, _): (String, String) = serde_json::from_str(&key.id).map_err(storage)?;
                ids.insert(id);
            }
            "governance"
                if matches!(
                    change.value["type"].as_str(),
                    Some(
                        "workflow_instance"
                            | "workflow_pending_decision"
                            | "workflow_confirmation"
                            | "workflow_segment"
                            | "workflow_reconciliation"
                            | "workflow_change"
                    )
                ) =>
            {
                seen.records.insert(key.id.clone(), row(conn, &key.id)?);
                if let Some(id) = record_instance(conn, &change.value)? {
                    ids.insert(id);
                }
            }
            "execution" if change.value["writer_revoked"] == true => {
                let previous =
                    load_table_filtered_params::<Value>(conn, "executions", "id=?1", &[&key.id])?
                        .pop();
                if previous.is_some_and(|r| r["writer_revoked"] == false) {
                    let session = change.value["session_id"].as_str().unwrap_or("");
                    let turn = prepared
                        .changes
                        .iter()
                        .find(|c| {
                            c.value["type"] == "worker_stop_intent"
                                && c.value["data"]["worker_operation_id"]
                                    == change.value["operation_id"]
                                && c.value["data"]["session_id"] == session
                        })
                        .map(|c| c.value["data"]["turn_id"].clone())
                        .unwrap_or(Value::Null);
                    for id in session_instances(conn, session)? {
                        ids.insert(id.clone());
                        seen.stopped.insert(
                            id,
                            (
                                session.into(),
                                change.value["operation_id"].as_str().unwrap_or("").into(),
                                turn.clone(),
                            ),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    for id in ids {
        if let Some(value) = row(conn, &id)? {
            seen.instances.insert(id, value);
        }
    }
    Ok(seen)
}
fn step(value: &Value) -> Value {
    json!({"execution_id":value["id"],"step":value["step"],"status":value["status"],"outputs":value["outputs"],"input_digest":value["input_digest"],"session_id":value["session_id"],"turn_id":value["turn_id"],"reason":value["reason"].as_str().map(|v|v.chars().take(512).collect::<String>())})
}
fn metadata_only(previous: &Value, current: &Value) -> bool {
    if previous["status"] != "running" || current["status"] != "running" {
        return false;
    }
    let mut previous = step(previous);
    let mut current = step(current);
    for key in ["session_id", "turn_id"] {
        previous.as_object_mut().unwrap().remove(key);
        current.as_object_mut().unwrap().remove(key);
    }
    previous == current
}

// Advisory notification scope, sampled inside the event's transaction. The
// original event remains available to SSE/audit; this never authorizes a step.
fn delegated_progress(conn: &Connection, instance: &Value) -> Result<BTreeMap<String, Value>> {
    let grants = load_table_filtered_params::<Value>(
        conn,
        "governance_records",
        "id IN (SELECT id FROM governance_records WHERE type='workflow_segment' AND definition_id=?1 AND json_extract(data,'$.status')='active' ORDER BY id LIMIT 65)",
        &[&instance["id"].as_str().unwrap_or("")],
    )?;
    if grants.is_empty()
        || grants.len() > 64
        || instance["data"]["paused"] == true
        || instance["data"]["cancel_requested"] == true
    {
        return Ok(BTreeMap::new());
    }
    // Only the frozen graph is needed, never the expanded source closure.
    let graph: Option<String> = conn.query_row(
        "SELECT coalesce(json_extract(i.data,'$.plan.document.steps'),json_extract(w.data,'$.steps')) FROM governance_records i LEFT JOIN source_definitions w ON w.reference=coalesce(json_extract(i.data,'$.plan.workflow_ref'),json_extract(i.data,'$.plan.workflow_revision_id')) AND w.type='workflow' WHERE i.id=?1",
        [instance["id"].as_str().unwrap_or("")], |r|r.get(0)).map_err(storage)?;
    let Some(graph) = graph else {
        return Ok(BTreeMap::new());
    };
    let graph: Vec<Value> = serde_json::from_str(&graph).map_err(storage)?;
    let mut hints = BTreeMap::new();
    for grant in grants {
        let data = &grant["data"];
        if data["actor"]["kind"] != "controller"
            || data["plan_revision_id"] != instance["data"]["active_plan_revision_id"]
        {
            continue;
        }
        let controller = row(conn, data["actor"]["controller_id"].as_str().unwrap_or(""))?;
        let driver = row(conn, data["driver_id"].as_str().unwrap_or(""))?;
        if !controller.as_ref().is_some_and(|c| {
            c["type"] == "workflow_controller"
                && c["data"]["status"] == "active"
                && c["data"]["epoch"] == data["actor"]["epoch"]
        }) || !driver.as_ref().is_some_and(|d| {
            d["type"] == "workflow_driver"
                && d["definition_id"] == instance["data"]["root_instance_id"]
                && d["data"]["mode"] == data["driver_mode"]
                && d["data"]["epoch"] == data["driver_epoch"]
        }) || data["operations"].as_u64().unwrap_or(u64::MAX)
            >= data["max_operations"].as_u64().unwrap_or(0)
        {
            continue;
        }
        let nodes = data["node_ids"].as_array().cloned().unwrap_or_default();
        let covered: BTreeSet<_> = instance["data"]["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| nodes.contains(&e["node_id"]))
            .filter_map(|e| e["step"].as_str())
            .collect();
        for execution in instance["data"]["steps"].as_array().into_iter().flatten() {
            if (data["include_human"] == true
                || !graph
                    .iter()
                    .any(|s| s["key"] == execution["step"] && s["confirmation"] == "human"))
                && delegated_execution_progress(execution, &graph, &covered)
            {
                hints.insert(execution["id"].as_str().unwrap_or("").to_owned(),
                    json!({"segment_id":grant["id"],"controller_id":data["actor"]["controller_id"],"epoch":data["actor"]["epoch"]}));
            }
        }
    }
    Ok(hints)
}

fn delegated_execution_progress(
    execution: &Value,
    graph: &[Value],
    covered: &BTreeSet<&str>,
) -> bool {
    let Some(key) = execution["step"]
        .as_str()
        .filter(|key| covered.contains(key))
    else {
        return false;
    };
    if !graph.iter().any(|s| s["key"] == key) {
        return false;
    }
    match execution["status"].as_str() {
        Some("running" | "awaiting_acceptance") => true,
        // An ungranted successor (including a Human step) needs the manager.
        Some("completed") => !graph.iter().any(|s| {
            !covered.contains(s["key"].as_str().unwrap_or(""))
                && s["dependencies"]
                    .as_array()
                    .is_some_and(|deps| deps.iter().any(|d| d == key))
        }),
        _ => false,
    }
}

#[cfg(test)]
#[test]
fn delegation_batches_only_success_within_the_exact_granted_graph() {
    let graph = vec![
        json!({"key":"one"}),
        json!({"key":"two","dependencies":["one"]}),
        json!({"key":"human","confirmation":"human","dependencies":["two"]}),
    ];
    let covered = BTreeSet::from(["one", "two"]);
    for status in ["running", "awaiting_acceptance", "completed"] {
        assert!(delegated_execution_progress(
            &json!({"step":"one","status":status}),
            &graph,
            &covered
        ));
    }
    assert!(!delegated_execution_progress(
        &json!({"step":"two","status":"completed"}),
        &graph,
        &covered
    ));
    assert!(!delegated_execution_progress(
        &json!({"step":"one","status":"completed"}),
        &graph,
        &BTreeSet::from(["one"])
    ));
    assert!(!delegated_execution_progress(
        &json!({"step":"unknown","status":"running"}),
        &graph,
        &BTreeSet::from(["unknown"])
    ));
    assert!(delegated_execution_progress(
        &json!({"step":"human","status":"running"}),
        &graph,
        &BTreeSet::from(["human"])
    ));
    assert!(!delegated_execution_progress(
        &json!({"step":"human","status":"waiting_confirmation"}),
        &graph,
        &BTreeSet::from(["human"])
    ));
    for status in [
        "failed",
        "waiting_input",
        "waiting_confirmation",
        "cancelled",
        "needs_reconciliation",
    ] {
        assert!(!delegated_execution_progress(
            &json!({"step":"one","status":status}),
            &graph,
            &covered
        ));
    }
}

#[cfg(test)]
#[test]
fn only_running_session_links_are_metadata_not_business_progress() {
    let before = json!({"id":"execution","step":"work","status":"running","outputs":{},"input_digest":"original","reason":"","session_id":null,"turn_id":null});
    let mut linked = before.clone();
    linked["session_id"] = json!("session");
    linked["turn_id"] = json!("turn");
    assert!(metadata_only(&before, &linked));
    for (key, value) in [
        ("status", json!("waiting_confirmation")),
        ("status", json!("failed")),
        ("reason", json!("permission required")),
        ("outputs", json!({"result":"output"})),
        ("input_digest", json!("changed")),
    ] {
        let mut changed = linked.clone();
        changed[key] = value;
        assert!(!metadata_only(&before, &changed), "{key}");
    }
    let mut queued = before.clone();
    queued["status"] = json!("queued");
    assert!(!metadata_only(&queued, &linked));
}
pub(super) fn append(
    conn: &Connection,
    prepared: &PreparedChangeSet,
    before: Observed,
) -> Result<()> {
    let context = prepared
        .changes
        .iter()
        .find(|c| c.value["type"] == "command_context");
    let actor = context
        .map(|c| c.value["data"]["context"]["actor"].clone())
        .unwrap_or(Value::Null);
    let cause = context
        .and_then(|c| c.value["definition_id"].as_str())
        .and_then(|id| id.strip_suffix(":action"));
    let mut ids: BTreeSet<_> = before.instances.keys().cloned().collect();
    for change in &prepared.changes {
        if change.resource.kind == "governance" && change.value["type"] == "workflow_instance" {
            ids.insert(change.resource.id.clone());
        }
    }
    let mut changes = Vec::new();
    for id in ids {
        let Some(current) = row(conn, &id)? else {
            continue;
        };
        let previous = before.instances.get(&id);
        let mut facts = Vec::new();
        for field in [
            "status",
            "paused",
            "cancel_requested",
            "active_plan_revision_id",
            "control_version",
        ] {
            if previous.is_none_or(|old| old["data"][field] != current["data"][field]) {
                facts.push(json!({"kind":field,"value":current["data"][field]}));
            }
        }
        let old: BTreeMap<_, _> = previous
            .into_iter()
            .flat_map(|r| r["data"]["steps"].as_array().into_iter().flatten())
            .filter_map(|s| s["id"].as_str().map(|id| (id, s)))
            .collect();
        let delegated = delegated_progress(conn, &current)?;
        for execution in current["data"]["steps"].as_array().into_iter().flatten() {
            if old
                .get(execution["id"].as_str().unwrap_or(""))
                .is_none_or(|s| step(s) != step(execution))
            {
                let mut fact = step(execution);
                let resource = json!([id, execution["id"]]).to_string();
                let revision:u64=conn.query_row("SELECT revision FROM resource_versions WHERE kind='workflow_step' AND resource_id=?1",[resource],|r|r.get(0)).map_err(storage)?;
                fact["step_revision"] = json!(revision);
                let mut change = json!({"kind":"execution","execution":fact,"metadata_only":old.get(execution["id"].as_str().unwrap_or("")).is_some_and(|previous|metadata_only(previous,execution))});
                if let Some(hint) = delegated.get(execution["id"].as_str().unwrap_or("")) {
                    change["delegated_progress"] = hint.clone();
                }
                facts.push(change);
            }
        }
        for batch in facts.chunks(16) {
            changes.push((
                current.clone(),
                json!({"kind":"workflow.changed","changes":batch}),
            ));
        }
        if let Some((session, operation, turn)) = before.stopped.get(&id) {
            changes.push((current, json!({"kind":"execution.stopped","session_id":session,"operation_id":operation,"turn_id":turn})));
        }
    }
    for change in &prepared.changes {
        if change.resource.kind != "governance" {
            continue;
        }
        let next = &change.value;
        let kind = next["type"].as_str().unwrap_or("");
        let old = before
            .records
            .get(&change.resource.id)
            .and_then(Option::as_ref);
        let fact = if kind == "workflow_pending_decision"
            && old.is_none_or(|r| r["data"]["status"] != next["data"]["status"])
        {
            let mut fact = json!({"kind":"decision.changed","decision_id":next["id"],"status":next["data"]["status"],"decision_ref":next["data"]["decision_ref"]});
            if let Some(reference) = next["data"].get("material_input_ref") {
                fact["material_input_ref"] = reference.clone();
            }
            Some(fact)
        } else if kind == "workflow_reconciliation"
            && old.is_none_or(|r| r["data"]["status"] != next["data"]["status"])
        {
            Some(
                json!({"kind":"execution.reconciled","reconciliation_id":next["id"],"execution_id":next["data"]["execution_id"],"status":next["data"]["status"],"error":next["data"]["error"]}),
            )
        } else if kind == "workflow_confirmation" && old.is_none() {
            Some(
                json!({"kind":"human.confirmed","execution_id":next["definition_id"],"confirmation_ref":next["id"]}),
            )
        } else if kind == "workflow_segment"
            && old.is_none_or(|r| r["data"]["status"] != next["data"]["status"])
        {
            Some(
                json!({"kind":"segment.changed","segment_id":next["id"],"status":next["data"]["status"],"reason":next["data"]["reason"],"plan_revision_id":next["data"]["plan_revision_id"],"operations":next["data"]["operations"],"max_operations":next["data"]["max_operations"]}),
            )
        } else if kind == "workflow_change"
            && old.is_none_or(|r| r["data"]["status"] != next["data"]["status"])
        {
            Some(
                json!({"kind":"plan.change","change_id":next["id"],"status":next["data"]["status"],"error":next["data"]["error"],"new_plan_revision_id":next["data"]["new_plan_revision_id"]}),
            )
        } else {
            None
        };
        if let Some(fact) = fact
            && let Some(id) = record_instance(conn, next)?
            && let Some(instance) = row(conn, &id)?
        {
            changes.push((instance, fact));
        }
    }
    for (instance, change) in changes {
        let detail = json!({"root_instance_id":instance["data"]["root_instance_id"],"instance_id":instance["id"],"source_revision":instance["revision"],"origin_turn_id":actor["turn_id"],"cause_call_id":cause,"change":change});
        conn.execute(
            "INSERT INTO events(target,kind,detail,time) VALUES(?1,'workflow.runtime',?2,?3)",
            params![
                instance["id"].as_str().unwrap_or(""),
                detail.to_string(),
                prepared.time
            ],
        )
        .map_err(storage)?;
    }
    Ok(())
}
