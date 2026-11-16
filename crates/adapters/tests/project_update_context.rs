//! Isolated finite-use-case checks. The production dispatcher deliberately stays
//! on the old path until every writer sharing Project invariants is migrated.
use devdesk_adapters::{SqliteStore, performance};
use devdesk_core::{
    Command, EngineeringStore, InvocationContext, concurrency::*,
    project_updates::ProjectUpdateContext,
};
use devdesk_domain::Project;
use serde_json::{Value, json};
use std::sync::{Arc, Barrier};
const PROJECT: &str = "c8516249-60a3-444a-988d-e5bf0e257611";
const OTHER: &str = "f62ddc57-2b83-4c67-b8d6-a7dbbfaaeea8";
fn fixture(root: &std::path::Path) -> Arc<SqliteStore> {
    let store = Arc::new(SqliteStore::open_project(root, PROJECT).unwrap());
    let project = Project {
        id: PROJECT.into(),
        title: "Original".into(),
        workspace: "/original".into(),
        workspace_definition: None,
        execution_mode: "live".into(),
        concurrency: 2,
        revision: 1,
    };
    let key = ResourceKey::new("project", PROJECT);
    store
        .commit_changes(&PreparedChangeSet {
            command_id: "test-fixture".into(),
            fingerprint: "native-project-fixture".into(),
            read_set: ReadSet::new(vec![ResourceVersion {
                resource: key.clone(),
                revision: 0,
            }])
            .unwrap(),
            changes: vec![ResourceChange {
                resource: key,
                value: json!(project),
            }],
            result: Value::Null,
            time: 1,
        })
        .unwrap();
    store
}
fn command(id: &str, revision: u64, payload: Value) -> Command {
    Command {
        command_id: id.into(),
        target: Some(PROJECT.into()),
        expected_revision: Some(revision),
        payload,
    }
}
fn prepare(
    store: &dyn EngineeringStore,
    command: &Command,
) -> devdesk_domain::Result<PreparedChangeSet> {
    ProjectUpdateContext::read(store, command)?
        .prepare(command, &InvocationContext::human(Some(PROJECT.into())))
}
#[test]
fn one_hundred_project_preparations_have_one_atomic_winner_and_provenance() {
    let root = tempfile::tempdir().unwrap();
    let store = fixture(root.path());
    let changes = (0..100)
        .map(|n| {
            prepare(
                store.as_ref(),
                &command(
                    &format!("rename-{n}"),
                    1,
                    json!({"title":format!("Title {n}")}),
                ),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let barrier = Arc::new(Barrier::new(100));
    let threads = changes
        .into_iter()
        .map(|change| {
            let (store, barrier) = (store.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                store.commit_changes(&change)
            })
        })
        .collect::<Vec<_>>();
    let mut successes = 0;
    for thread in threads {
        match thread.join().unwrap() {
            Ok(_) => successes += 1,
            Err(error) => assert_eq!(error.code, "revision_conflict"),
        }
    }
    assert_eq!(successes, 1);
    let project = store
        .read_resources(&[ResourceKey::new("project", PROJECT)])
        .unwrap()
        .remove(0);
    assert_eq!(project.value.unwrap()["revision"], 2);
    let connection = rusqlite::Connection::open(
        SqliteStore::project_path(root.path(), PROJECT)
            .unwrap()
            .join("engineering.sqlite3"),
    )
    .unwrap();
    let (count,actor,project):(u64,String,String)=connection.query_row("SELECT count(*),json_extract(data,'$.context.actor.kind'),json_extract(data,'$.context.project_id') FROM governance_records WHERE type='command_context'",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
    assert_eq!(count, 1);
    assert_eq!(actor, "human");
    assert_eq!(project, PROJECT);
}
#[test]
fn a_prepared_original_command_replays_without_overwriting_a_later_edit() {
    let root = tempfile::tempdir().unwrap();
    let store = fixture(root.path());
    let first = prepare(
        store.as_ref(),
        &command("first", 1, json!({"title":"First"})),
    )
    .unwrap();
    let receipt = store.commit_changes(&first).unwrap();
    let second = prepare(
        store.as_ref(),
        &command("second", 2, json!({"title":"Second","concurrency":8})),
    )
    .unwrap();
    store.commit_changes(&second).unwrap();
    let replay = store.commit_changes(&first).unwrap();
    assert_eq!(replay.cursor, receipt.cursor);
    assert_eq!(
        serde_json::to_value(replay.versions).unwrap(),
        serde_json::to_value(receipt.versions).unwrap()
    );
    let project = store
        .read_resources(&[ResourceKey::new("project", PROJECT)])
        .unwrap()
        .remove(0)
        .value
        .unwrap();
    assert_eq!(project["title"], "Second");
    assert_eq!(project["concurrency"], 8);
    assert_eq!(project["revision"], 3);
    assert_eq!(
        prepare(
            store.as_ref(),
            &command("stale", 1, json!({"title":"Stale"}))
        )
        .unwrap_err()
        .code,
        "revision_conflict"
    );
}
#[test]
fn finite_project_preparation_validates_scope_and_never_restores_unrelated_facts() {
    let root = tempfile::tempdir().unwrap();
    let store = fixture(root.path());
    let connection = rusqlite::Connection::open(
        SqliteStore::project_path(root.path(), PROJECT)
            .unwrap()
            .join("engineering.sqlite3"),
    )
    .unwrap();
    connection.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('unreadable','workflow_instance','other',?1,'unreadable',1,'fixture','{}',?2,0,1,1)",rusqlite::params![json!(format!("project:{PROJECT}")).to_string(),json!({"plan":{"$source":"unrestorable-history"},"large":"x".repeat(1_000_000)}).to_string()]).unwrap();
    let trace = performance::Trace::start();
    let request = command("finite", 1, json!({"title":"Finite"}));
    let prepared = prepare(store.as_ref(), &request).unwrap();
    let trace = trace.finish();
    for name in [
        "project.full_snapshot",
        "project.transaction",
        "source.restore_records",
        "workflow.compile",
    ] {
        assert!(trace["spans"][name].is_null(), "{trace}");
    }
    assert_eq!(prepared.read_set.versions().len(), 2);
    assert_eq!(prepared.changes.len(), 2);
    let wrong = ProjectUpdateContext::read(store.as_ref(), &request)
        .unwrap()
        .prepare(&request, &InvocationContext::human(Some(OTHER.into())))
        .unwrap_err();
    assert_eq!(wrong.code, "invalid_scope");
    for payload in [
        json!({}),
        json!({"title":null}),
        json!({"title":" "}),
        json!({"concurrency":0}),
        json!({"concurrency":33}),
        json!({"concurrency":-1}),
        json!({"concurrency":"8"}),
        json!({"runtime_preferences":{}}),
    ] {
        assert!(prepare(store.as_ref(), &command("invalid", 1, payload)).is_err());
    }
    let mut missing = request;
    missing.expected_revision = None;
    assert_eq!(
        ProjectUpdateContext::read(store.as_ref(), &missing)
            .err()
            .unwrap()
            .code,
        "precondition_required"
    );
}

#[test]
fn workspace_changes_guard_repository_membership_without_blocking_title_edits() {
    let root = tempfile::tempdir().unwrap();
    let store = fixture(root.path());
    let connection = rusqlite::Connection::open(
        SqliteStore::project_path(root.path(), PROJECT)
            .unwrap()
            .join("engineering.sqlite3"),
    )
    .unwrap();
    let workspace = command("move-workspace", 1, json!({"workspace":"/moved"}));
    let prepared = prepare(store.as_ref(), &workspace).unwrap();
    let title = prepare(
        store.as_ref(),
        &command("title", 1, json!({"title":"Allowed title edit"})),
    )
    .unwrap();
    connection.execute("INSERT INTO features(id,project_id,title,status,revision,input_version) VALUES('feature',?1,'Fixture','running',1,1)",[PROJECT]).unwrap();
    assert_eq!(
        store.commit_changes(&prepared).unwrap_err().code,
        "revision_conflict"
    );
    assert!(prepare(store.as_ref(), &workspace).is_err());
    assert!(
        title
            .read_set
            .versions()
            .iter()
            .all(|version| version.resource.kind != "collection")
    );
    store.commit_changes(&title).unwrap();
    let unchanged = command(
        "same-workspace",
        2,
        json!({"workspace":"/original","concurrency":4}),
    );
    store
        .commit_changes(&prepare(store.as_ref(), &unchanged).unwrap())
        .unwrap();
    let original = command("original-title", 3, json!({"title":"Title"}));
    let context = ProjectUpdateContext::read(store.as_ref(), &original).unwrap();
    let replaced = command("original-title", 3, json!({"workspace":"/bypass"}));
    assert!(
        context
            .prepare(&replaced, &InvocationContext::human(Some(PROJECT.into())))
            .is_err()
    );
}

#[test]
fn a_mainline_worktree_freezes_workspace_even_before_the_first_feature() {
    let root = tempfile::tempdir().unwrap();
    let store = fixture(root.path());
    let connection = rusqlite::Connection::open(
        SqliteStore::project_path(root.path(), PROJECT)
            .unwrap()
            .join("engineering.sqlite3"),
    )
    .unwrap();
    let request = command("before-mainline", 1, json!({"workspace":"/moved"}));
    let prepared = prepare(store.as_ref(), &request).unwrap();
    // Projection facts only, not a real Git receipt or worker execution.
    connection.execute("INSERT INTO repository_operations(id,project_id,action,status,receipt,reason,revision) VALUES('fixture-connect',?1,?2,'completed',NULL,'projection fixture',1)",rusqlite::params![PROJECT,json!({"kind":"connect","repository":"fixture/repo","mainline":"main"}).to_string()]).unwrap();
    connection.execute("INSERT INTO repository_worktrees(id,project_id,owner_id,kind,branch,path,base,operation_id) VALUES(?1,?1,?1,'mainline','main','/original',?2,'fixture-connect')",rusqlite::params![PROJECT,"a".repeat(40)]).unwrap();
    assert_eq!(
        store.commit_changes(&prepared).unwrap_err().code,
        "revision_conflict"
    );
    assert!(prepare(store.as_ref(), &request).is_err());
    assert!(
        prepare(
            store.as_ref(),
            &command("rename-with-mainline", 1, json!({"title":"Renamed"}))
        )
        .is_ok()
    );
}
