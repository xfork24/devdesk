//! Opt-in recovery regression on a credential-free COPY of a real failed scene.
//! This fixture controls its offline workflow explicitly; it is not evidence of
//! autonomous manager recovery and must never point at a live dataset.
use devdesk_adapters::{FileArtifacts, SqliteStore, integrations::NativeTools};
use devdesk_core::*;
use devdesk_domain::content_digest;
use serde_json::{Value, json};
use std::sync::Arc;
#[path = "../../../tests/support/control.rs"]
mod control;
#[path = "../../../tests/support/runtime.rs"]
mod runtime;

#[test]
#[ignore = "requires an explicitly isolated real publication-timeout dataset copy"]
fn completed_publication_retry_crosses_real_sqlite_and_platform_admission() {
    let path = std::path::PathBuf::from(std::env::var("DEVDESK_PUBLICATION_RETRY_COPY").unwrap());
    assert!(path.join(".performance-copy").is_file());
    assert!(!path.join("credentials").exists());
    let project = std::env::var("DEVDESK_PUBLICATION_RETRY_PROJECT").unwrap();
    let store = Arc::new(SqliteStore::open_project(&path, &project).unwrap());
    let engine = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(store.root().into())),
        runtime::adapters(),
    )
    .with_tool_executor(Arc::new(NativeTools::new(&path).unwrap()));
    let before = store.snapshot().unwrap();
    let root = before
        .governance
        .iter()
        .find(|r| {
            r.r#type == "workflow_instance"
                && r.data["parent_execution_id"].is_null()
                && r.data["steps"].as_array().is_some_and(|steps| {
                    steps
                        .iter()
                        .any(|s| s["step"] == "publish" && s["retry_of"].is_string())
                })
        })
        .unwrap()
        .id
        .clone();
    let previous = before.configuration(&root).unwrap().data["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["step"] == "publish")
        .unwrap()
        .clone();
    let original_id = previous["retry_of"].as_str().unwrap().to_owned();
    let original_call = before.configuration(&original_id).unwrap().clone();
    assert_eq!(original_call.data["status"], "succeeded");
    assert_eq!(original_call.data["remote_effect"], "applied");
    let original_history = before
        .governance
        .iter()
        .find(|r| r.r#type == "workflow_execution_history" && r.definition_id == original_id)
        .unwrap()
        .clone();
    // Fault-boundary setup only: the intervening exact-lease rejection made no
    // remote change. Do not manufacture a successful receipt for that attempt.
    store.seed_fixture_state(None, &mut |state| {
        for row in &mut state.governance {
            if row.r#type == "workflow_driver" && row.definition_id == root {
                row.data["mode"] = json!("manual");
                row.data["controller_id"] = Value::Null;
                row.revision += 1;
            }
            if row.r#type == "workflow_controller" && row.data["root_instance_id"] == root {
                row.data["status"] = json!("closed");
                row.revision += 1;
            }
            if row.id == previous["id"] {
                row.data["status"] = json!("failed");
                row.data["remote_effect"] = json!("not_applied");
                row.revision += 1;
            }
            if row.id == root {
                row.data["status"] = json!("running");
                let step = row.data["steps"].as_array_mut().unwrap().iter_mut()
                    .find(|s| s["id"] == previous["id"]).unwrap();
                step["status"] = json!("cancelled");
                step["finished"] = json!(previous["started"].as_i64().unwrap() + 1);
                step["failure"] = json!({"kind":"technical","effect":"not_applied","code":"exact_lease_rejected","message":"Offline fixture: no remote write occurred","source_ref":previous["id"]});
                row.revision += 1;
            }
            row.content_hash = content_digest(&row.refs, &row.data);
        }
        Ok(Value::Null)
    }).unwrap();
    let retried = control::command(
        &engine,
        "workflow.step.retry",
        &root,
        json!({"execution_id":previous["id"],"reason":"offline regression fixture"}),
    )
    .unwrap();
    let replacement = retried["execution"]["id"].as_str().unwrap();
    assert_ne!(replacement, previous["id"].as_str().unwrap());
    assert_eq!(retried["execution"]["retry_of"], previous["id"]);
    control::command(
        &engine,
        "workflow.step.dispatch",
        &root,
        json!({"execution_id":replacement,"reason":"offline regression fixture"}),
    )
    .unwrap();
    let intent = engine
        .pending_workflow_intents()
        .unwrap()
        .into_iter()
        .find(|r| r.definition_id == replacement && r.data["method"] == "workflow.tool.execute")
        .unwrap();
    let command: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
    let prepared = engine
        .prepare_configured_platform(&command)
        .unwrap()
        .unwrap();
    let result = engine.enqueue_prepared_platform_tool(&command, prepared.request(), &prepared);
    engine.observe_workflow_intent(&intent, &result).unwrap();
    result.unwrap();
    for _ in 0..12 {
        engine.drive_platform_tools().unwrap();
        engine.evaluate_workflows().unwrap();
    }
    let after = store.snapshot().unwrap();
    let call = after.configuration(replacement).unwrap();
    assert_eq!(call.data["status"], "succeeded", "{}", call.data);
    assert_eq!(call.data["receipt"], original_call.data["receipt"]);
    let effect = after
        .configuration(call.data["effect_id"].as_str().unwrap())
        .unwrap();
    assert!(effect.data["plan"]["action"]["ReusedPublication"].is_object());
    assert_eq!(
        after.configuration(&original_id).unwrap().data,
        original_call.data
    );
    assert_eq!(
        after.configuration(&original_history.id).unwrap().data,
        original_history.data
    );
    let step = after.configuration(&root).unwrap().data["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == replacement)
        .unwrap();
    assert_eq!(step["status"], "awaiting_acceptance");
    println!(
        "PUBLICATION_RETRY_OFFLINE={}",
        json!({"root":root,"new_execution":replacement,"original_physical_operation":original_id,"receipt_reused":true,"git_or_github_ports_available":false,"original_cancellation_preserved":true})
    );
}
