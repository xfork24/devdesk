use devdesk_adapters::{GlobalStore, SqliteStore};
use devdesk_core::{EngineeringStore, id};
use std::path::Path;

fn reserve(root: &Path, project: &str, published: bool) {
    let conn = rusqlite::Connection::open(root.join("global.sqlite3")).unwrap();
    conn.execute(
        "INSERT INTO project_catalog(id,command_id,fingerprint,project) VALUES(?1,?1,'fixture',?2)",
        rusqlite::params![project, published.then_some("{}")],
    )
    .unwrap();
}
fn reject_unchanged(root: &Path, message: &str) {
    let path = root.join("global.sqlite3");
    let before = std::fs::read(&path).unwrap();
    let error = GlobalStore::open(root).err().unwrap();
    assert!(error.message.contains(message), "{error:?}");
    assert_eq!(std::fs::read(path).unwrap(), before);
}
#[test]
fn incompatible_registered_projects_are_rejected_before_global_writes() {
    let root = tempfile::tempdir().unwrap();
    GlobalStore::open(root.path()).unwrap();
    let project = id();
    let store = SqliteStore::open_project(root.path(), &project).unwrap();
    let path = Path::new(&store.root()).join("engineering.sqlite3");
    drop(store);
    reserve(root.path(), &project, true);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("PRAGMA user_version=55;").unwrap();
    drop(conn);
    let before = std::fs::read(&path).unwrap();
    reject_unchanged(root.path(), "数据集协议不兼容");
    assert_eq!(std::fs::read(path).unwrap(), before);
}
#[test]
fn incomplete_reservations_are_reopenable_but_existing_incompatible_files_are_not() {
    let root = tempfile::tempdir().unwrap();
    GlobalStore::open(root.path()).unwrap();
    let project = id();
    reserve(root.path(), &project, false);
    GlobalStore::open(root.path()).unwrap();
    let store = SqliteStore::open_project(root.path(), &project).unwrap();
    let path = Path::new(&store.root()).join("engineering.sqlite3");
    drop(store);
    GlobalStore::open(root.path()).unwrap();
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch("PRAGMA user_version=55;").unwrap();
    drop(conn);
    reject_unchanged(root.path(), "数据集协议不兼容");
}
#[test]
fn global_discussion_compatibility_is_checked_before_global_writes() {
    let root = tempfile::tempdir().unwrap();
    GlobalStore::open(root.path()).unwrap();
    let path = SqliteStore::global_discussion_path(root.path()).unwrap();
    let store = SqliteStore::open(&path).unwrap();
    drop(store);
    GlobalStore::open(root.path()).unwrap();
    let conn = rusqlite::Connection::open(path.join("engineering.sqlite3")).unwrap();
    conn.execute_batch("PRAGMA user_version=55;").unwrap();
    drop(conn);
    reject_unchanged(root.path(), "数据集协议不兼容");
}
#[test]
fn current_projects_and_legacy_discussions_can_open_the_same_dataset() {
    let root = tempfile::tempdir().unwrap();
    GlobalStore::open(root.path()).unwrap();
    let project = id();
    SqliteStore::open_project(root.path(), &project).unwrap();
    reserve(root.path(), &project, true);
    let discussion = SqliteStore::global_discussion_path(root.path()).unwrap();
    SqliteStore::open(&discussion).unwrap();
    let conn = rusqlite::Connection::open(discussion.join("engineering.sqlite3")).unwrap();
    conn.execute_batch("PRAGMA user_version=60;").unwrap();
    drop(conn);
    GlobalStore::open(root.path()).unwrap();
    SqliteStore::open_project(root.path(), &project).unwrap();
    SqliteStore::open(&discussion).unwrap();
}

#[test]
fn missing_published_project_is_not_silently_recreated() {
    let root = tempfile::tempdir().unwrap();
    GlobalStore::open(root.path()).unwrap();
    let project = id();
    reserve(root.path(), &project, true);
    reject_unchanged(root.path(), "数据库缺失");
    assert!(
        !SqliteStore::project_path(root.path(), &project)
            .unwrap()
            .exists()
    );
}

#[test]
fn missing_removed_project_does_not_block_the_dataset() {
    let root = tempfile::tempdir().unwrap();
    GlobalStore::open(root.path()).unwrap();
    let project = id();
    reserve(root.path(), &project, true);
    let connection = rusqlite::Connection::open(root.path().join("global.sqlite3")).unwrap();
    connection
        .execute(
            "INSERT INTO settings(key,value) VALUES(?1,'true')",
            [&GlobalStore::project_removal_key(&project).id],
        )
        .unwrap();
    drop(connection);
    let global = GlobalStore::open(root.path()).unwrap();
    assert!(global.removed_project_missing(&project).unwrap());
    assert!(global.project_removed(&project).unwrap());
}

#[test]
fn copied_project_matches_repository_and_branch_and_reserves_its_original_id() {
    let root = tempfile::tempdir().unwrap();
    let global = GlobalStore::open(root.path()).unwrap();
    let copy = |id: &str| {
        let store = SqliteStore::open_project(root.path(), id).unwrap();
        let project_root = SqliteStore::project_path(root.path(), id).unwrap();
        let checkout = project_root.join("repository");
        std::fs::create_dir(&checkout).unwrap();
        let db = rusqlite::Connection::open(project_root.join("engineering.sqlite3")).unwrap();
        db.execute(
            "INSERT INTO projects(id,title,workspace,execution_mode,concurrency,revision,workspace_definition)
             VALUES(?1,'Historical project',?2,'live',2,3,json_object('repository','Owner/Repo','mainline','main','path',?2))",
            rusqlite::params![id, checkout.to_str().unwrap()],
        )
        .unwrap();
        drop(store);
    };
    let original = id();
    copy(&original);
    assert!(
        global
            .copied_github_project(root.path(), "owner/repo", "next")
            .unwrap()
            .is_none()
    );
    let project = global
        .copied_github_project(root.path(), "owner/repo", "main")
        .unwrap()
        .unwrap();
    assert_eq!(project.id, original);
    global.reserve_copied_project(&project).unwrap();
    global.reserve_copied_project(&project).unwrap();
    let duplicate = id();
    copy(&duplicate);
    assert_eq!(
        global
            .copied_github_project(root.path(), "owner/repo", "main")
            .unwrap_err()
            .code,
        "project_ambiguous"
    );
    global.publish_project(&project).unwrap();
    assert_eq!(
        global
            .copied_github_project(root.path(), "owner/repo", "main")
            .unwrap()
            .unwrap()
            .id,
        duplicate
    );
}

#[test]
fn the_previous_intent_layouts_are_rejected_without_changing_project_or_global_files() {
    for version in [56, 57, 58] {
        let root = tempfile::tempdir().unwrap();
        GlobalStore::open(root.path()).unwrap();
        let project = id();
        let store = SqliteStore::open_project(root.path(), &project).unwrap();
        let path = Path::new(&store.root()).join("engineering.sqlite3");
        drop(store);
        reserve(root.path(), &project, true);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(&format!("PRAGMA user_version={version};"))
            .unwrap();
        drop(conn);
        let before = std::fs::read(&path).unwrap();
        reject_unchanged(root.path(), "数据集协议不兼容");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(SqliteStore::open_project(root.path(), &project).is_err());
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}

#[test]
fn same_numbered_but_different_resource_layout_is_rejected_without_repair() {
    let root = tempfile::tempdir().unwrap();
    GlobalStore::open(root.path()).unwrap();
    for legacy in ["single-host-resource-v1", "single-host-resource-v2"] {
        let conn = rusqlite::Connection::open(root.path().join("global.sqlite3")).unwrap();
        conn.execute("UPDATE dataset_layout SET contract=?1 WHERE id=1", [legacy])
            .unwrap();
        drop(conn);
        reject_unchanged(root.path(), "数据集资源协议不兼容");
    }
}

#[test]
fn pre_integration_global_resource_layout_is_rejected_without_writes() {
    let root = tempfile::tempdir().unwrap();
    GlobalStore::open(root.path()).unwrap();
    let conn = rusqlite::Connection::open(root.path().join("global.sqlite3")).unwrap();
    conn.execute_batch("PRAGMA user_version=23;").unwrap();
    drop(conn);
    reject_unchanged(root.path(), "数据集协议不兼容");
}
