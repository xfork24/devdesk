use crate::controller_events::{pending_events, wait};
use crate::governance::{record, update_record};
use crate::*;

fn row(kind: &str, id: &str, data: Value) -> ConfigurationRecord {
    let mut r = record(
        kind,
        "root".into(),
        Scope::global(),
        id.into(),
        1,
        None,
        data,
        false,
    );
    r.id = id.into();
    r
}
fn state() -> EngineeringState {
    let mut s = EngineeringState::default();
    s.governance.push(row("workflow_instance","root",json!({"root_instance_id":"root","steps":[{"id":"execution","step":"work","status":"queued","outputs":{}}]})));
    let c: WorkflowController = serde_json::from_value(json!({
        "project_id":"project","owner_id":"owner","root_instance_id":"root",
        "agent_revision_id":"agent:1","role_ref":"role:1","harness_permission_ref":"profile:1",
        "policy":WorkflowControlPolicy::default(),"policy_digest":"","session_id":null,
        "epoch":1,"status":"active","event_cursor":1,"wakeup_turns":0,"reason":"",
        "delivery_id":"delivery","bootstrap_epoch":1
    }))
    .unwrap();
    s.governance
        .push(row("workflow_controller", "controller", json!(c)));
    s
}
fn args() -> Value {
    json!({"delivery_id":"delivery","summary":"等待实际执行结果","waiting_for":["root"]})
}
fn notice(s: &mut EngineeringState, root: &str, origin: Value, cause: Value) {
    event(s,"root","workflow.runtime",&json!({"root_instance_id":root,"origin_turn_id":origin,"cause_call_id":cause,"change":{"kind":"execution.stopped"}}).to_string());
}
#[test]
fn waiting_checks_existing_events_atomically_and_cannot_hide_foreign_results() {
    let mut s = state();
    notice(&mut s, "another", Value::Null, Value::Null);
    assert_eq!(
        wait(&mut s, "controller", "turn", &args()).unwrap()["status"],
        "waiting"
    );
    notice(&mut s, "root", Value::Null, Value::Null);
    assert_eq!(
        wait(&mut s, "controller", "turn", &args()).unwrap()["status"],
        "pending_events"
    );
    assert_eq!(
        s.configuration("controller").unwrap().data["source_cursor"],
        0
    );
}
#[test]
fn waiting_then_new_result_is_durable_and_visible_without_an_instruction() {
    let mut s = state();
    wait(&mut s, "controller", "turn", &args()).unwrap();
    let saved = serde_json::to_string(&s).unwrap();
    let mut resumed: EngineeringState = serde_json::from_str(&saved).unwrap();
    notice(&mut resumed, "root", Value::Null, Value::Null);
    let c: WorkflowController = resumed
        .configuration("controller")
        .unwrap()
        .decode()
        .unwrap();
    let events = pending_events(&resumed, &c);
    assert_eq!(events.len(), 1);
    assert!(events[0]["instructions"].is_null());
    assert_eq!(c.checkpoint["summary"], "等待实际执行结果");
}
#[test]
fn only_the_exact_delivered_native_receipt_can_coalesce_an_own_action() {
    let mut s = state();
    notice(&mut s, "root", json!("turn"), json!("call"));
    s.governance.push(row(
        "controller_tool_reply",
        "reply",
        json!({"status":"delivered"}),
    ));
    assert_eq!(
        wait(&mut s, "controller", "turn", &args()).unwrap()["pending"],
        1
    );
    s.governance.last_mut().unwrap().definition_id = "call".into();
    assert_eq!(
        wait(&mut s, "controller", "turn", &args()).unwrap()["pending"],
        0
    );
    assert_eq!(
        s.configuration("controller").unwrap().data["acknowledged_cursor"],
        1
    );
    notice(&mut s, "root", Value::Null, Value::Null);
    notice(&mut s, "root", json!("turn"), json!("call"));
    assert_eq!(
        wait(&mut s, "controller", "turn", &args()).unwrap()["pending"],
        2
    );
}
#[test]
fn stale_deliveries_and_foreign_wait_objects_are_rejected() {
    let mut s = state();
    let mut input = args();
    input["delivery_id"] = json!("old");
    assert!(wait(&mut s, "controller", "turn", &input).is_err());
    s.governance.push(row(
        "workflow_instance",
        "foreign",
        json!({"root_instance_id":"foreign"}),
    ));
    input = args();
    input["waiting_for"] = json!(["foreign"]);
    assert!(wait(&mut s, "controller", "turn", &input).is_err());
}
#[test]
fn event_backlog_pages_by_bytes_without_skipping_other_root_boundaries() {
    let mut s = state();
    for index in 0..60 {
        event(&mut s,"root","workflow.runtime",&json!({"root_instance_id":"root","instance_id":"root","change":{"kind":"workflow.changed","changes":[{"reason":"中".repeat(1300)}]}}).to_string());
        if index % 5 == 0 {
            notice(&mut s, "another-root", Value::Null, Value::Null);
        }
    }
    let mut controller: WorkflowController =
        s.configuration("controller").unwrap().decode().unwrap();
    let mut delivered = std::collections::BTreeSet::new();
    let mut pages = 0;
    loop {
        let batch = pending_events(&s, &controller);
        if batch.is_empty() {
            break;
        }
        assert!(batch.len() <= 32);
        assert!(serde_json::to_vec(&batch).unwrap().len() <= 64 * 1024);
        for item in &batch {
            assert_eq!(item["source"]["root_instance_id"], "root");
            assert!(delivered.insert(item["id"].as_u64().unwrap()));
        }
        controller.source_cursor = batch.last().unwrap()["id"].as_u64().unwrap();
        controller = serde_json::from_str(&serde_json::to_string(&controller).unwrap()).unwrap();
        pages += 1;
    }
    assert!(pages > 2);
    assert_eq!(delivered.len(), 60);
}

#[test]
fn oversized_event_keeps_exact_source_and_output_progress_without_copying_details() {
    let mut s = state();
    event(&mut s,"root","workflow.runtime",&json!({"root_instance_id":"root","instance_id":"root","source_revision":9,"change":{"kind":"workflow.changed","changes":[{"execution":{"execution_id":"execution","step":"work","status":"awaiting_acceptance","outputs":{"result":"sealed"}},"reason":"x".repeat(100_000)}]}}).to_string());
    let controller: WorkflowController = s.configuration("controller").unwrap().decode().unwrap();
    let batch = pending_events(&s, &controller);
    assert_eq!(batch.len(), 1);
    assert!(serde_json::to_vec(&batch).unwrap().len() < 16 * 1024);
    assert_eq!(batch[0]["source"]["source_revision"], 9);
    assert_eq!(batch[0]["source"]["change"]["record_ref"], "root");
    assert_eq!(batch[0]["source"]["change"]["details_omitted"], true);
    assert_eq!(
        batch[0]["source"]["change"]["executions"][0]["execution_id"],
        "execution"
    );
    assert_eq!(batch[0]["source"]["change"]["has_outputs"], true);
    assert!(s.events[0].detail.len() > 100_000);
}
#[test]
fn commit_events_ignore_accounting_and_checkpoint_updates_but_record_actual_results() {
    let mut s = state();
    let before = s.clone();
    s.governance[0].data["spent_ms"] = json!(123);
    s.governance[0].revision += 1;
    append_workflow_events(&before, &mut s);
    assert!(s.events.is_empty());
    let before = s.clone();
    let mut data = s.governance[0].data.clone();
    data["steps"][0]["status"] = json!("awaiting_acceptance");
    data["steps"][0]["outputs"] = json!({"result":"sealed-output"});
    update_record(&mut s.governance[0], data);
    append_workflow_events(&before, &mut s);
    assert_eq!(s.events.len(), 1);
    let observed = s.clone();
    append_workflow_events(&observed, &mut s);
    assert_eq!(s.events.len(), 1);
    let event: Value = serde_json::from_str(&s.events[0].detail).unwrap();
    assert_eq!(
        event["change"]["changes"][0]["execution"]["outputs"]["result"],
        "sealed-output"
    );
}
#[test]
fn interaction_requires_actual_outputs_and_freezes_context_links() {
    let mut s = state();
    s.governance[0].data["steps"][0] = json!({"id":"execution","input_digest":"digest","session_id":"review-session","turn_id":"review-turn","outputs":{"result":"report"}});
    s.governance.push(row(
        "workflow_output",
        "report",
        json!({"execution_id":"execution","input_digest":"digest"}),
    ));
    let reference = json!({"label":"审核报告","purpose":"本轮独立初审","instance_id":"root","execution_id":"execution","output_id":"report"});
    let mut interaction = json!({"title":"确认本轮材料","background":"初审已返回","requested_action":"请核对需求范围","completed_work":"已安排独立初审","review_summary":"查看报告中的实际结论","materials":[reference],"evidence":[reference]});
    let arguments = json!({"execution_id":"execution","outputs":{"result":"report"}});
    let normalized =
        crate::controller_interactions::validate(&s, &s.governance[0], &interaction, &arguments)
            .unwrap();
    assert_eq!(normalized["evidence"][0]["session_id"], "review-session");
    assert_eq!(normalized["evidence"][0]["turn_id"], "review-turn");
    interaction["materials"] = json!([]);
    assert!(
        crate::controller_interactions::validate(&s, &s.governance[0], &interaction, &arguments)
            .is_err()
    );
    interaction["materials"] = json!([reference]);
    interaction["materials"][0]["output_id"] = json!("missing");
    assert!(
        crate::controller_interactions::validate(&s, &s.governance[0], &interaction, &arguments)
            .is_err()
    );
    interaction["materials"] = json!([reference]);
    interaction["evidence"] = json!([]);
    assert!(
        crate::controller_interactions::validate(&s, &s.governance[0], &interaction, &arguments)
            .is_err()
    );
}
