//! Reservation admission only; no Host provisioning or worker is started.
use devdesk_adapters::GlobalStore;
use devdesk_core::{Command, EngineeringStore, InvocationContext, project_reservation};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::sync::{Arc, Barrier, mpsc};
fn command(id: &str) -> Command {
    Command {
        command_id: id.into(),
        target: None,
        expected_revision: None,
        payload: json!({"title":"Reserved project"}),
    }
}
#[test]
fn one_hundred_competing_replays_reserve_one_identity_and_one_receipt() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(GlobalStore::open(root.path()).unwrap());
    let before = store.event_cursor().unwrap();
    let barrier = Arc::new(Barrier::new(100));
    let threads = (0..100)
        .map(|_| {
            let (store, barrier) = (store.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                store.reserve_project(
                    &command("same"),
                    &devdesk_core::InvocationContext::human(None),
                )
            })
        })
        .collect::<Vec<_>>();
    let ids = threads
        .into_iter()
        .map(|thread| thread.join().unwrap().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), 1);
    assert_eq!(store.event_cursor().unwrap(), before + 1);
    assert!(store.project(ids.first().unwrap()).unwrap().is_none());
    let mut changed = command("same");
    changed.payload["title"] = json!("Other");
    assert_eq!(
        store
            .reserve_project(&changed, &devdesk_core::InvocationContext::human(None))
            .unwrap_err()
            .code,
        "invalid_params"
    );
    let db = rusqlite::Connection::open(root.path().join("global.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM project_catalog", [], |row| row
            .get::<_, u64>(0))
            .unwrap(),
        1
    );
    assert_eq!(db.query_row("SELECT count(*) FROM governance_records WHERE type='command_context' AND json_extract(data,'$.context.actor.kind')='human'",[],|row|row.get::<_,u64>(0)).unwrap(),1);
}
#[test]
fn independent_reservations_do_not_take_the_legacy_global_writer() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(GlobalStore::open(root.path()).unwrap());
    let (ready, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let old = store.clone();
    let writer = std::thread::spawn(move || {
        old.seed_fixture_state(None, &mut |_| {
            ready.send(()).unwrap();
            blocked.recv().unwrap();
            Ok(Value::Null)
        })
    });
    waiting.recv().unwrap();
    let barrier = Arc::new(Barrier::new(100));
    let threads = (0..100)
        .map(|n| {
            let (store, barrier) = (store.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                store.reserve_project(
                    &command(&format!("independent-{n}")),
                    &devdesk_core::InvocationContext::human(None),
                )
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    release.send(()).unwrap();
    writer.join().unwrap().unwrap();
    let ids = results
        .into_iter()
        .map(Result::unwrap)
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), 100);
}
#[test]
fn compatible_old_reservations_replay_and_scope_mismatches_leave_no_rows() {
    let root = tempfile::tempdir().unwrap();
    let store = GlobalStore::open(root.path()).unwrap();
    let c = command("legacy");
    let fp = serde_json::to_string(&(
        "project.create",
        &c.target,
        &c.expected_revision,
        &c.payload,
    ))
    .unwrap();
    let db = rusqlite::Connection::open(root.path().join("global.sqlite3")).unwrap();
    db.execute(
        "INSERT INTO project_catalog(id,command_id,fingerprint) VALUES('existing','legacy',?1)",
        [fp],
    )
    .unwrap();
    let cursor = store.event_cursor().unwrap();
    assert_eq!(
        store
            .reserve_project(&c, &devdesk_core::InvocationContext::human(None))
            .unwrap(),
        "existing"
    );
    assert_eq!(store.event_cursor().unwrap(), cursor);
    assert_eq!(
        project_reservation::reserve(
            &store,
            &command("wrong"),
            &InvocationContext::human(Some("other".into()))
        )
        .unwrap_err()
        .code,
        "invalid_scope"
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM project_catalog", [], |row| row
            .get::<_, u64>(0))
            .unwrap(),
        1
    );
}
