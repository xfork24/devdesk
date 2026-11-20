//! A successful native terminal and its admitted stop proof are ordinary progress.
//! Output admission can follow cleanup seconds later; it still performs all checks.
use crate::*;
use devdesk_core::concurrency::PreparedChangeSet;
use rusqlite::Connection;
use std::collections::BTreeMap;

fn successful_terminal(
    poll: &Value,
    execution: &Value,
    session: &str,
    operation: &str,
    turn: &Value,
) -> bool {
    poll["type"] == "provider_poll_observation"
        && poll["immutable"] == true
        && poll["data"]["basis"]["session_id"] == session
        && poll["data"]["basis"]["worker_operation_id"] == operation
        && poll["data"]["basis"]["turn_id"] == *turn
        && poll["data"]["turn"]["cancel_requested"] == false
        && poll["data"]["authority"]["workflow"]["execution_id"] == execution["id"]
        && poll["data"]["authority"]["workflow"]["input_digest"] == execution["input_digest"]
        && poll["data"]["authority"]["workflow"]["plan_revision_id"]
            == execution["plan_revision_id"]
        && poll["data"]["value"]["observations"]
            .as_array()
            .is_some_and(|items| {
                items.iter().any(|item| {
                    item["type"] == "terminal"
                        && item["turn"] == *turn
                        && item["outcome"] == "completed"
                        && item["quiescent"] == true
                })
            })
}

pub(super) fn normal_stop_hint(
    conn: &Connection,
    instance: &Value,
    prepared: &PreparedChangeSet,
    delegated: &BTreeMap<String, Value>,
    session: &str,
    operation: &str,
    turn: &Value,
) -> Result<Option<Value>> {
    let Some(execution) = instance["data"]["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|e| {
            e["session_id"] == session
                && e["turn_id"] == *turn
                && turn.is_string()
                && e["failure"].is_null()
                && matches!(
                    e["status"].as_str(),
                    Some("running" | "awaiting_acceptance" | "waiting_confirmation" | "completed")
                )
        })
    else {
        return Ok(None);
    };
    let Some(hint) = delegated
        .get(execution["id"].as_str().unwrap_or(""))
        .filter(|h| h["prepare_human_requests"] == true)
    else {
        return Ok(None);
    };
    let explicitly_stopped: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM governance_records WHERE type='workflow_stop' AND definition_id=?1)", [execution["id"].as_str().unwrap_or("")], |r|r.get(0)).map_err(storage)?;
    if explicitly_stopped {
        return Ok(None);
    }
    let Some(intent) = prepared.changes.iter().map(|c| &c.value).find(|r| {
        r["type"] == "worker_stop_intent"
            && r["data"]["status"] == "completed"
            && r["data"]["session_id"] == session
            && r["data"]["worker_operation_id"] == operation
            && r["data"]["turn_id"] == *turn
    }) else {
        return Ok(None);
    };
    for guard in intent["data"]["authority"]
        .as_array()
        .into_iter()
        .flatten()
        .take(16)
    {
        if guard["resource"]["kind"] != "governance" {
            continue;
        }
        let Some(id) = guard["resource"]["id"].as_str() else {
            continue;
        };
        let Some(poll) =
            load_table_filtered_params::<Value>(conn, "governance_records", "id=?1", &[&id])?.pop()
        else {
            continue;
        };
        if successful_terminal(&poll, execution, session, operation, turn) {
            let mut hint = hint.clone();
            hint["normal_native_stop"] = json!(true);
            return Ok(Some(hint));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delayed_output_admission_needs_exact_terminal_and_completed_stop_without_user_stop() {
        use devdesk_core::concurrency::{ReadSet, ResourceChange, ResourceKey};
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE governance_records(id TEXT,type TEXT,definition_id TEXT,immutable BOOLEAN,data JSON)").unwrap();
        let execution = json!({"id":"e","input_digest":"input","plan_revision_id":"plan","session_id":"s","turn_id":"t","status":"running"});
        let instance = json!({"data":{"steps":[execution]}});
        let poll = json!({"basis":{"session_id":"s","worker_operation_id":"op","turn_id":"t"},"turn":{"cancel_requested":false},
            "authority":{"workflow":{"execution_id":"e","input_digest":"input","plan_revision_id":"plan"}},
            "value":{"observations":[{"type":"terminal","turn":"t","outcome":"completed","quiescent":true}]}});
        conn.execute(
            "INSERT INTO governance_records VALUES('poll','provider_poll_observation','e',1,?1)",
            [poll.to_string()],
        )
        .unwrap();
        let mut prepared = PreparedChangeSet {
            command_id: "stop".into(),
            fingerprint: "fixture".into(),
            read_set: ReadSet::new(vec![]).unwrap(),
            changes: vec![ResourceChange {
                resource: ResourceKey::new("governance", "intent"),
                value: json!({"type":"worker_stop_intent","data":{
                "status":"pending","session_id":"s","worker_operation_id":"op","turn_id":"t","authority":[{"resource":{"kind":"governance","id":"poll"}}]}}),
            }],
            result: Value::Null,
            time: 1,
        };
        let delegated = BTreeMap::from([(
            "e".into(),
            json!({"controller_id":"manager","epoch":2,"prepare_human_requests":true}),
        )]);
        assert!(
            normal_stop_hint(
                &conn,
                &instance,
                &prepared,
                &delegated,
                "s",
                "op",
                &json!("t")
            )
            .unwrap()
            .is_none()
        );
        prepared.changes[0].value["data"]["status"] = json!("completed");
        assert_eq!(
            normal_stop_hint(
                &conn,
                &instance,
                &prepared,
                &delegated,
                "s",
                "op",
                &json!("t")
            )
            .unwrap()
            .unwrap()["normal_native_stop"],
            true
        );
        assert!(
            normal_stop_hint(
                &conn,
                &instance,
                &prepared,
                &BTreeMap::new(),
                "s",
                "op",
                &json!("t")
            )
            .unwrap()
            .is_none()
        );
        conn.execute(
            "INSERT INTO governance_records VALUES('human-stop','workflow_stop','e',1,'{}')",
            [],
        )
        .unwrap();
        assert!(
            normal_stop_hint(
                &conn,
                &instance,
                &prepared,
                &delegated,
                "s",
                "op",
                &json!("t")
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn terminal_hint_requires_the_exact_successful_quiescent_uncancelled_attempt() {
        let e = json!({"id":"e","input_digest":"input","plan_revision_id":"plan"});
        let poll = json!({"type":"provider_poll_observation","immutable":true,"data":{
            "basis":{"session_id":"s","worker_operation_id":"op","turn_id":"t"},"turn":{"cancel_requested":false},
            "authority":{"workflow":{"execution_id":"e","input_digest":"input","plan_revision_id":"plan"}},
            "value":{"observations":[{"type":"terminal","turn":"t","outcome":"completed","quiescent":true}]}}});
        assert!(successful_terminal(&poll, &e, "s", "op", &json!("t")));
        for (pointer, value) in [
            ("/data/turn/cancel_requested", json!(true)),
            ("/data/value/observations/0/outcome", json!("failed")),
            ("/data/value/observations/0/quiescent", json!(false)),
            ("/data/basis/worker_operation_id", json!("foreign")),
            ("/data/authority/workflow/input_digest", json!("stale")),
            ("/data/authority/workflow/plan_revision_id", json!("old")),
        ] {
            let mut changed = poll.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(
                !successful_terminal(&changed, &e, "s", "op", &json!("t")),
                "{pointer}"
            );
        }
    }
}
