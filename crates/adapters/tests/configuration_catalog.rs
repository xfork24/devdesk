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
        .seed_fixture_state(None, &mut |state| {
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
        .seed_fixture_state(None, &mut |state| {
            state.governance.push(added.clone());
            Ok(json!({}))
        })
        .unwrap();
    let after = store.configuration_snapshot().unwrap();
    assert!(after.revision > before.revision);
    assert!(before.record(&added.id).is_none());
    assert!(after.record(&added.id).is_some());
    let failed =
        store.seed_fixture_state(None, &mut |_| Err(err("test_rollback", "not committed")));
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
        .seed_fixture_state(None, &mut |state| {
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

#[test]
#[ignore = "explicit source scaling profile"]
fn configuration_source_scaling() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(GlobalStore::open(directory.path()).unwrap());
    let _engine = initialized(store.clone());
    let snapshot = store.configuration_snapshot().unwrap();
    let role = snapshot.records("role").next().unwrap().clone();
    for count in [200, 2000, 10000] {
        let mut state = snapshot.state.as_ref().clone();
        for n in state
            .governance
            .iter()
            .filter(|r| r.is_definition())
            .count()..count
        {
            let mut row = role.clone();
            row.definition_id = format!("scaling-role-{n}");
            row.id = format!("{}:1", row.definition_id);
            row.version = 1;
            state.governance.push(row);
        }
        let started = std::time::Instant::now();
        let catalog = devdesk_core::ConfigurationCatalogSnapshot::new(state, 1, true).unwrap();
        let construction = started.elapsed().as_secs_f64() * 1000.0;
        let started = std::time::Instant::now();
        let result =
            devdesk_core::configuration_index(&catalog, &json!({"limit":1000}), None).unwrap();
        println!(
            "{}",
            json!({"sources":count,"construction_ms":construction,"index_ms":started.elapsed().as_secs_f64()*1000.0,"page_bytes":result.to_string().len(),"total":result["total"]})
        );
    }
}

#[test]
fn indexed_membership_retirement_and_heads_match_authoritative_rules() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(GlobalStore::open(directory.path()).unwrap());
    let _engine = initialized(store.clone());
    let mut state = store
        .configuration_snapshot()
        .unwrap()
        .state
        .as_ref()
        .clone();
    let role = state
        .governance
        .iter()
        .find(|r| r.r#type == "role")
        .unwrap()
        .clone();
    let member = state
        .governance
        .iter()
        .find(|r| r.r#type == "template_catalog_member" && r.definition_id == role.id)
        .unwrap()
        .clone();
    state
        .governance
        .iter_mut()
        .find(|r| r.id == TEMPLATE_CATALOG_STATE)
        .unwrap()
        .data["generation"] = json!(2);
    let mut current = member.clone();
    current.id = "new-membership".into();
    current.data["generation"] = json!(2);
    state.governance.push(current);
    for (kind, target, data) in [
        ("role_retirement", role.definition_id.clone(), json!({})),
        ("configuration_retirement", role.id.clone(), json!({})),
        (
            "standard_archive",
            state
                .governance
                .iter()
                .find(|r| r.r#type == "standard")
                .unwrap()
                .id
                .clone(),
            json!({"archived":true}),
        ),
    ] {
        let mut row = role.clone();
        row.id = format!("test-{kind}");
        row.r#type = kind.into();
        row.definition_id = target;
        row.data = data;
        state.governance.push(row);
    }
    let catalog = devdesk_core::ConfigurationCatalogSnapshot::new(state, 2, true).unwrap();
    for record in &catalog.state.governance {
        assert_eq!(
            catalog.is_current(record),
            template_record_current(&catalog.state, record),
            "{}",
            record.id
        );
        assert_eq!(
            catalog.is_retired(record),
            devdesk_core::configuration_retired(&catalog.state, record),
            "{}",
            record.id
        );
        assert_eq!(
            catalog.is_head(record),
            !catalog
                .state
                .governance
                .iter()
                .any(|n| n.r#type == record.r#type
                    && n.definition_id == record.definition_id
                    && n.version > record.version)
        );
    }
    assert!(catalog.is_current(&role));
    assert!(catalog.is_retired(&role));
}
