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
    stopped: BTreeMap<String, (String, String)>,
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
        Some("workflow_pending_decision") => Ok(record["definition_id"].as_str().map(str::to_owned)),
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
                        "workflow_instance" | "workflow_pending_decision" | "workflow_confirmation"
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
                    for id in session_instances(conn, session)? {
                        ids.insert(id.clone());
                        seen.stopped.insert(
                            id,
                            (
                                session.into(),
                                change.value["operation_id"].as_str().unwrap_or("").into(),
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
        for execution in current["data"]["steps"].as_array().into_iter().flatten() {
            if old
                .get(execution["id"].as_str().unwrap_or(""))
                .is_none_or(|s| step(s) != step(execution))
            {
                let mut fact = step(execution);
                let resource = json!([id, execution["id"]]).to_string();
                let revision:u64=conn.query_row("SELECT revision FROM resource_versions WHERE kind='workflow_step' AND resource_id=?1",[resource],|r|r.get(0)).map_err(storage)?;
                fact["step_revision"] = json!(revision);
                facts.push(json!({"kind":"execution","execution":fact}));
            }
        }
        for batch in facts.chunks(16) {
            changes.push((
                current.clone(),
                json!({"kind":"workflow.changed","changes":batch}),
            ));
        }
        if let Some((session, operation)) = before.stopped.get(&id) {
            changes.push((
                current,
                json!({"kind":"execution.stopped","session_id":session,"operation_id":operation}),
            ));
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
            Some(
                json!({"kind":"decision.changed","decision_id":next["id"],"status":next["data"]["status"],"decision_ref":next["data"]["decision_ref"]}),
            )
        } else if kind == "workflow_confirmation" && old.is_none() {
            Some(
                json!({"kind":"human.confirmed","execution_id":next["definition_id"],"confirmation_ref":next["id"]}),
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
