//! Real SQLite reads remain complete when advisory retention runs out of bytes.
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
use devdesk_adapters::{FileArtifacts, SqliteStore, performance};
use devdesk_core::{concurrency::ResourceKey, *};
use devdesk_domain::*;
use serde_json::{Value, json};
use std::sync::Arc;

fn fixture(path: &std::path::Path, count: usize, bytes: usize) -> Engine {
    let engine = Engine::new(
        Arc::new(SqliteStore::open_fixture(path).unwrap()),
        Arc::new(FileArtifacts::new(path.into())),
        runtime::adapters(),
    );
    engine.bootstrap().unwrap();
    let project = engine
        .execute(
            "project.create",
            Command {
                command_id: id(),
                target: None,
                expected_revision: None,
                payload: json!({"title":"Readonly bounds"}),
            },
        )
        .unwrap();
    let scope = Scope::project(project["id"].as_str().unwrap());
    engine
        .store
        .seed_fixture_state(None, &mut |state| {
            for n in 0..count {
                let data = json!({"payload":"x".repeat(bytes),"index":n});
                state.governance.push(ConfigurationRecord {
                    id: format!("read-bounds-{n}"),
                    r#type: "fixture_read_bounds".into(),
                    definition_id: format!("read-bounds-{n}"),
                    scope: scope.clone(),
                    name: "Read bound fixture".into(),
                    version: 1,
                    supersedes: None,
                    content_hash: content_digest(&References::new(), &data),
                    refs: References::new(),
                    data,
                    immutable: true,
                    revision: 1,
                    created_at: 1,
                });
            }
            Ok(Value::Null)
        })
        .unwrap();
    engine
}

#[test]
fn oversized_advisory_row_falls_back_to_full_storage_reads() {
    let dir = tempfile::tempdir().unwrap();
    let e = fixture(dir.path(), 1, 1024 * 1024);
    let trace = performance::Trace::start();
    let _scope = e.store.advisory_read_scope();
    let key = ResourceKey::new("governance", "read-bounds-0");
    for _ in 0..2 {
        let rows = e.store.read_resources(std::slice::from_ref(&key)).unwrap();
        assert_eq!(
            rows[0].value.as_ref().unwrap()["data"]["payload"]
                .as_str()
                .unwrap()
                .len(),
            1024 * 1024
        );
    }
    let report = trace.finish();
    assert_eq!(report["spans"]["resource.read"]["calls"], 2);
    assert!(report["spans"]["resource.advisory_hit"].is_null());
}

#[test]
fn aggregate_byte_limit_bypasses_rows_without_truncation_before_entry_limit() {
    // One hundred entries is below the 256-entry cap, but their bodies exceed
    // the one-MiB byte budget. Both retained and bypassed values must be exact.
    let dir = tempfile::tempdir().unwrap();
    let e = fixture(dir.path(), 100, 64 * 1024);
    let trace = performance::Trace::start();
    let _scope = e.store.advisory_read_scope();
    for _ in 0..2 {
        for n in 0..100 {
            let rows = e
                .store
                .read_resources(&[ResourceKey::new("governance", format!("read-bounds-{n}"))])
                .unwrap();
            let data = &rows[0].value.as_ref().unwrap()["data"];
            assert_eq!(data["index"], n);
            assert_eq!(data["payload"].as_str().unwrap().len(), 64 * 1024);
        }
    }
    let report = trace.finish();
    let hits = report["spans"]["resource.advisory_hit"]["calls"]
        .as_u64()
        .unwrap();
    assert!(hits > 0 && hits < 100, "{report}");
    assert_eq!(
        report["spans"]["resource.read"]["calls"].as_u64().unwrap() + hits,
        200
    );
}
