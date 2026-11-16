use devdesk_adapters::GlobalStore;
use devdesk_core::{EngineeringStore, concurrency::*};
use serde_json::{Value, json};
use std::sync::{Arc, Barrier};

fn condition(id: &str, revision: u64) -> ResourceVersion {
    ResourceVersion {
        resource: ResourceKey::new("setting", id),
        revision,
    }
}

fn prepared(command: &str, id: &str, revision: u64, value: Value) -> PreparedChangeSet {
    PreparedChangeSet {
        command_id: command.into(),
        fingerprint: json!({"actor":"test-human","target":id,"revision":revision,"value":value})
            .to_string(),
        read_set: ReadSet::new(vec![condition(id, revision)]).unwrap(),
        changes: vec![ResourceChange {
            resource: ResourceKey::new("setting", id),
            value: value.clone(),
        }],
        result: value,
        time: 1,
    }
}

#[test]
fn one_hundred_same_version_writes_have_one_winner() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(GlobalStore::open(directory.path()).unwrap());
    store
        .commit_changes(&prepared("seed", "target", 0, json!(0)))
        .unwrap();
    let barrier = Arc::new(Barrier::new(100));
    let threads: Vec<_> = (0..100)
        .map(|n| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let change = prepared(&format!("write-{n}"), "target", 1, json!(n + 1));
                barrier.wait();
                store.commit_changes(&change)
            })
        })
        .collect();
    let mut committed = 0;
    let mut conflicts = 0;
    for thread in threads {
        match thread.join().unwrap() {
            Ok(_) => committed += 1,
            Err(error) => {
                assert_eq!(error.code, "revision_conflict", "{error}");
                conflicts += 1;
            }
        }
    }
    assert_eq!((committed, conflicts), (1, 99));
    let target = store
        .read_resources(&[ResourceKey::new("setting", "target")])
        .unwrap();
    assert_eq!(target[0].revision, 2);
    assert_eq!(store.events_after(0, 100).unwrap().len(), 2);
}

#[test]
fn independent_targets_do_not_share_a_project_wide_revision() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(GlobalStore::open(directory.path()).unwrap());
    let barrier = Arc::new(Barrier::new(100));
    let threads: Vec<_> = (0..100)
        .map(|n| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let id = format!("target-{n}");
                barrier.wait();
                store.commit_changes(&prepared(&id, &id, 0, json!(n)))
            })
        })
        .collect();
    for thread in threads {
        assert_eq!(thread.join().unwrap().unwrap().versions[0].revision, 1);
    }
    let keys: Vec<_> = (0..100)
        .map(|n| ResourceKey::new("setting", format!("target-{n}")))
        .collect();
    let observed = store.read_resources(&keys).unwrap();
    assert!(
        observed
            .iter()
            .all(|value| value.revision == 1 && value.value.is_some())
    );
    assert_eq!(store.events_after(0, 100).unwrap().len(), 100);
}

#[test]
fn concurrent_replays_precede_version_checks_and_commit_once() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(GlobalStore::open(directory.path()).unwrap());
    store
        .commit_changes(&prepared("seed", "target", 0, json!(0)))
        .unwrap();
    let change = Arc::new(prepared("same-command", "target", 1, json!(1)));
    let barrier = Arc::new(Barrier::new(100));
    let threads: Vec<_> = (0..100)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            let change = change.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.commit_changes(&change)
            })
        })
        .collect();
    let mut cursor = None;
    for thread in threads {
        let receipt = thread.join().unwrap().unwrap();
        assert_eq!(receipt.versions[0].revision, 2);
        assert_eq!(*cursor.get_or_insert(receipt.cursor), receipt.cursor);
    }
    assert_eq!(store.events_after(0, 100).unwrap().len(), 2);
    let reused = prepared("same-command", "target", 1, json!(2));
    assert!(store.commit_changes(&reused).is_err());
}

#[test]
fn stale_read_dependencies_prevent_write_skew_and_partial_batches() {
    let directory = tempfile::tempdir().unwrap();
    let store = GlobalStore::open(directory.path()).unwrap();
    store
        .commit_changes(&prepared("seed-a", "a", 0, json!(true)))
        .unwrap();
    store
        .commit_changes(&prepared("seed-b", "b", 0, json!(true)))
        .unwrap();
    let mut first = prepared("first", "a", 1, json!(false));
    first.read_set = ReadSet::new(vec![condition("a", 1), condition("b", 1)]).unwrap();
    let mut second = prepared("second", "b", 1, json!(false));
    second.read_set = first.read_set.clone();
    store.commit_changes(&first).unwrap();
    assert_eq!(
        store.commit_changes(&second).unwrap_err().code,
        "revision_conflict"
    );
    let state = store
        .read_resources(&[
            ResourceKey::new("setting", "a"),
            ResourceKey::new("setting", "b"),
        ])
        .unwrap();
    assert_eq!(state[0].value, Some(json!(false)));
    assert_eq!(state[1].value, Some(json!(true)));
    let mut batch = prepared("batch", "a", 2, json!(true));
    batch.read_set = ReadSet::new(vec![condition("a", 2), condition("b", 9)]).unwrap();
    batch.changes.push(ResourceChange {
        resource: ResourceKey::new("setting", "b"),
        value: json!(false),
    });
    assert_eq!(
        store.commit_changes(&batch).unwrap_err().code,
        "revision_conflict"
    );
    let state = store
        .read_resources(&[
            ResourceKey::new("setting", "a"),
            ResourceKey::new("setting", "b"),
        ])
        .unwrap();
    assert_eq!(state[0].value, Some(json!(false)));
    assert_eq!(state[1].value, Some(json!(true)));
    assert_eq!(store.events_after(0, 100).unwrap().len(), 3);
}

#[test]
fn a_sql_failure_rolls_back_versions_values_events_and_idempotency() {
    let directory = tempfile::tempdir().unwrap();
    let store = GlobalStore::open(directory.path()).unwrap();
    let mut change = prepared("failed-command", "a", 0, json!(1));
    change.read_set = ReadSet::new(vec![condition("a", 0), condition("b", 0)]).unwrap();
    change.changes.push(ResourceChange {
        resource: ResourceKey::new("setting", "b"),
        value: json!(2),
    });
    let conn = rusqlite::Connection::open(directory.path().join("global.sqlite3")).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_b BEFORE INSERT ON settings WHEN NEW.key='b' BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert!(store.commit_changes(&change).is_err());
    let observed = store
        .read_resources(&[
            ResourceKey::new("setting", "a"),
            ResourceKey::new("setting", "b"),
        ])
        .unwrap();
    assert!(
        observed
            .iter()
            .all(|value| value.revision == 0 && value.value.is_none())
    );
    assert!(store.events_after(0, 100).unwrap().is_empty());
    conn.execute_batch("DROP TRIGGER reject_b;").unwrap();
    assert_eq!(store.commit_changes(&change).unwrap().versions.len(), 2);
    assert_eq!(store.events_after(0, 100).unwrap().len(), 1);
}

#[test]
fn replay_does_not_publish_changes_or_advance_the_catalog_clock() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let directory = tempfile::tempdir().unwrap();
    let store = GlobalStore::open(directory.path()).unwrap();
    let notified = Arc::new(AtomicUsize::new(0));
    let counter = notified.clone();
    let listener: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    store.subscribe_changes(Arc::downgrade(&listener));
    let change = prepared("once", "target", 0, json!({"value":1}));
    let first = store.commit_changes(&change).unwrap();
    let revision = store.configuration_snapshot().unwrap().revision;
    for _ in 0..10 {
        assert_eq!(store.commit_changes(&change).unwrap().cursor, first.cursor);
    }
    assert_eq!(notified.load(Ordering::SeqCst), 1);
    assert_eq!(store.configuration_snapshot().unwrap().revision, revision);
}

#[test]
fn project_resource_reads_are_finite_and_cannot_insert_a_different_project() {
    use devdesk_adapters::{SqliteStore, performance};
    let directory = tempfile::tempdir().unwrap();
    let project = "c8516249-60a3-444a-988d-e5bf0e257611";
    let store = SqliteStore::open_project(directory.path(), project).unwrap();
    let key = ResourceKey::new("project", project);
    let change = PreparedChangeSet {
        command_id: "create-project".into(),
        fingerprint: "project-identity".into(),
        read_set: ReadSet::new(vec![ResourceVersion {
            resource: key.clone(),
            revision: 0,
        }])
        .unwrap(),
        changes: vec![ResourceChange {
            resource: key.clone(),
            value: json!({"id":project,"title":"Own project","workspace":"unused","execution_mode":"live","concurrency":4,"revision":1,"workspace_definition":null}),
        }],
        result: json!({"id":project}),
        time: 1,
    };
    store.commit_changes(&change).unwrap();
    let trace = performance::Trace::start();
    let read = store.read_resources(std::slice::from_ref(&key)).unwrap();
    let measurement = trace.finish();
    assert_eq!(read[0].value.as_ref().unwrap()["title"], "Own project");
    assert!(measurement["spans"]["project.full_snapshot"].is_null());
    let mut other = change;
    other.command_id = "wrong-project".into();
    other.changes[0].resource.id = "f62ddc57-2b83-4c67-b8d6-a7dbbfaaeea8".into();
    other.changes[0].value["id"] = json!(other.changes[0].resource.id);
    assert!(store.commit_changes(&other).is_err());
    assert_eq!(store.events_after(0, 100).unwrap().len(), 1);
}

#[test]
fn immutable_sources_publish_with_their_exact_head_and_cannot_be_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let store = GlobalStore::open(directory.path()).unwrap();
    let source_key = ResourceKey::new("source", "sample-spec:1");
    let head_key = ResourceKey::new("source_head", "sample-spec");
    let source = json!({"type":"spec","identity":"sample-spec","revision":1,"name":"Sample","scope":"global","data":{"schema_version":3,"body":"Sample contract","checks":[]}});
    let mut change = PreparedChangeSet {
        command_id: "publish".into(),
        fingerprint: "exact-source-and-head".into(),
        read_set: ReadSet::new(vec![ResourceVersion {
            resource: source_key.clone(),
            revision: 0,
        }])
        .unwrap(),
        changes: vec![ResourceChange {
            resource: source_key.clone(),
            value: source.clone(),
        }],
        result: source.clone(),
        time: 1,
    };
    assert_eq!(
        store.commit_changes(&change).unwrap_err().code,
        "precondition_required"
    );
    change.read_set = ReadSet::new(vec![
        ResourceVersion {
            resource: source_key.clone(),
            revision: 0,
        },
        ResourceVersion {
            resource: head_key.clone(),
            revision: 0,
        },
    ])
    .unwrap();
    change.changes.push(ResourceChange {
        resource: head_key.clone(),
        value: json!({"reference":"sample-spec:1"}),
    });
    store.commit_changes(&change).unwrap();
    let read = store
        .read_resources(&[source_key.clone(), head_key.clone()])
        .unwrap();
    assert_eq!(read[0].value, Some(source));
    assert_eq!(
        read[1].value.as_ref().unwrap()["reference"],
        "sample-spec:1"
    );
    change.command_id = "overwrite".into();
    change.fingerprint = "attempted-source-overwrite".into();
    change.read_set = ReadSet::new(vec![
        ResourceVersion {
            resource: source_key,
            revision: 1,
        },
        ResourceVersion {
            resource: head_key,
            revision: 1,
        },
    ])
    .unwrap();
    change.changes[0].value["data"]["body"] = json!("Changed immutable body");
    assert!(store.commit_changes(&change).is_err());
    assert_eq!(store.events_after(0, 100).unwrap().len(), 1);
}

#[test]
fn sqlite_writer_contention_is_bounded_and_does_not_block_readers() {
    let directory = tempfile::tempdir().unwrap();
    let store = GlobalStore::open(directory.path()).unwrap();
    store
        .commit_changes(&prepared("seed", "target", 0, json!(1)))
        .unwrap();
    let blocker = rusqlite::Connection::open(directory.path().join("global.sqlite3")).unwrap();
    blocker
        .execute_batch("BEGIN IMMEDIATE; UPDATE settings SET value='2' WHERE key='target';")
        .unwrap();
    let before = std::time::Instant::now();
    let read = store
        .read_resources(&[ResourceKey::new("setting", "target")])
        .unwrap();
    assert_eq!(read[0].value, Some(json!(1)));
    let change = prepared("after-contention", "target", 1, json!(3));
    assert_eq!(
        store.commit_changes(&change).unwrap_err().code,
        "storage_busy"
    );
    assert!(before.elapsed() < std::time::Duration::from_secs(1));
    blocker.execute_batch("ROLLBACK;").unwrap();
    assert_eq!(
        store.commit_changes(&change).unwrap().versions[0].revision,
        2
    );
}
