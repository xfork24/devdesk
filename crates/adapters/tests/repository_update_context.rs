//! Preparation only: SQL facts are fixtures, not actual Git receipts or a worker run.
use devdesk_adapters::{SqliteStore, performance};
use devdesk_core::{
    Command, EngineeringStore, InvocationContext, concurrency::*, repository_updates::*,
};
use devdesk_domain::{Project, Workspace};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::sync::{Arc, Barrier};
const PROJECT: &str = "c8516249-60a3-444a-988d-e5bf0e257611";
fn fixture(root: &std::path::Path) -> (Arc<SqliteStore>, Connection) {
    let store = Arc::new(SqliteStore::open_project(root, PROJECT).unwrap());
    let project = Project {
        id: PROJECT.into(),
        title: "Fixture".into(),
        workspace: "/fixture/repository".into(),
        workspace_definition: Some(Workspace {
            repository: "fixture/repo".into(),
            path: "/fixture/repository".into(),
            mainline: "main".into(),
        }),
        connection: None,
        execution_mode: "live".into(),
        concurrency: 2,
        revision: 1,
    };
    let key = ResourceKey::new("project", PROJECT);
    store
        .commit_changes(&PreparedChangeSet {
            command_id: "fixture-project".into(),
            fingerprint: "fixture-project".into(),
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
    let connection = Connection::open(
        SqliteStore::project_path(root, PROJECT)
            .unwrap()
            .join("engineering.sqlite3"),
    )
    .unwrap();
    (store, connection)
}
fn command(id: &str, payload: Value) -> Command {
    Command {
        command_id: id.into(),
        target: Some(PROJECT.into()),
        expected_revision: Some(1),
        payload,
    }
}
fn registration(store: &dyn EngineeringStore, id: &str) -> PreparedChangeSet {
    let command = command(id, json!({"repository":"fixture/repo","mainline":"main"}));
    RepositoryRegistrationContext::read(store, &command)
        .unwrap()
        .prepare(&command, &InvocationContext::human(Some(PROJECT.into())))
        .unwrap()
}
fn mainline(connection: &Connection) {
    connection.execute("INSERT INTO repository_operations(id,project_id,action,status,receipt,reason,revision) VALUES('bootstrap',?1,?2,'completed',NULL,'projection fixture',1)",params![PROJECT,json!({"kind":"connect","repository":"fixture/repo","mainline":"main"}).to_string()]).unwrap();
    connection.execute("INSERT INTO repository_worktrees(id,project_id,owner_id,kind,branch,path,base,operation_id) VALUES(?1,?1,?1,'mainline','main','/fixture/worktree',?2,'bootstrap')",params![PROJECT,"a".repeat(40)]).unwrap();
    connection
        .execute(
            "INSERT INTO heads(id,project_id,revision) VALUES(?1,?2,?3)",
            params![format!("main:{PROJECT}"), PROJECT, "a".repeat(40)],
        )
        .unwrap();
}
#[test]
fn one_hundred_same_snapshot_registrations_create_one_intent_with_original_actor() {
    let root = tempfile::tempdir().unwrap();
    let (store, connection) = fixture(root.path());
    let prepared = (0..100)
        .map(|n| registration(store.as_ref(), &format!("registration-{n}")))
        .collect::<Vec<_>>();
    let barrier = Arc::new(Barrier::new(100));
    let threads = prepared
        .into_iter()
        .map(|prepared| {
            let (store, barrier) = (store.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                store.commit_changes(&prepared)
            })
        })
        .collect::<Vec<_>>();
    let mut winners = 0;
    for thread in threads {
        match thread.join().unwrap() {
            Ok(_) => winners += 1,
            Err(error) => assert_eq!(error.code, "revision_conflict"),
        }
    }
    assert_eq!(winners, 1);
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM repository_operations", [], |row| row
                .get::<_, u64>(
                0
            ))
            .unwrap(),
        1
    );
    let actor:String=connection.query_row("SELECT json_extract(c.data,'$.context.actor.kind') FROM governance_records i JOIN governance_records c ON c.id=json_extract(i.data,'$.command_context_id') WHERE i.type='repository_intent' AND c.type='command_context'",[],|row|row.get(0)).unwrap();
    assert_eq!(actor, "human");
}
#[test]
fn a_feature_created_after_preparation_invalidates_the_registration_predicate() {
    let root = tempfile::tempdir().unwrap();
    let (store, connection) = fixture(root.path());
    let prepared = registration(store.as_ref(), "before-feature");
    connection.execute("INSERT INTO features(id,project_id,title,status,revision,input_version) VALUES('feature',?1,'Feature','running',1,1)",[PROJECT]).unwrap();
    assert_eq!(
        store.commit_changes(&prepared).unwrap_err().code,
        "revision_conflict"
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM repository_operations", [], |row| row
                .get::<_, u64>(
                0
            ))
            .unwrap(),
        0
    );
    let command = command("after-feature", json!({"repository":"fixture/repo"}));
    assert!(RepositoryRegistrationContext::read(store.as_ref(), &command).is_err());
}
#[test]
fn refresh_is_fenced_by_head_and_git_activity_without_loading_project_history() {
    let root = tempfile::tempdir().unwrap();
    let (store, connection) = fixture(root.path());
    mainline(&connection);
    connection.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('history','workflow_instance','owner',?1,'history',1,'fixture','{}',?2,0,1,1)",params![json!(format!("project:{PROJECT}")).to_string(),json!({"root_instance_id":"unrestorable-root","plan":{"$source":"unrestorable-history"},"large":"x".repeat(1_000_000)}).to_string()]).unwrap();
    let command = command("refresh", json!({"revision":"b".repeat(40)}));
    let trace = performance::Trace::start();
    let prepared = RepositoryRefreshContext::read(store.as_ref(), &command)
        .unwrap()
        .prepare(&command, &InvocationContext::human(Some(PROJECT.into())))
        .unwrap();
    let trace = trace.finish();
    for name in [
        "project.full_snapshot",
        "project.transaction",
        "source.restore_records",
        "workflow.compile",
    ] {
        assert!(trace["spans"][name].is_null(), "{trace}");
    }
    // A raw head update also advances the collection token, even though a head
    // has no native numeric revision. The stale prepared intent cannot commit.
    connection
        .execute("UPDATE heads SET revision=?1", ["c".repeat(40)])
        .unwrap();
    assert_eq!(
        store.commit_changes(&prepared).unwrap_err().code,
        "revision_conflict"
    );
    connection.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('git-call','platform_call','call',?1,'Git',1,'fixture','{}',?2,0,1,1)",params![json!(format!("feature:{PROJECT}:feature")).to_string(),json!({"status":"needs_reconciliation","request":{"definition":{"qualified_name":"git.status"}}}).to_string()]).unwrap();
    assert!(RepositoryRefreshContext::read(store.as_ref(), &command).is_err());
}
#[test]
fn unrelated_membership_changes_do_not_act_as_a_project_wide_write_lock() {
    let root = tempfile::tempdir().unwrap();
    let (store, connection) = fixture(root.path());
    mainline(&connection);
    let command = command("independent-refresh", json!({"revision":"b".repeat(40)}));
    let prepared = RepositoryRefreshContext::read(store.as_ref(), &command)
        .unwrap()
        .prepare(&command, &InvocationContext::human(Some(PROJECT.into())))
        .unwrap();
    assert!(
        !prepared
            .read_set
            .versions()
            .iter()
            .any(|guard| guard.resource == feature_collection(PROJECT))
    );
    connection.execute("INSERT INTO features(id,project_id,title,status,revision,input_version) VALUES('unrelated',?1,'Unrelated','running',1,1)",[PROJECT]).unwrap();
    store.commit_changes(&prepared).unwrap();
}
