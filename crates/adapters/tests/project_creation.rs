//! Scoped metadata admission only, not directory provisioning or workflow capacity.
use devdesk_adapters::{FileArtifacts, SqliteStore, performance};
use devdesk_core::{
    Command, Engine, EngineeringStore, InvocationContext, concurrency::ResourceKey,
    project_creation::create,
};
use devdesk_domain::Event;
use serde_json::{Value, json};
use std::sync::{Arc, Barrier, mpsc};
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
const PROJECT: &str = "1920ec0a-e036-4fe3-8f6b-4ad059a2f515";
fn command(id: &str, title: &str) -> Command {
    Command {
        command_id: id.into(),
        target: None,
        expected_revision: None,
        payload: json!({"title":title}),
    }
}

#[test]
fn one_hundred_competing_creations_have_one_project_context_and_original_provenance() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open_project(root.path(), PROJECT).unwrap());
    let barrier = Arc::new(Barrier::new(100));
    let threads = (0..100)
        .map(|n| {
            let (store, barrier) = (store.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                create(
                    store.as_ref(),
                    &command(&format!("create-{n}"), &format!("Project {n}")),
                    &InvocationContext::human(Some(PROJECT.into())),
                )
            })
        })
        .collect::<Vec<_>>();
    let mut successes = 0;
    for thread in threads {
        match thread.join().unwrap() {
            Ok(receipt) => {
                successes += 1;
                assert_eq!(receipt.result["id"], PROJECT);
            }
            Err(error) => assert_eq!(error.code, "revision_conflict"),
        }
    }
    assert_eq!(successes, 1);
    let connection =
        rusqlite::Connection::open(std::path::Path::new(&store.root()).join("engineering.sqlite3"))
            .unwrap();
    let (projects,contexts,audits):(u64,u64,u64)=connection.query_row("SELECT (SELECT count(*) FROM projects),(SELECT count(*) FROM contexts),(SELECT count(*) FROM governance_records WHERE type='command_context' AND json_extract(data,'$.context.actor.kind')='human')",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!((projects, contexts, audits), (1, 1, 1));
    assert_eq!(store.event_cursor().unwrap(), 1);
}

#[test]
fn one_hundred_original_replays_preserve_one_creation_and_reject_other_parameters() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open_project(root.path(), PROJECT).unwrap());
    let barrier = Arc::new(Barrier::new(100));
    let threads = (0..100)
        .map(|_| {
            let (store, barrier) = (store.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                create(
                    store.as_ref(),
                    &command("same-command", "Original"),
                    &InvocationContext::human(Some(PROJECT.into())),
                )
            })
        })
        .collect::<Vec<_>>();
    let mut cursor = None;
    for thread in threads {
        let receipt = thread.join().unwrap().unwrap();
        if let Some(previous) = cursor {
            assert_eq!(receipt.cursor, previous);
        } else {
            cursor = Some(receipt.cursor);
        }
        assert_eq!(receipt.versions.len(), 3);
    }
    assert_eq!(
        create(
            store.as_ref(),
            &command("same-command", "Changed"),
            &InvocationContext::human(Some(PROJECT.into()))
        )
        .unwrap_err()
        .code,
        "invalid_params"
    );
    assert_eq!(store.project_metadata().unwrap().title, "Original");
    assert_eq!(store.event_cursor().unwrap(), cursor.unwrap());
}

#[test]
fn scoped_engine_creation_bypasses_old_writer_and_never_restores_engineering_history() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open_project(root.path(), PROJECT).unwrap());
    let engine = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(store.root().into())),
        runtime::adapters(),
    );
    let (ready, blocked) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let legacy = store.clone();
    let writer = std::thread::spawn(move || {
        legacy.seed_fixture_state(None, &mut |state| {
            ready.send(()).unwrap();
            released.recv().unwrap();
            state.events.push(Event {
                id: 1,
                target: "old-writer".into(),
                kind: "test.observed".into(),
                detail: String::new(),
                time: 1,
            });
            Ok(Value::Null)
        })
    });
    blocked.recv().unwrap();
    let (done, completed) = mpsc::channel();
    let admission = std::thread::spawn(move || {
        let trace = performance::Trace::start();
        let result = engine.execute(
            "project.create",
            command("actual-engine-entry", "Native scoped creation"),
        );
        done.send((result, trace.finish())).unwrap();
    });
    let observed = completed.recv_timeout(std::time::Duration::from_secs(2));
    release.send(()).unwrap();
    writer.join().unwrap().unwrap();
    admission.join().unwrap();
    let (result, report) = observed.unwrap();
    assert_eq!(result.unwrap()["id"], PROJECT);
    for span in [
        "project.full_snapshot",
        "project.transaction",
        "source.restore_records",
        "workflow.compile",
    ] {
        assert!(report["spans"][span].is_null(), "{report}");
    }
    let rows = store
        .read_resources(&[ResourceKey::new("project", PROJECT)])
        .unwrap();
    assert_eq!(rows[0].revision, 1);
    assert!(rows[0].value.is_some());
    assert_eq!(store.events_after(0, 100).unwrap().len(), 2);
}

#[test]
fn creation_scope_and_payload_validation_cannot_write_partial_metadata() {
    let root = tempfile::tempdir().unwrap();
    let store = SqliteStore::open_project(root.path(), PROJECT).unwrap();
    let context = InvocationContext::human(Some(PROJECT.into()));
    let request = command("valid", "Project");
    assert_eq!(
        create(&store, &request, &InvocationContext::human(None))
            .unwrap_err()
            .code,
        "invalid_scope"
    );
    for payload in [
        json!({}),
        json!({"title":null}),
        json!({"title":" "}),
        json!({"title":"Project","use_builtin_defaults":true}),
        json!({"title":"Project","mainline":" "}),
        json!({"title":"Project","unsupported":true}),
    ] {
        assert!(
            create(
                &store,
                &Command {
                    payload,
                    ..request.clone()
                },
                &context
            )
            .is_err()
        );
    }
    assert!(
        store
            .read_resources(&[ResourceKey::new("project", PROJECT)])
            .unwrap()[0]
            .value
            .is_none()
    );
    assert_eq!(store.event_cursor().unwrap(), 0);
}
