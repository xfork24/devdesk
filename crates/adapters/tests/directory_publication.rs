//! Directory snapshots are derived observations, not authoritative project edits.
use devdesk_adapters::GlobalStore;
use devdesk_core::{Command, EngineeringStore};
use devdesk_domain::Project;
use serde_json::{Value, json};
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
fn fixture(root: &std::path::Path) -> (Arc<GlobalStore>, Project) {
    let store = Arc::new(GlobalStore::open(root).unwrap());
    let id = store
        .reserve_project(
            &Command {
                command_id: "reserve".into(),
                target: None,
                expected_revision: None,
                payload: json!({"title":"Directory fixture"}),
            },
            &devdesk_core::InvocationContext::human(None),
        )
        .unwrap();
    let project = Project {
        id,
        title: "Revision 1".into(),
        workspace: String::new(),
        workspace_definition: None,
        execution_mode: "live".into(),
        concurrency: 2,
        revision: 1,
    };
    (store, project)
}

#[test]
fn identical_directory_snapshots_do_not_write_or_notify_and_equal_version_conflicts_fail() {
    let root = tempfile::tempdir().unwrap();
    let (store, project) = fixture(root.path());
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let listener: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        count.fetch_add(1, Ordering::Relaxed);
    });
    store.subscribe_changes(Arc::downgrade(&listener));
    store.publish_project(&project).unwrap();
    let observer = rusqlite::Connection::open(root.path().join("global.sqlite3")).unwrap();
    let version = || {
        observer
            .query_row("PRAGMA data_version", [], |row| row.get::<_, u64>(0))
            .unwrap()
    };
    let before = version();
    for _ in 0..100 {
        store.publish_project(&project).unwrap();
    }
    assert_eq!(version(), before);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let mut wrong = project.clone();
    wrong.title = "Different body at same version".into();
    assert!(store.publish_project(&wrong).is_err());
    assert_eq!(
        store.project(&project.id).unwrap().unwrap().title,
        project.title
    );
    assert_eq!(version(), before);
}

#[test]
fn one_hundred_out_of_order_project_snapshots_converge_to_the_highest_source_revision() {
    let root = tempfile::tempdir().unwrap();
    let (store, project) = fixture(root.path());
    let barrier = Arc::new(Barrier::new(100));
    let threads = (1..=100)
        .map(|revision| {
            let (store, barrier) = (store.clone(), barrier.clone());
            let mut next = project.clone();
            next.revision = revision;
            next.title = format!("Revision {revision}");
            std::thread::spawn(move || {
                barrier.wait();
                store.publish_project(&next)
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap().unwrap();
    }
    let current = store.project(&project.id).unwrap().unwrap();
    assert_eq!(current.revision, 100);
    assert_eq!(current.title, "Revision 100");
    store.publish_project(&project).unwrap();
    assert_eq!(store.project(&project.id).unwrap().unwrap().revision, 100);
}

#[test]
fn publishing_a_snapshot_does_not_wait_for_the_old_global_business_writer() {
    let root = tempfile::tempdir().unwrap();
    let (store, project) = fixture(root.path());
    let (ready, blocked) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let legacy = store.clone();
    let writer = std::thread::spawn(move || {
        legacy.seed_fixture_state(None, &mut |_| {
            ready.send(()).unwrap();
            released.recv().unwrap();
            Ok(Value::Null)
        })
    });
    blocked.recv().unwrap();
    let (done, completed) = mpsc::channel();
    let publisher = store.clone();
    let publication = std::thread::spawn(move || {
        done.send(publisher.publish_project(&project)).unwrap();
    });
    let result = completed.recv_timeout(std::time::Duration::from_secs(2));
    release.send(()).unwrap();
    writer.join().unwrap().unwrap();
    publication.join().unwrap();
    result.unwrap().unwrap();
}
