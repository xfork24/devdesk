//! Predicate tokens are maintained by SQLite for every fact writer; no I/O or
//! business lock is acquired by this read-set protection.
use devdesk_adapters::SqliteStore;
use devdesk_core::{EngineeringStore, concurrency::*};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
const PROJECT: &str = "c8516249-60a3-444a-988d-e5bf0e257611";
fn fixture(root: &std::path::Path) -> (SqliteStore, Connection) {
    let store = SqliteStore::open_project(root, PROJECT).unwrap();
    let connection = Connection::open(
        SqliteStore::project_path(root, PROJECT)
            .unwrap()
            .join("engineering.sqlite3"),
    )
    .unwrap();
    connection.execute("INSERT INTO projects(id,title,workspace,execution_mode,concurrency,revision) VALUES(?1,'Fixture','','live',2,1)",[PROJECT]).unwrap();
    (store, connection)
}
fn revision(store: &dyn EngineeringStore, key: &ResourceKey) -> u64 {
    store.read_resources(std::slice::from_ref(key)).unwrap()[0].revision
}
fn operation(connection: &Connection, id: &str) {
    connection.execute("INSERT INTO repository_operations(id,project_id,action,status,receipt,reason,revision) VALUES(?1,?2,?3,'prepared',NULL,'',1)",params![id,PROJECT,json!({"kind":"connect","repository":"fixture/repo","mainline":"main"}).to_string()]).unwrap();
}
fn guarded_context(command: &str, collection: ResourceKey, version: u64) -> PreparedChangeSet {
    let context = ResourceKey::new("context", command);
    PreparedChangeSet {
        command_id: command.into(),
        fingerprint: command.into(),
        read_set: ReadSet::new(vec![
            ResourceVersion {
                resource: collection,
                revision: version,
            },
            ResourceVersion {
                resource: context.clone(),
                revision: 0,
            },
        ])
        .unwrap(),
        changes: vec![ResourceChange {
            resource: context,
            value: json!({"id":command,"owner_id":PROJECT,"content":"guarded result","revision":1}),
        }],
        result: Value::Null,
        time: 1,
    }
}
#[test]
fn repository_collection_tokens_detect_insert_update_delete_and_aba() {
    let root = tempfile::tempdir().unwrap();
    let (store, connection) = fixture(root.path());
    let key = repository_collection(PROJECT);
    let original = revision(&store, &key);
    assert_eq!(original, 0);
    let stale = guarded_context("stale-absence", key.clone(), original);
    operation(&connection, "op");
    let inserted = revision(&store, &key);
    assert!(inserted > original);
    connection
        .execute(
            "UPDATE repository_operations SET status='completed',revision=revision+1 WHERE id='op'",
            [],
        )
        .unwrap();
    let completed = revision(&store, &key);
    assert!(completed > inserted);
    connection
        .execute("DELETE FROM repository_operations WHERE id='op'", [])
        .unwrap();
    assert!(revision(&store, &key) > completed);
    assert_eq!(
        store.commit_changes(&stale).unwrap_err().code,
        "revision_conflict"
    );
    assert!(
        store
            .read_resources(&[ResourceKey::new("context", "stale-absence")])
            .unwrap()[0]
            .value
            .is_none()
    );
}
#[test]
fn predicate_updates_roll_back_and_readers_observe_the_committed_token() {
    let root = tempfile::tempdir().unwrap();
    let (store, connection) = fixture(root.path());
    let key = repository_collection(PROJECT);
    connection.execute_batch("BEGIN IMMEDIATE;").unwrap();
    operation(&connection, "uncommitted");
    assert_eq!(revision(&store, &key), 0);
    connection.execute_batch("ROLLBACK;").unwrap();
    assert_eq!(revision(&store, &key), 0);
    store
        .commit_changes(&guarded_context("valid-after-rollback", key.clone(), 0))
        .unwrap();
    // A caller cannot mutate a storage-maintained collection token itself.
    let forged = PreparedChangeSet {
        command_id: "forged-counter".into(),
        fingerprint: "forged-counter".into(),
        read_set: ReadSet::new(vec![ResourceVersion {
            resource: key.clone(),
            revision: 0,
        }])
        .unwrap(),
        changes: vec![ResourceChange {
            resource: key,
            value: json!(100),
        }],
        result: Value::Null,
        time: 1,
    };
    assert_eq!(
        store.commit_changes(&forged).unwrap_err().code,
        "invalid_params"
    );
}
#[test]
fn feature_membership_changes_without_conflicting_on_unrelated_feature_edits() {
    let root = tempfile::tempdir().unwrap();
    let (store, connection) = fixture(root.path());
    let features = feature_collection(PROJECT);
    let repository = repository_collection(PROJECT);
    connection.execute("INSERT INTO features(id,project_id,title,status,revision,input_version) VALUES('feature',?1,'Feature','running',1,1)",[PROJECT]).unwrap();
    let inserted = revision(&store, &features);
    assert_eq!(inserted, 1);
    assert_eq!(revision(&store, &repository), 0);
    connection
        .execute(
            "UPDATE features SET title='Renamed',revision=revision+1 WHERE id='feature'",
            [],
        )
        .unwrap();
    assert_eq!(revision(&store, &features), inserted);
    connection
        .execute("DELETE FROM features WHERE id='feature'", [])
        .unwrap();
    assert!(revision(&store, &features) > inserted);
}
#[test]
fn repository_heads_worktrees_and_git_calls_advance_only_the_relevant_token() {
    let root = tempfile::tempdir().unwrap();
    let (store, connection) = fixture(root.path());
    let key = repository_collection(PROJECT);
    connection
        .execute(
            "INSERT INTO heads(id,project_id,revision) VALUES('main',?1,'revision')",
            [PROJECT],
        )
        .unwrap();
    let head = revision(&store, &key);
    assert!(head > 0);
    connection
        .execute("UPDATE heads SET revision='next' WHERE id='main'", [])
        .unwrap();
    assert!(revision(&store, &key) > head);
    operation(&connection, "op");
    let before = revision(&store, &key);
    connection.execute("INSERT INTO repository_worktrees(id,project_id,owner_id,kind,branch,path,base,operation_id) VALUES('work',?1,?1,'mainline','main','/fixture','revision','op')",[PROJECT]).unwrap();
    assert!(revision(&store, &key) > before);
    let record = |id: &str, scope: &str, name: &str| {
        connection.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES(?1,'platform_call',?1,?2,?1,1,'fixture','{}',?3,0,1,1)",params![id,json!(scope).to_string(),json!({"status":"prepared","request":{"definition":{"qualified_name":name}}}).to_string()]).unwrap();
    };
    let before = revision(&store, &key);
    record("non-git", &format!("project:{PROJECT}"), "context.read");
    assert_eq!(revision(&store, &key), before);
    record("git", &format!("feature:{PROJECT}:feature"), "git.status");
    assert!(revision(&store, &key) > before);
    let before = revision(&store, &key);
    connection.execute("UPDATE governance_records SET data=json_set(data,'$.status','completed') WHERE id='git'",[]).unwrap();
    assert_eq!(revision(&store, &key), before + 1);
    record("global-git", "global", "git.status");
    let before = revision(&store, &key);
    assert!(
        connection
            .execute(
                "UPDATE governance_records SET scope=?1 WHERE id='global-git'",
                [json!(format!("project:{PROJECT}")).to_string()]
            )
            .is_err()
    );
    assert_eq!(revision(&store, &key), before);
    record("next-git", &format!("project:{PROJECT}"), "git.status");
    assert_eq!(revision(&store, &key), before + 1);
}
