use devdesk_adapters::{FileArtifacts, GlobalStore, SqliteStore, performance};
use devdesk_core::{Engine, ports::EngineeringStore};
use devdesk_domain::*;
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
#[path = "../../../tests/support/runtime.rs"]
mod runtime;

fn initialized(store: Arc<dyn EngineeringStore>) -> Engine {
    let engine = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(store.root().into())),
        runtime::adapters(),
    );
    engine.bootstrap().unwrap();
    engine
}
fn source(identity: &str) -> ConfigurationRecord {
    SourceDefinition {
        r#type: "role".into(),
        identity: identity.into(),
        revision: 1,
        name: identity.into(),
        scope: Scope::global(),
        refs: Default::default(),
        data: json!({"instructions":"configuration cache test"}),
    }
    .into_record()
    .unwrap()
}

#[test]
fn configuration_cache_pins_revisions_and_ignores_fact_only_commits() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(GlobalStore::open(directory.path()).unwrap());
    let _engine = initialized(store.clone());
    let before = store.configuration_snapshot().unwrap();
    let trace = performance::Trace::start();
    assert!(Arc::ptr_eq(
        &before,
        &store.configuration_snapshot().unwrap()
    ));
    let measured = trace.finish();
    assert_eq!(measured["spans"]["catalog.cache_hit"]["calls"], 1);
    assert!(measured["spans"]["global.full_snapshot"].is_null());
    assert!(measured["spans"]["source.load_records"].is_null());
    store
        .transact(None, &mut |state| {
            state.events.push(Event {
                id: 1,
                target: "test".into(),
                kind: "test.fact".into(),
                detail: String::new(),
                time: 0,
            });
            Ok(json!({}))
        })
        .unwrap();
    assert!(Arc::ptr_eq(
        &before,
        &store.configuration_snapshot().unwrap()
    ));
    let added = source("new-source");
    store
        .transact(None, &mut |state| {
            state.governance.push(added.clone());
            Ok(json!({}))
        })
        .unwrap();
    let after = store.configuration_snapshot().unwrap();
    assert!(after.revision > before.revision);
    assert!(before.record(&added.id).is_none());
    assert!(after.record(&added.id).is_some());
    let failed = store.transact(None, &mut |_| Err(err("test_rollback", "not committed")));
    assert!(failed.is_err());
    assert!(Arc::ptr_eq(
        &after,
        &store.configuration_snapshot().unwrap()
    ));
}

#[test]
fn configuration_and_events_do_not_decode_unrelated_engineering_history() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(directory.path()).unwrap());
    let _engine = initialized(store.clone());
    // Deliberately damaged, unrelated history detects accidental full-history decoding.
    let conn = rusqlite::Connection::open(directory.path().join("engineering.sqlite3")).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
    conn.execute("INSERT INTO conversation_events(id,session_id,turn_id,sequence,deduplication_key,kind,content,time) VALUES('unrelated','missing','missing',0,'unrelated','invalid-domain-event','',0)",[]).unwrap();
    assert!(store.snapshot().is_err());
    let trace = performance::Trace::start();
    let catalog = store.configuration_snapshot().unwrap();
    assert!(!catalog.state.governance.is_empty());
    assert!(catalog.state.conversation_events.is_empty());
    assert!(store.events_after(0, 100).is_ok());
    let measured = trace.finish();
    assert!(measured["spans"]["project.full_snapshot"].is_null());
    assert_eq!(measured["spans"]["events.indexed_read"]["calls"], 1);
}

#[test]
fn readonly_catalog_detects_commits_and_listeners_run_after_unlock() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(GlobalStore::open(directory.path()).unwrap());
    let _engine = initialized(store.clone());
    let before = GlobalStore::read_configuration(directory.path()).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let listener: Arc<dyn Fn() + Send + Sync> = {
        let store = store.clone();
        let calls = calls.clone();
        Arc::new(move || {
            assert!(store.configuration_snapshot().is_ok());
            assert!(store.events_after(0, 100).is_ok());
            calls.fetch_add(1, Ordering::SeqCst);
        })
    };
    store.subscribe_changes(Arc::downgrade(&listener));
    let added = source("outside-connection");
    store
        .transact(None, &mut |state| {
            state.governance.push(added.clone());
            Ok(json!({}))
        })
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let after = GlobalStore::read_configuration(directory.path()).unwrap();
    assert!(before.record(&added.id).is_none());
    assert!(after.record(&added.id).is_some());
    assert!(after.revision > before.revision);
}
