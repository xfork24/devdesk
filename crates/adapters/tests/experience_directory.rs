//! Derived metadata only; these fixtures do not run a workflow or a worker.
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
use devdesk_adapters::{FileArtifacts, GlobalStore, SqliteStore, performance};
use devdesk_core::{Engine, EngineeringStore, experience_directory::*};
use devdesk_domain::*;
use serde_json::{Value, json};
use std::sync::{Arc, Barrier, mpsc};
const PROJECT: &str = "5cb86774-e3de-4b31-a81a-41a8ea57b9f1";
const OTHER: &str = "8bbf5e97-b15a-45fd-a4e8-236456a11c64";
fn engine(path: &std::path::Path, project: &str) -> Arc<Engine> {
    if !path.join("global.sqlite3").exists() {
        GlobalStore::open(path).unwrap();
    }
    let store = Arc::new(SqliteStore::open_project(path, project).unwrap());
    Arc::new(Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(store.root().into())),
        runtime::adapters(),
    ))
}
fn metadata(project: &str, snapshot: &str, time: u64) -> Value {
    json!({"project_id":project,"project_name":"Historical project","feature_id":"feature","feature_name":"Historical Feature","agent_instance_id":"agent","agent_revision_id":"agent:1","snapshot_id":snapshot,"session_id":"session","content_digest":"fixture-snapshot-digest","captured_at":time,"work_time":time,"kind":"conversation_completed","binding":{"model":"fixture-model","installation":{"key":{"provider":"codex","version":"fixture"}},"credential_ref":"must-not-be-mirrored","permissions":{"large":"x".repeat(1000)}}})
}
fn source(engine: &Engine, id: &str, revision: u64, data: &Value) {
    let conn = rusqlite::Connection::open(
        std::path::Path::new(&engine.store.root()).join("engineering.sqlite3"),
    )
    .unwrap();
    let scope = engine
        .store
        .project_id()
        .map_or_else(Scope::global, Scope::project);
    conn.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES(?1,'experience','feature',?2,'Experience',1,?3,'{}',?4,0,?5,1) ON CONFLICT(id) DO UPDATE SET data=excluded.data,revision=excluded.revision,content_hash=excluded.content_hash",rusqlite::params![id,serde_json::to_string(&scope).unwrap(),content_digest(&References::new(),data),data.to_string(),revision]).unwrap();
}
fn count(store: &GlobalStore) -> u64 {
    let conn =
        rusqlite::Connection::open(std::path::Path::new(&store.root()).join("global.sqlite3"))
            .unwrap();
    conn.query_row(
        "SELECT count(*) FROM governance_records WHERE type='experience_directory'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}
#[test]
fn metadata_is_bounded_and_preserves_history_count_without_private_execution_content() {
    let root = tempfile::tempdir().unwrap();
    let engine = engine(root.path(), PROJECT);
    let mut data = metadata(PROJECT, "current", 1000);
    data["history"] = json!(
        (0..45)
            .map(|i| metadata(PROJECT, &format!("snapshot-{i}"), i))
            .collect::<Vec<_>>()
    );
    data["large_unrelated_field"] = json!("x".repeat(1_000_000));
    source(&engine, "source", 1, &data);
    let trace = performance::Trace::start();
    let row = engine
        .store
        .experience_directory_source("source")
        .unwrap()
        .unwrap();
    let report = trace.finish();
    assert_eq!(row.metadata["history"].as_array().unwrap().len(), 20);
    assert_eq!(row.metadata["history_count"], 45);
    assert_eq!(row.metadata["history_more"], true);
    assert_eq!(row.metadata["history"][0]["snapshot_id"], "snapshot-44");
    assert_eq!(row.metadata["binding"]["model"], "fixture-model");
    let body = row.metadata.to_string();
    assert!(
        !body.contains("must-not-be-mirrored")
            && !body.contains("permissions")
            && !body.contains("large_unrelated_field")
    );
    assert!(body.len() <= 64 * 1024);
    for stage in [
        "project.full_snapshot",
        "source.restore_records",
        "source.restore_reference",
        "workflow.compile",
    ] {
        assert!(report["spans"][stage].is_null(), "{report}");
    }
}
#[test]
fn header_pages_seek_the_owner_index_without_sorting_or_reading_payloads() {
    let root = tempfile::tempdir().unwrap();
    let engine = engine(root.path(), PROJECT);
    let conn = rusqlite::Connection::open(
        std::path::Path::new(&engine.store.root()).join("engineering.sqlite3"),
    )
    .unwrap();
    for owner in [Some(PROJECT), None] {
        let mut query = conn.prepare("EXPLAIN QUERY PLAN SELECT id,revision FROM governance_records WHERE type='experience' AND owner_project IS ?1 AND id>?2 ORDER BY id LIMIT 17").unwrap();
        let plan = query
            .query_map(rusqlite::params![owner, "source-000"], |r| {
                r.get::<_, String>(3)
            })
            .unwrap()
            .map(|row| row.unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(plan.contains("idx_experience_headers"), "{plan}");
        assert!(plan.contains("owner_project=? AND id>?"), "{plan}");
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
    }
}
#[test]
fn global_discussion_metadata_uses_its_own_namespace_without_a_project_identity() {
    let root = tempfile::tempdir().unwrap();
    let global = GlobalStore::open(root.path()).unwrap();
    let path = SqliteStore::global_discussion_path(root.path()).unwrap();
    let store = Arc::new(SqliteStore::open(&path).unwrap());
    let engine = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(store.root().into())),
        runtime::adapters(),
    );
    source(
        &engine,
        "discussion",
        1,
        &metadata(GLOBAL_DISCUSSION_NAMESPACE, "snapshot", 1),
    );
    engine.publish_experience_index(&global).unwrap();
    let row = global
        .read_resources(&[resource_key(&engine.store.namespace(), "discussion")])
        .unwrap()
        .remove(0);
    let row = ConfigurationRecord::from_storage(row.value.unwrap()).unwrap();
    assert_eq!(row.scope, Scope::global());
    assert_eq!(row.data["value"]["project_id"], GLOBAL_DISCUSSION_NAMESPACE);
    assert_eq!(count(&global), 1);
}
#[test]
fn one_hundred_identical_observations_commit_once_and_scope_keys_are_independent() {
    let root = tempfile::tempdir().unwrap();
    let source_engine = engine(root.path(), PROJECT);
    let other = engine(root.path(), OTHER);
    let global = Arc::new(GlobalStore::open(root.path()).unwrap());
    source(
        &source_engine,
        "same-source-id",
        1,
        &metadata(PROJECT, "snapshot", 1),
    );
    source(
        &other,
        "same-source-id",
        1,
        &metadata(OTHER, "other-snapshot", 1),
    );
    let entry = Arc::new(
        source_engine
            .store
            .experience_directory_source("same-source-id")
            .unwrap()
            .unwrap(),
    );
    let namespace = source_engine.store.namespace();
    let barrier = Arc::new(Barrier::new(100));
    let threads = (0..100)
        .map(|_| {
            let (global, entry, namespace, barrier) = (
                global.clone(),
                entry.clone(),
                namespace.clone(),
                barrier.clone(),
            );
            std::thread::spawn(move || {
                barrier.wait();
                publish(global.as_ref(), &namespace, &entry)
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap().unwrap();
    }
    assert_eq!(global.event_cursor().unwrap(), 1);
    assert_eq!(count(&global), 1);
    other.publish_experience_index(global.as_ref()).unwrap();
    assert_eq!(count(&global), 2);
    assert_ne!(
        resource_key(&namespace, "same-source-id"),
        resource_key(&other.store.namespace(), "same-source-id")
    );
}
#[test]
fn newer_source_versions_are_monotone_and_contradictory_same_version_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let engine = engine(root.path(), PROJECT);
    let global = GlobalStore::open(root.path()).unwrap();
    source(&engine, "source", 1, &metadata(PROJECT, "original", 1));
    let older = engine
        .store
        .experience_directory_source("source")
        .unwrap()
        .unwrap();
    publish(&global, &engine.store.namespace(), &older).unwrap();
    source(&engine, "source", 2, &metadata(PROJECT, "newer", 2));
    let newer = engine
        .store
        .experience_directory_source("source")
        .unwrap()
        .unwrap();
    publish(&global, &engine.store.namespace(), &newer).unwrap();
    let cursor = global.event_cursor().unwrap();
    publish(&global, &engine.store.namespace(), &older).unwrap();
    assert_eq!(global.event_cursor().unwrap(), cursor);
    let mut contradictory = newer;
    contradictory.metadata["snapshot_id"] = json!("different");
    assert!(publish(&global, &engine.store.namespace(), &contradictory).is_err());
    let row = global
        .read_resources(&[resource_key(&engine.store.namespace(), "source")])
        .unwrap()
        .remove(0);
    let record = ConfigurationRecord::from_storage(row.value.unwrap()).unwrap();
    assert_eq!(record.data["source_revision"], 2);
    assert_eq!(record.data["value"]["snapshot_id"], "newer");
}
#[test]
fn source_pagination_and_rotation_eventually_visit_idle_metadata_without_rewriting_it() {
    let root = tempfile::tempdir().unwrap();
    let engine = engine(root.path(), PROJECT);
    let global = GlobalStore::open(root.path()).unwrap();
    for i in 0..41 {
        source(
            &engine,
            &format!("source-{i:03}"),
            1,
            &metadata(PROJECT, &format!("snapshot-{i}"), i),
        );
    }
    let first = engine.store.experience_headers(None).unwrap();
    assert_eq!(first.records.len(), 16);
    let second = engine
        .store
        .experience_headers(first.next_after.as_deref())
        .unwrap();
    assert_eq!(second.records.len(), 16);
    let third = engine
        .store
        .experience_headers(second.next_after.as_deref())
        .unwrap();
    assert_eq!(third.records.len(), 9);
    assert!(third.next_after.is_none());
    for _ in 0..100 {
        engine.publish_experience_index(&global).unwrap();
        if count(&global) == 41 {
            break;
        }
    }
    assert_eq!(count(&global), 41);
    let cursor = global.event_cursor().unwrap();
    for _ in 0..10 {
        engine.publish_experience_index(&global).unwrap();
    }
    assert_eq!(global.event_cursor().unwrap(), cursor);
}
#[test]
fn publication_does_not_restore_unrelated_history_or_wait_for_legacy_global_writer() {
    let root = tempfile::tempdir().unwrap();
    let engine = engine(root.path(), PROJECT);
    let global = Arc::new(GlobalStore::open(root.path()).unwrap());
    source(&engine, "source", 1, &metadata(PROJECT, "snapshot", 1));
    let (ready, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let old = global.clone();
    let writer = std::thread::spawn(move || {
        old.transact(None, &mut |_| {
            ready.send(()).unwrap();
            blocked.recv().unwrap();
            Ok(Value::Null)
        })
    });
    waiting.recv().unwrap();
    let conn = rusqlite::Connection::open(
        std::path::Path::new(&engine.store.root()).join("engineering.sqlite3"),
    )
    .unwrap();
    conn.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('unrestorable','workflow_instance','owner',?1,'History',1,'fixture','{}',?2,0,1,1)",rusqlite::params![serde_json::to_string(&Scope::project(PROJECT)).unwrap(),json!({"plan":{"$source":"missing-frozen-source"},"large":"x".repeat(1_000_000)}).to_string()]).unwrap();
    let trace = performance::Trace::start();
    let result = engine.publish_experience_index(global.as_ref());
    let report = trace.finish();
    release.send(()).unwrap();
    writer.join().unwrap().unwrap();
    result.unwrap();
    for stage in [
        "project.full_snapshot",
        "global.full_snapshot",
        "project.transaction",
        "global.transaction",
        "source.restore_records",
        "workflow.compile",
    ] {
        assert!(report["spans"][stage].is_null(), "{report}");
    }
    assert_eq!(count(&global), 1);
}
#[test]
fn foreign_metadata_is_not_published_and_source_like_text_remains_literal() {
    let root = tempfile::tempdir().unwrap();
    let engine = engine(root.path(), PROJECT);
    let global = GlobalStore::open(root.path()).unwrap();
    source(&engine, "foreign", 1, &metadata(OTHER, "snapshot", 1));
    assert!(engine.publish_experience_index(&global).is_err());
    assert_eq!(count(&global), 0);
    let mut data = metadata(PROJECT, "literal", 1);
    let encoded = "\u{1e}devdesk-source-text:{\"$source\":\"source\",\"ref\":\"absent\"}";
    data["binding"]["model"] = json!(encoded);
    source(&engine, "literal", 1, &data);
    let entry = engine
        .store
        .experience_directory_source("literal")
        .unwrap()
        .unwrap();
    publish(&global, &engine.store.namespace(), &entry).unwrap();
    let state = global.snapshot().unwrap();
    let record = state
        .configuration(&resource_key(&engine.store.namespace(), "literal").id)
        .unwrap();
    assert_eq!(record.data["value"]["binding"]["model"], encoded);
}
