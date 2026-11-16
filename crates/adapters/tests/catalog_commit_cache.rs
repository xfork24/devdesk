use devdesk_adapters::{GlobalStore, SqliteStore, performance};
use devdesk_core::{EngineeringStore, concurrency::*};
use serde_json::{Value, json};
use std::sync::Arc;
fn change(id: &str, kind: &str, key: &str, value: Value) -> PreparedChangeSet {
    let resource = ResourceKey::new(kind, key);
    PreparedChangeSet {
        command_id: id.into(),
        fingerprint: id.into(),
        read_set: ReadSet::new(vec![ResourceVersion {
            resource: resource.clone(),
            revision: 0,
        }])
        .unwrap(),
        changes: vec![ResourceChange { resource, value }],
        result: Value::Null,
        time: 1,
    }
}
#[test]
fn ordinary_resource_commits_preserve_the_source_view_and_source_commits_refresh_it() {
    let root = tempfile::tempdir().unwrap();
    let store = GlobalStore::open(root.path()).unwrap();
    let before = store.configuration_snapshot().unwrap();
    store
        .commit_changes(&change(
            "preference",
            "setting",
            "global_concurrency",
            json!(8),
        ))
        .unwrap();
    let trace = performance::Trace::start();
    let after = store.configuration_snapshot().unwrap();
    let trace = trace.finish();
    assert!(Arc::ptr_eq(&before, &after));
    assert!(trace["spans"]["catalog.build"].is_null());
    let source = ResourceKey::new("source", "cache-spec:1");
    let head = ResourceKey::new("source_head", "cache-spec");
    store.commit_changes(&PreparedChangeSet {
        command_id:"source-publication".into(),fingerprint:"source-publication".into(),
        read_set:ReadSet::new(vec![ResourceVersion {resource:source.clone(),revision:0},ResourceVersion {resource:head.clone(),revision:0}]).unwrap(),
        changes:vec![ResourceChange {resource:source,value:json!({"identity":"cache-spec","revision":1,"type":"spec","name":"Cache test","scope":"global","data":{"schema_version":3,"body":"Cache contract","checks":[]}})},ResourceChange {resource:head,value:json!({"reference":"cache-spec:1","revision":1,"type":"spec"})}],result:Value::Null,time:1,
    }).unwrap();
    let fresh = store.configuration_snapshot().unwrap();
    assert!(fresh.revision > before.revision);
    assert!(!Arc::ptr_eq(&before, &fresh));
    assert!(fresh.record("cache-spec:1").is_some());
}
#[test]
fn project_runtime_commits_do_not_rebuild_configuration() {
    let root = tempfile::tempdir().unwrap();
    let project = "c8516249-60a3-444a-988d-e5bf0e257611";
    let store = SqliteStore::open_project(root.path(), project).unwrap();
    let before = store.configuration_snapshot().unwrap();
    store.commit_changes(&change("runtime-context","context","runtime-context",json!({"id":"runtime-context","owner_id":project,"content":"Runtime facts","revision":1}))).unwrap();
    let trace = performance::Trace::start();
    let after = store.configuration_snapshot().unwrap();
    let trace = trace.finish();
    assert!(Arc::ptr_eq(&before, &after));
    assert!(trace["spans"]["catalog.build"].is_null());
}
