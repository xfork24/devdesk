//! Insert-only history import. Ports are test doubles; no external model is run.
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
use devdesk_adapters::{FileArtifacts, SqliteStore, performance};
use devdesk_core::{
    Command, Engine, EngineeringStore, InvocationContext, concurrency::ResourceKey,
    project_creation,
};
use devdesk_domain::*;
use serde_json::{Value, json};
use std::sync::{Arc, Barrier, mpsc};
const PROJECT: &str = "3b023b60-4e89-4138-a7a5-3c02e487f927";
fn fixture(path: &std::path::Path) -> Arc<Engine> {
    let store = Arc::new(SqliteStore::open_project(path, PROJECT).unwrap());
    project_creation::create(
        store.as_ref(),
        &Command {
            command_id: "project".into(),
            target: None,
            expected_revision: None,
            payload: json!({"title":"Import destination"}),
        },
        &InvocationContext::human(Some(PROJECT.into())),
    )
    .unwrap();
    Arc::new(Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(store.root().into())),
        runtime::adapters(),
    ))
}
fn detail() -> Value {
    json!({"snapshot":Snapshot { id:"historical-snapshot".into(),feature_id:Some("historical-feature".into()),project_id:Some("historical-project".into()),task_id:None,kind:"turn_completed".into(),content:"Frozen historical context".into(),documents:vec!["historical-document".into()],inherited_from:None,capture:SnapshotCapture::default(),runtime_bindings:vec![],execution_instances:vec![] },"entry":{"id":"history","captured_at":100},"documents":[{"id":"historical-document","content":"Source data only"}]})
}
fn command(id: &str) -> Command {
    Command {
        command_id: id.into(),
        target: Some(PROJECT.into()),
        expected_revision: Some(1),
        payload: json!({"snapshot_id":"historical-snapshot"}),
    }
}
#[test]
fn one_hundred_replays_publish_one_snapshot_with_provenance_without_loading_history() {
    let root = tempfile::tempdir().unwrap();
    let engine = fixture(root.path());
    let connection = rusqlite::Connection::open(
        std::path::Path::new(&engine.store.root()).join("engineering.sqlite3"),
    )
    .unwrap();
    connection.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('unrestorable','workflow_instance','owner',?1,'history',1,'fixture','{}',?2,0,1,1)",rusqlite::params![serde_json::to_string(&Scope::project(PROJECT)).unwrap(),json!({"root_instance_id":"unrestorable-root","plan":{"$source":"absent-frozen-input"},"large":"x".repeat(1_000_000)}).to_string()]).unwrap();
    let before = engine.store.event_cursor().unwrap();
    let barrier = Arc::new(Barrier::new(100));
    let threads = (0..100)
        .map(|_| {
            let (engine, barrier) = (engine.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                let trace = performance::Trace::start();
                let result = engine.import_experience(
                    &command("same"),
                    PROJECT,
                    &detail(),
                    &InvocationContext::human(Some(PROJECT.into())),
                );
                (result, trace.finish())
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        let (result, trace) = thread.join().unwrap();
        assert_eq!(result.unwrap(), "import:same");
        for stage in [
            "project.full_snapshot",
            "project.transaction",
            "source.restore_records",
            "workflow.compile",
        ] {
            assert!(trace["spans"][stage].is_null(), "{trace}");
        }
    }
    let rows = engine
        .store
        .read_resources(&[
            ResourceKey::new("snapshot", "import:same"),
            ResourceKey::new("governance", "source:same"),
        ])
        .unwrap();
    let imported: Snapshot = serde_json::from_value(rows[0].value.clone().unwrap()).unwrap();
    assert_eq!(imported.project_id.as_deref(), Some(PROJECT));
    assert!(
        imported.feature_id.is_none()
            && imported.task_id.is_none()
            && imported.documents.is_empty()
            && imported.inherited_from.is_none()
    );
    assert!(imported.content.contains("Source data only"));
    let source = ConfigurationRecord::from_storage(rows[1].value.clone().unwrap()).unwrap();
    assert!(source.immutable);
    assert_eq!(source.scope, Scope::project(PROJECT));
    assert_eq!(engine.store.events_after(before, 100).unwrap().len(), 1);
    let mut changed = detail();
    changed["snapshot"]["content"] = json!("Different");
    assert_eq!(
        engine
            .import_experience(
                &command("same"),
                PROJECT,
                &changed,
                &InvocationContext::human(Some(PROJECT.into()))
            )
            .unwrap_err()
            .code,
        "invalid_params"
    );
}
#[test]
fn imports_commit_while_legacy_writer_is_busy_and_keep_both_appends() {
    let root = tempfile::tempdir().unwrap();
    let engine = fixture(root.path());
    let (ready, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let old = engine.clone();
    let writer = std::thread::spawn(move || {
        old.store.seed_fixture_state(None, &mut |state| {
            ready.send(()).unwrap();
            blocked.recv().unwrap();
            state.events.push(Event {
                id: 1,
                target: "old".into(),
                kind: "test.old".into(),
                detail: "fixture".into(),
                time: 1,
            });
            Ok(Value::Null)
        })
    });
    waiting.recv().unwrap();
    let result = engine.import_experience(
        &command("unblocked"),
        PROJECT,
        &detail(),
        &InvocationContext::human(Some(PROJECT.into())),
    );
    release.send(()).unwrap();
    writer.join().unwrap().unwrap();
    assert_eq!(result.unwrap(), "import:unblocked");
    let state = engine.store.snapshot().unwrap();
    assert!(
        state
            .snapshots
            .iter()
            .any(|snapshot| snapshot.id == "import:unblocked")
    );
    assert!(state.events.iter().any(|event| event.target == "old"));
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|row| row.r#type == "command_context"
                && row.definition_id == "unblocked"
                && row.data["context"]["actor"]["kind"] == "human")
            .count(),
        1
    );
}
#[test]
fn mismatched_scope_cannot_leave_partial_history() {
    let root = tempfile::tempdir().unwrap();
    let engine = fixture(root.path());
    let before = engine.store.event_cursor().unwrap();
    for (pid, context) in [
        (
            "wrong-project",
            InvocationContext::human(Some("wrong-project".into())),
        ),
        (PROJECT, InvocationContext::human(None)),
    ] {
        assert!(
            engine
                .import_experience(&command("wrong"), pid, &detail(), &context)
                .is_err()
        );
    }
    assert_eq!(engine.store.event_cursor().unwrap(), before);
    assert!(
        engine
            .store
            .read_resources(&[ResourceKey::new("snapshot", "import:wrong")])
            .unwrap()[0]
            .value
            .is_none()
    );
}
