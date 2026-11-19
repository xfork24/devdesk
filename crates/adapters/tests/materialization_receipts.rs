//! Receipt publication only. ResourceFixture never builds an image or starts a worker.
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
use devdesk_adapters::{FileArtifacts, SqliteStore, performance};
use devdesk_core::{Engine, concurrency::ResourceKey};
use devdesk_domain::*;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::sync::{Arc, Barrier, mpsc};

fn fixture(path: &std::path::Path) -> Arc<Engine> {
    let engine = Arc::new(Engine::new(
        Arc::new(SqliteStore::open(path).unwrap()),
        Arc::new(FileArtifacts::new(path.into())),
        runtime::adapters(),
    ));
    engine.bootstrap().unwrap();
    engine
}
fn harness(engine: &Engine) -> ConfigurationRecord {
    engine
        .store
        .configuration_snapshot()
        .unwrap()
        .state
        .configuration("builtin-harness-codex-managed:1")
        .unwrap()
        .clone()
}
fn receipt(record: &ConfigurationRecord) -> Value {
    json!({"harness_revision_id":record.id,"sandbox_ref":"builtin-sandbox-docker:2","recipe_digest":"verified-fixture-recipe","installation_id":"verified-fixture-installation","key":{"provider":"codex","version":"fixture","os":"linux","arch":"aarch64"},"artifact":{"kind":"docker_image","image_id":format!("sha256:{}","a".repeat(64))}})
}

#[test]
fn one_hundred_identical_verifications_publish_one_immutable_receipt() {
    let root = tempfile::tempdir().unwrap();
    let engine = fixture(root.path());
    let record = harness(&engine);
    let data = receipt(&record);
    let cursor = engine.store.event_cursor().unwrap();
    let barrier = Arc::new(Barrier::new(100));
    let threads = (0..100)
        .map(|_| {
            let (engine, record, data, barrier) = (
                engine.clone(),
                record.clone(),
                data.clone(),
                barrier.clone(),
            );
            std::thread::spawn(move || {
                barrier.wait();
                engine.record_harness_materialization(
                    engine.store.as_ref(),
                    "verified-receipt",
                    &record,
                    &data,
                )
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap().unwrap();
    }
    let row = engine
        .store
        .read_resources(&[ResourceKey::new("governance", "verified-receipt")])
        .unwrap()
        .remove(0);
    assert_eq!(row.revision, 1);
    let saved = ConfigurationRecord::from_storage(row.value.unwrap()).unwrap();
    assert_eq!(saved.data, data);
    assert!(saved.immutable);
    assert_eq!(engine.store.events_after(cursor, 100).unwrap().len(), 1);
    let mut replacement = data;
    replacement["installation_id"] = json!("different-installation");
    assert_eq!(
        engine
            .record_harness_materialization(
                engine.store.as_ref(),
                "verified-receipt",
                &record,
                &replacement,
            )
            .unwrap_err()
            .code,
        "invalid_params"
    );
    assert_eq!(engine.store.events_after(cursor, 100).unwrap().len(), 1);
}

#[test]
fn materialization_reads_no_unrelated_engineering_history() {
    let root = tempfile::tempdir().unwrap();
    let engine = fixture(root.path());
    let record = harness(&engine);
    let connection = Connection::open(root.path().join("engineering.sqlite3")).unwrap();
    connection.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('unrestorable-history','workflow_instance','owner','\"global\"','history',1,'fixture','{}',?1,0,1,1)",params![json!({"root_instance_id":"unrestorable-root","plan":{"$source":"missing-frozen-input"},"large":"x".repeat(1_000_000)}).to_string()]).unwrap();
    // Prime only the immutable configuration view. The corrupt history must not
    // be restored by either preparation or publication.
    engine.store.configuration_snapshot().unwrap();
    let trace = performance::Trace::start();
    let result = engine
        .materialize_harness_in_sandbox(
            &runtime::ResourceFixture {
                store: engine.store.as_ref(),
            },
            &record,
            "builtin-sandbox-docker:2",
        )
        .unwrap();
    assert!(result.recipe_digest.is_some());
    let report = trace.finish();
    for span in [
        "project.full_snapshot",
        "project.transaction",
        "source.restore_records",
        "workflow.compile",
    ] {
        assert!(report["spans"][span].is_null(), "{report}");
    }
}

#[test]
fn publication_interleaves_with_an_old_writer_without_losing_either_receipt() {
    let root = tempfile::tempdir().unwrap();
    let engine = fixture(root.path());
    let record = harness(&engine);
    let data = receipt(&record);
    let (ready, blocked) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let store = engine.store.clone();
    let writer = std::thread::spawn(move || {
        store.seed_fixture_state(None, &mut |state| {
            ready.send(()).unwrap();
            released.recv().unwrap();
            state.events.push(Event {
                id: state.events.last().map_or(1, |event| event.id + 1),
                target: "legacy-observation".into(),
                kind: "test.observed".into(),
                detail: String::new(),
                time: 1,
            });
            Ok(Value::Null)
        })
    });
    blocked.recv().unwrap();
    let (completed, result) = mpsc::channel();
    let active = engine.clone();
    let publication = std::thread::spawn(move || {
        completed
            .send(active.record_harness_materialization(
                active.store.as_ref(),
                "concurrent-receipt",
                &record,
                &data,
            ))
            .unwrap();
    });
    let observed = result.recv_timeout(std::time::Duration::from_secs(2));
    release.send(()).unwrap();
    writer.join().unwrap().unwrap();
    publication.join().unwrap();
    observed.unwrap().unwrap();
    assert!(
        engine
            .store
            .read_resources(&[ResourceKey::new("governance", "concurrent-receipt")])
            .unwrap()[0]
            .value
            .is_some()
    );
    let events = engine.store.events_after(0, 100).unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.target == "concurrent-receipt")
    );
    assert!(
        events
            .iter()
            .any(|event| event.target == "legacy-observation")
    );
}
