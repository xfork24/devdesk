use devdesk_adapters::*;
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
fn engine(path: &Path) -> Engine {
    let store = Arc::new(SqliteStore::open(path).unwrap());
    let e = Engine::new(
        store,
        Arc::new(FileArtifacts::new(path.into())),
        runtime::adapters(),
    )
    .with_tool_executor(Arc::new(
        devdesk_adapters::integrations::NativeTools::new(path).unwrap(),
    ));
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    e
}
fn revision(e: &Engine, target: &str) -> Option<u64> {
    let s = e.store.snapshot().unwrap();
    s.projects
        .iter()
        .find(|x| x.id == target)
        .map(|x| x.revision)
        .or_else(|| {
            s.features
                .iter()
                .find(|x| x.id == target)
                .map(|x| x.revision)
        })
        .or_else(|| s.tasks.iter().find(|x| x.id == target).map(|x| x.revision))
        .or_else(|| {
            s.contexts
                .iter()
                .find(|x| x.id == target)
                .map(|x| x.revision)
        })
}

fn cmd(e: &Engine, m: &str, t: Option<&str>, p: Value) -> Result<Value> {
    e.execute(
        m,
        Command {
            command_id: id(),
            target: t.map(str::to_owned),
            expected_revision: t.and_then(|t| revision(e, t)),
            payload: p,
        },
    )
}

fn call(e: &Engine, m: &str, t: &str, p: Value) -> Value {
    let result = cmd(e, m, if t.is_empty() { None } else { Some(t) }, p)
        .unwrap_or_else(|err| panic!("{m} {t}: {err}"));
    if m == "merge.apply" {
        e.tick(4).unwrap();
    }
    result
}

fn result_id(v: Value) -> String {
    v["id"].as_str().unwrap().into()
}

fn new_feature(e: &Engine) -> String {
    let pid = result_id(call(e, "project.create", "", json!({"title":"测试项目"})));
    runtime::configure_project(e, &pid);
    result_id(call(
        e,
        "feature.create",
        "",
        json!({"project_id":pid,"title":"导出功能"}),
    ))
}

#[test]
fn revision_conflicts_and_pagination_are_enforced() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    assert!(
        e.execute(
            "attempt.start",
            Command {
                command_id: id(),
                target: Some(f.clone()),
                expected_revision: Some(999),
                payload: json!({"phase":"planning"})
            }
        )
        .is_err()
    );
    let value = e.query("query.history", &json!({"limit":1}), 4).unwrap();
    assert_eq!(value["events"].as_array().unwrap().len(), 1);
}

#[test]
fn combined_history_sorts_before_paging_and_reuses_redacted_call_projections() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let time = 9_000_000_000_000;
    e.store.transact(None, &mut |s| {
        let first = s.events.last().map_or(1, |e| e.id + 1);
        for (index, (name, delta)) in [("newest-event", 300), ("oldest-event", 100), ("middle-event", 200)].into_iter().enumerate() {
            s.events.push(Event { id: first + index as u64, target: "old-call".into(), kind: name.into(), detail: name.into(), time: time + delta });
        }
        for (id, kind, delta) in [("new-call", "tool_call", 400), ("old-call", "tool_call", 150), ("platform-call", "platform_call", 250)] {
            let data = if kind == "platform_call" {
                json!({"status":"succeeded","request":{"definition":{"qualified_name":"workspace.current"},"arguments":{}},"receipt":{"path":"/work"}})
            } else {
                json!({"status":"succeeded","invocation":{"tool":{"definition":{"sensitive_parameters":["/password"]}},"arguments":{"password":"secret-value","visible":"kept"}}})
            };
            s.governance.push(ConfigurationRecord {
                id: id.into(), kind: kind.into(), definition_id: id.into(), scope: Scope::Global,
                name: id.into(), version: 1, supersedes: None, content_hash: configuration_hash(&data), data,
                immutable: false, revision: 1, created_at: time + delta,
            });
        }
        Ok(Value::Null)
    }).unwrap();
    let query = |offset| {
        e.query(
            "query.history",
            &json!({"include_tool_calls":true,"offset":offset,"limit":3}),
            4,
        )
        .unwrap()
    };
    let first = query(0);
    assert_eq!(first["total"], e.store.snapshot().unwrap().events.len() + 3);
    assert_eq!(first["records"][0]["call"]["id"], "new-call");
    assert_eq!(first["records"][1]["event"]["kind"], "newest-event");
    assert_eq!(first["records"][2]["call"]["id"], "platform-call");
    assert_eq!(
        first["records"][0]["call"]["data"]["invocation"]["arguments"]["password"],
        "[redacted]"
    );
    assert_eq!(
        first["records"][0]["call"]["data"]["invocation"]["arguments"]["visible"],
        "kept"
    );
    assert_eq!(
        first["records"][2]["call"]["data"]["invocation"]["arguments"],
        json!({})
    );
    let second = query(3);
    assert_eq!(second["records"][0]["event"]["kind"], "middle-event");
    assert_eq!(second["records"][1]["call"]["id"], "old-call");
    assert_eq!(second["records"][2]["event"]["kind"], "oldest-event");
    assert_eq!(query(0), first);
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .configuration("new-call")
            .unwrap()
            .data["invocation"]["arguments"]["password"],
        "secret-value"
    );
    let events = e
        .query("query.history", &json!({"target":"old-call","limit":3}), 4)
        .unwrap();
    assert_eq!(events["total"], 3);
    assert_eq!(events["events"][0]["kind"], "newest-event");
    assert!(events.get("records").is_none());
}

#[cfg(unix)]
#[test]
fn artifact_symlinks_cannot_escape_dataset() {
    use std::os::unix::fs::symlink;
    let d = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    std::fs::remove_dir(d.path().join("artifacts")).unwrap();
    symlink(outside.path(), d.path().join("artifacts")).unwrap();
    assert!(
        e.artifacts
            .seal("feature", None, "documents", "test")
            .is_err()
    );
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    drop(e);
    assert!(SqliteStore::open(d.path()).is_err());
}

#[cfg(unix)]
#[test]
fn artifact_read_rejects_symlink_replacement() {
    use std::os::unix::fs::symlink;
    let d = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let a = e
        .artifacts
        .seal("feature", None, "documents", "original")
        .unwrap();
    std::fs::rename(d.path().join("artifacts"), out.path().join("moved")).unwrap();
    symlink(out.path().join("moved"), d.path().join("artifacts")).unwrap();
    assert!(e.artifacts.read(&a).is_err());
}

#[test]
fn hardening_document_target_cannot_bypass_revision() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let fid = new_feature(&e);
    assert!(
        cmd(
            &e,
            "document.revise",
            None,
            json!({"feature_id":fid,"kind":"requirement","body":"x"})
        )
        .is_err()
    );
    assert!(
        e.execute(
            "document.revise",
            Command {
                command_id: id(),
                target: Some(fid),
                expected_revision: None,
                payload: json!({"kind":"requirement","body":"x"})
            }
        )
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn current_databases_exclude_retired_permission_storage_and_conversion_indexes() {
    let dir = tempfile::tempdir().unwrap();
    let _global = GlobalStore::open(dir.path()).unwrap();
    let project = id();
    let _project = SqliteStore::open_project(dir.path(), &project).unwrap();
    for (path, version) in [
        (dir.path().join("global.sqlite3"), 15),
        (
            SqliteStore::project_path(dir.path(), &project)
                .unwrap()
                .join("engineering.sqlite3"),
            46,
        ),
    ] {
        let db = rusqlite::Connection::open(path).unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            version
        );
        let definition_columns: Vec<String> = db
            .prepare("PRAGMA table_info(definitions)")
            .unwrap()
            .query_map([], |r| r.get(1))
            .unwrap()
            .map(|row| row.unwrap())
            .collect();
        assert!(
            !definition_columns
                .iter()
                .any(|name| ["slots", "rules"].contains(&name.as_str()))
        );
        if version == 44 {
            for table in ["projects", "features", "tasks", "snapshots"] {
                let columns: Vec<String> = db
                    .prepare(&format!("PRAGMA table_info({table})"))
                    .unwrap()
                    .query_map([], |r| r.get(1))
                    .unwrap()
                    .map(|row| row.unwrap())
                    .collect();
                assert!(
                    !columns
                        .iter()
                        .any(|name| ["binding", "runtime_preferences"].contains(&name.as_str()))
                );
            }
        }
        assert_eq!(db.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('permissions','idx_governance_conversion_source','idx_execution_task')", [], |r|r.get::<_, u32>(0)).unwrap(), 0);
    }
}

#[cfg(unix)]
#[test]
fn global_and_project_database_paths_reject_symlink_redirection() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".devdesk-dataset"), "devdesk-schema-v1\n").unwrap();
    std::fs::write(outside.path().join("db"), b"do not modify").unwrap();
    symlink(outside.path().join("db"), dir.path().join("global.sqlite3")).unwrap();
    assert!(GlobalStore::open(dir.path()).is_err());
    assert_eq!(
        std::fs::read(outside.path().join("db")).unwrap(),
        b"do not modify"
    );
    std::fs::remove_file(dir.path().join("global.sqlite3")).unwrap();
    symlink(outside.path(), dir.path().join("projects")).unwrap();
    assert!(GlobalStore::open(dir.path()).is_err());
    assert!(SqliteStore::open_project(dir.path(), &id()).is_err());
    std::fs::remove_file(dir.path().join("projects")).unwrap();
    std::fs::create_dir(dir.path().join("projects")).unwrap();
    let pid = id();
    symlink(outside.path(), dir.path().join("projects").join(&pid)).unwrap();
    assert!(SqliteStore::open_project(dir.path(), &pid).is_err());
    assert!(SqliteStore::open_project(dir.path(), "../escape").is_err());
}

#[test]
fn native_configuration_has_no_prototype_provider_or_execution_controls() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let state = e.store.snapshot().unwrap();
    assert!(
        state.harnesses.iter().all(
            |h| h.origin == Origin::External && ["codex", "dsh"].contains(&h.provider.as_str())
        )
    );
    assert!(
        state
            .sandboxes
            .iter()
            .all(|s| s.origin == Origin::External && s.provider == "docker")
    );
    assert!(!state.definitions.iter().any(|d| d.body.contains("模拟")));
    let fid = new_feature(&e);
    assert!(
        cmd(
            &e,
            "attempt.start",
            Some(&fid),
            json!({"phase":"planning","scenario":"success"})
        )
        .is_err()
    );
    assert!(cmd(&e, "attempt.start", Some(&fid), json!({"phase":"planning"})).is_err());
    assert!(cmd(&e, "run.continue", None, json!({})).is_err());
    assert!(cmd(&e,"harness.create",None,json!({"name":"Unsupported provider","data":{"provider":"simulation","runtime_version":"1","capabilities":[],"model_providers":[]}})).is_err());
    let state = e.store.snapshot().unwrap();
    assert!(state.projects.iter().all(|p| p.execution_mode == "live"));
    assert!(state.sessions.is_empty() && state.executions.is_empty());
}

// Recorded native boundary facts exercise Run acceptance separately from worker transport.

#[test]
fn empty_or_cleared_datasets_initialize_despite_hidden_metadata() {
    for marker in [
        None,
        Some("devdesk-schema-v1\n"),
        Some("devdesk-schema-v1\\n"),
        Some("devdesk-source-v2\n"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join(".DS_Store"), b"Finder metadata").unwrap();
        std::fs::write(directory.path().join(".devdesk.lock"), b"").unwrap();
        if let Some(marker) = marker {
            std::fs::write(directory.path().join(".devdesk-dataset"), marker).unwrap();
        }
        let store = GlobalStore::open(directory.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(directory.path().join(".devdesk-dataset")).unwrap(),
            "devdesk-source-v2\n"
        );
        assert!(directory.path().join("global.sqlite3").is_file());
        drop(store);
        let reopened = GlobalStore::open(directory.path()).unwrap();
        assert!(reopened.snapshot().is_ok());
    }
}

#[test]
fn cleared_dataset_reinitialization_cannot_replace_an_active_lock() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join(".devdesk-dataset");
    std::fs::write(&marker, "old marker").unwrap();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.path().join(".devdesk.lock"))
        .unwrap();
    fs2::FileExt::try_lock_exclusive(&lock).unwrap();
    let error = GlobalStore::open(directory.path()).err().unwrap();
    assert_eq!(error.code, "dataset_locked");
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "old marker");
    assert!(!directory.path().join("global.sqlite3").exists());
    fs2::FileExt::unlock(&lock).unwrap();
    assert!(GlobalStore::open(directory.path()).is_ok());
}

#[test]
fn foreign_contents_are_not_treated_as_a_cleared_dataset() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("notes.txt"), "keep").unwrap();
    assert!(GlobalStore::open(directory.path()).is_err());
    assert!(!directory.path().join(".devdesk-dataset").exists());
    assert!(!directory.path().join(".devdesk.lock").exists());
    std::fs::write(directory.path().join(".devdesk-dataset"), "old marker").unwrap();
    assert!(GlobalStore::open(directory.path()).is_err());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("notes.txt")).unwrap(),
        "keep"
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join(".devdesk-dataset")).unwrap(),
        "old marker"
    );
}
