//! Explicit offline migration check against an isolated credential-free copy.
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
use devdesk_adapters::{FileArtifacts, SqliteStore};
use devdesk_core::*;
use serde_json::{Value, json};
use std::sync::Arc;
fn view(e: &Engine, project: &str, instance: &str) -> Value {
    e.query(
        "query.workflow_instance",
        &json!({"project_id":project,"id":instance}),
        1,
    )
    .unwrap()
}
fn counters(path: &std::path::Path) -> Vec<(String, String)> {
    let c = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap();
    let mut q=c.prepare("SELECT id,content_hash FROM governance_records WHERE type='workflow_rework_budget' ORDER BY id").unwrap();
    q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}
#[test]
#[ignore = "requires an explicitly isolated copy of an existing candidate-branch delivery"]
fn existing_delivery_republishes_only_transport_and_invalidates_old_approval() {
    let path = std::path::PathBuf::from(std::env::var("DEVDESK_DELIVERY_MIGRATION_COPY").unwrap());
    assert!(path.join(".delivery-migration-copy").is_file());
    assert!(!path.join("credentials").exists());
    let pid = std::env::var("DEVDESK_DELIVERY_MIGRATION_PROJECT").unwrap();
    let root = std::env::var("DEVDESK_DELIVERY_MIGRATION_ROOT").unwrap();
    let store = Arc::new(SqliteStore::open_project(&path, &pid).unwrap());
    let e = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(store.root().into())),
        runtime::adapters(),
    );
    let before = view(&e, &pid, &root);
    let child_id = before["children"][0]["id"].as_str().unwrap();
    let child = view(&e, &pid, child_id);
    let original_steps = before["runtime"]["steps"].as_array().unwrap().clone();
    let published = original_steps
        .iter()
        .find(|s| s["step"] == "publication")
        .unwrap();
    let selected = store
        .read_resources(&[devdesk_core::concurrency::ResourceKey::new(
            "governance",
            format!("workflow-delivery:{root}"),
        )])
        .unwrap();
    let publication = selected[0].value.as_ref().unwrap()["data"]["publication"].clone();
    let database = std::path::Path::new(&store.root()).join("engineering.sqlite3");
    let budgets = counters(&database);
    e.execute("workflow.pause",Command{command_id:id(),target:Some(root.clone()),expected_revision:before["record"]["revision"].as_u64(),payload:json!({"plan_revision_id":before["runtime"]["active_plan_revision_id"],"control_version":before["runtime"]["control_version"]})}).unwrap();
    let paused = view(&e, &pid, &root);
    let command = Command {
        command_id: id(),
        target: Some(root.clone()),
        expected_revision: paused["record"]["revision"].as_u64(),
        payload: json!({
        "plan_revision_id":paused["runtime"]["active_plan_revision_id"],"control_version":paused["runtime"]["control_version"],
        "expected_step_revision":paused["step_revisions"][published["id"].as_str().unwrap()],
        "expected_pr_number":publication["pull_request"]["number"],"expected_head":publication["pull_request"]["head"],"expected_candidate":publication["revision"],
        "execution_id":published["id"],"reason":"Offline explicit transport migration; no business approval"}),
    };
    let mut stale = command.clone();
    stale.command_id = id();
    stale.payload["expected_head"] = json!("0".repeat(40));
    assert!(e.execute("workflow.delivery.republish", stale).is_err());
    let result = e
        .execute("workflow.delivery.republish", command.clone())
        .unwrap();
    assert_eq!(
        result,
        e.execute("workflow.delivery.republish", command).unwrap()
    );
    let after = view(&e, &pid, &root);
    assert_eq!(
        after["runtime"]["active_plan_revision_id"],
        before["runtime"]["active_plan_revision_id"]
    );
    assert_eq!(after["runtime"]["owner_id"], before["runtime"]["owner_id"]);
    assert_eq!(after["runtime"]["paused"], true);
    assert_eq!(
        view(&e, &pid, child_id)["record"]["content_hash"],
        child["record"]["content_hash"]
    );
    let affected = result["affected"].as_array().unwrap();
    assert_eq!(affected.len(), 4);
    for original in &original_steps {
        let now = after["runtime"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["step"] == original["step"])
            .unwrap();
        if affected.contains(&original["step"]) {
            assert_ne!(now["id"], original["id"]);
            assert_eq!(now["status"], "queued");
        } else {
            assert_eq!(now["id"], original["id"]);
            assert_eq!(now["outputs"], original["outputs"]);
            assert_eq!(
                after["step_revisions"][now["id"].as_str().unwrap()],
                before["step_revisions"][original["id"].as_str().unwrap()]
            );
        }
    }
    assert_eq!(counters(&database), budgets);
    let summary = e
        .query(
            "query.orchestration",
            &json!({"root_instance_id":root,"project_id":pid}),
            1,
        )
        .unwrap();
    assert!(
        summary["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["data"]["status"] != "pending" || r["current"] == false)
    );
}
