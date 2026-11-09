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
    );
    e.bootstrap().unwrap();
    e
}
fn legacy_store(path: &Path) -> SqliteStore {
    std::fs::create_dir_all(path).unwrap();
    std::fs::write(path.join(".devdesk-dataset"), "devdesk-schema-v1\n").unwrap();
    let db = rusqlite::Connection::open(path.join("engineering.sqlite3")).unwrap();
    db.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
    db.execute_batch(include_str!(
        "../../../tests/fixtures/legacy/engineering-v34.sql"
    ))
    .unwrap();
    drop(db);
    let artifacts: std::collections::BTreeMap<String, String> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/legacy/artifacts.json"
    ))
    .unwrap();
    for (relative, body) in artifacts {
        let file = path.join(relative);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, body).unwrap();
    }
    SqliteStore::open(path).unwrap()
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
            s.attempts
                .iter()
                .find(|x| x.id == target)
                .map(|x| x.revision)
        })
        .or_else(|| s.merges.iter().find(|x| x.id == target).map(|x| x.revision))
        .or_else(|| {
            s.contexts
                .iter()
                .find(|x| x.id == target)
                .map(|x| x.revision)
        })
        .or_else(|| {
            s.effects
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

#[test]
fn project_databases_migrate_full_history_and_keep_the_legacy_source() {
    let dir = tempfile::tempdir().unwrap();
    let old = legacy_store(dir.path());
    let before = serde_json::to_value(old.snapshot().unwrap()).unwrap();
    std::fs::write(
        dir.path().join("runtime.json"),
        r#"{"global_concurrency":7}"#,
    )
    .unwrap();
    assert!(GlobalStore::open(dir.path()).is_err());
    drop(old);
    let original = std::fs::read(dir.path().join("engineering.sqlite3")).unwrap();
    let global = GlobalStore::open(dir.path()).unwrap();
    assert_eq!(global.concurrency().unwrap(), 7);
    let (projects, count) = global.projects(0, 100).unwrap();
    assert_eq!(count, 2);
    assert_eq!(global.github_projects(0, 100).unwrap().1, 0);
    let mut combined = std::collections::HashMap::<String, usize>::new();
    let global_state = global.snapshot().unwrap();
    let mut events = global_state.events.len();
    for project in &projects {
        let store = SqliteStore::open_project(dir.path(), &project.id).unwrap();
        let state = store.snapshot().unwrap();
        assert_eq!(state.projects.len(), 1);
        assert_eq!(state.projects[0].id, project.id);
        assert_eq!(state.features.len(), 1);
        assert!(state.features.iter().all(|f| f.project_id == project.id));
        let artifacts = FileArtifacts::new(store.root().into());
        for artifact in &state.artifacts {
            assert!(!artifacts.read(artifact).unwrap().is_empty());
        }
        events += state.events.len();
        let json = serde_json::to_value(&state).unwrap();
        for (table, rows) in json.as_object().unwrap() {
            *combined.entry(table.clone()).or_default() += rows.as_array().unwrap().len();
        }
        let conn = rusqlite::Connection::open(
            std::path::PathBuf::from(store.root()).join("engineering.sqlite3"),
        )
        .unwrap();
        assert_eq!(
            conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get::<_, usize>(0)
            })
            .unwrap(),
            0
        );
        let fp:String=conn.query_row("SELECT fingerprint FROM operations WHERE json_extract(fingerprint,'$[0]')='project.create'",[],|r|r.get(0)).unwrap();
        let command_id: String = conn
            .query_row(
                "SELECT id FROM operations WHERE fingerprint=?1",
                [&fp],
                |r| r.get(0),
            )
            .unwrap();
        let values: Value = serde_json::from_str(&fp).unwrap();
        let command = Command {
            command_id,
            target: None,
            expected_revision: None,
            payload: values[3].clone(),
        };
        assert_eq!(global.reserve_project(&command).unwrap(), project.id);
    }
    for (table, rows) in before.as_object().unwrap() {
        if ![
            "governance",
            "permissions",
            "definitions",
            "harnesses",
            "sandboxes",
            "events",
        ]
        .contains(&table.as_str())
        {
            assert_eq!(
                combined[table],
                rows.as_array().unwrap().len(),
                "lost {table}"
            );
        }
    }
    assert_eq!(events, before["events"].as_array().unwrap().len());
    assert_eq!(
        serde_json::to_value(global_state.definitions).unwrap(),
        before["definitions"]
    );
    let conn = rusqlite::Connection::open(dir.path().join("global.sqlite3")).unwrap();
    assert_eq!(conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('features','runs','projects','deliveries')",[],|r|r.get::<_,usize>(0)).unwrap(),0);
    assert_eq!(
        original,
        std::fs::read(dir.path().join("engineering.sqlite3")).unwrap()
    );
    drop(conn);
    drop(global);
    let reopened = GlobalStore::open(dir.path()).unwrap();
    assert_eq!(reopened.projects(0, 100).unwrap().1, 2);
    assert_eq!(reopened.concurrency().unwrap(), 7);
}

#[test]
fn project_database_migration_retries_after_artifact_failure_without_publishing_partial_state() {
    let dir = tempfile::tempdir().unwrap();
    let old = legacy_store(dir.path());
    let f2 = old.snapshot().unwrap().features[1].id.clone();
    let artifact = old
        .snapshot()
        .unwrap()
        .artifacts
        .iter()
        .find(|a| a.feature_id.as_deref() == Some(f2.as_str()))
        .unwrap()
        .clone();
    let path = dir.path().join(&artifact.relative_path);
    let bytes = std::fs::read(&path).unwrap();
    drop(old);
    std::fs::write(&path, b"corrupt").unwrap();
    assert!(GlobalStore::open(dir.path()).is_err());
    let conn = rusqlite::Connection::open(dir.path().join("global.sqlite3")).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM project_catalog", [], |r| r
            .get::<_, usize>(0))
            .unwrap(),
        0
    );
    drop(conn);
    std::fs::write(path, bytes).unwrap();
    let global = GlobalStore::open(dir.path()).unwrap();
    assert_eq!(global.projects(0, 100).unwrap().1, 2);
}

#[test]
fn global_configuration_upgrades_a_copy_of_v1_and_rejects_project_data() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".devdesk-dataset"), "devdesk-schema-v1\n").unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("engineering.sqlite3")).unwrap();
    conn.execute_batch(include_str!(
        "../../../sql/project/foundation/001_initial.sql"
    ))
    .unwrap();
    drop(conn);
    let global = GlobalStore::open(dir.path()).unwrap();
    assert!(!global.snapshot().unwrap().definitions.is_empty());
    let mut state = global.snapshot().unwrap();
    let other = tempfile::tempdir().unwrap();
    let old = engine(&other.path().join("legacy"));
    new_feature(&old);
    state.projects = old.store.snapshot().unwrap().projects;
    assert!(
        global
            .transact(None, &mut |s| {
                s.projects = state.projects.clone();
                Ok(json!({}))
            })
            .is_err()
    );
    let conn = rusqlite::Connection::open(dir.path().join("engineering.sqlite3")).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        1
    );
    global.set_concurrency(9).unwrap();
    assert_eq!(global.concurrency().unwrap(), 9);
    assert!(global.set_concurrency(0).is_err());
    assert!(global.set_concurrency(33).is_err());
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
    assert!(state.runs.is_empty() && state.sessions.is_empty() && state.executions.is_empty());
}

#[test]
fn legacy_history_remains_readable_without_mutation_or_execution() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(legacy_store(dir.path()));
    let before = serde_json::to_value(store.snapshot().unwrap()).unwrap();
    let e = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(dir.path().into())),
        runtime::adapters(),
    );
    e.bootstrap().unwrap();
    e.tick(4).unwrap();
    let state = store.snapshot().unwrap();
    assert!(state.runs.iter().all(|r| r.origin == Origin::Simulated));
    assert!(e.query("query.workspace", &json!({}), 4).is_ok());
    assert!(
        cmd(
            &e,
            "project.update",
            Some(&state.projects[0].id),
            json!({"title":"Cannot rewrite history"})
        )
        .is_err()
    );
    assert_eq!(
        before,
        serde_json::to_value(store.snapshot().unwrap()).unwrap()
    );
}

// Recorded native boundary facts exercise Run acceptance separately from worker transport.
fn recorded_native_stage(e: &Engine, answer: &str, stopped: bool, cancelled: bool) -> Id {
    recorded_native_stage_with_kind(e, answer, stopped, cancelled, RuntimeExecutionKind::Harness)
}
fn recorded_native_stage_with_kind(
    e: &Engine,
    answer: &str,
    stopped: bool,
    cancelled: bool,
    kind: RuntimeExecutionKind,
) -> Id {
    let fid = new_feature(e);
    call(
        e,
        "document.revise",
        &fid,
        json!({"kind":"requirement","title":"Native scope","body":"Require observable native evidence"}),
    );
    let state = e.store.snapshot().unwrap();
    let project = state.feature(&fid).unwrap().project_id.clone();
    let (template, queued) = runtime::queue_conversation(e, &project);
    let mut session = e
        .store
        .snapshot()
        .unwrap()
        .sessions
        .iter()
        .find(|s| s.id == template)
        .unwrap()
        .clone();
    let sid = id();
    let aid = id();
    let rid = id();
    let tid = id();
    session.id = sid.clone();
    session.owner_id = fid.clone();
    session.feature_id = Some(fid.clone());
    session.scope = Some(SessionScope::Feature);
    session.agent = "builtin-agent-architect-v1".into();
    session.external_session = Some(format!("fixture-native-{sid}"));
    session.lifecycle = Some(SessionLifecycle::Idle);
    session.runtime_binding.as_mut().unwrap().work_type = WorkType::Design;
    let mut protocol = e
        .artifacts
        .seal(&fid, None, "protocol", "Explicit native protocol fixture")
        .unwrap();
    protocol.origin = Origin::External;
    e.store
        .transact(None, &mut |s| {
            let obsolete = s.turns.iter_mut().find(|t| t.id == queued).unwrap();
            obsolete.status = TurnStatus::Cancelled;
            obsolete.finished = Some(1);
            obsolete.revision += 1;
            let f = s.feature(&fid)?.clone();
            let context = s
                .contexts
                .iter()
                .find(|c| c.owner_id == fid)
                .unwrap()
                .clone();
            let mut binding = f.binding.clone();
            binding.agent = session.agent.clone();
            s.artifacts.push(protocol.clone());
            s.sessions.push(session.clone());
            s.attempts.push(Attempt {
                id: aid.clone(),
                feature_id: fid.clone(),
                task_id: None,
                phase: "requirement_definition".into(),
                baseline: Some("a".repeat(40)),
                source_base: Some("a".repeat(40)),
                live_sessions: [("requirement_validate".into(), sid.clone())].into(),
                git_context: None,
                status: Status::Running,
                input_version: f.input_version,
                documents: s
                    .documents
                    .iter()
                    .filter(|d| s.document_in_feature(d, &fid))
                    .map(|d| d.id.clone())
                    .collect(),
                outputs: vec![],
                scope: None,
                binding: binding.clone(),
                context_content: context.content,
                context_revision: context.revision,
                paused: false,
                revision: 1,
            });
            s.runs.push(Run {
                id: rid.clone(),
                attempt_id: Some(aid.clone()),
                merge_id: None,
                node: "requirement_validate".into(),
                dependencies: vec![],
                status: Status::Running,
                retry_of: None,
                session_id: sid.clone(),
                binding,
                binding_verified: true,
                writes: false,
                worktree: "fixture-readonly".into(),
                outcome: "external".into(),
                cancel_requested: cancelled,
                started: Some(1),
                finished: None,
                result: None,
                result_artifact: None,
                origin: Origin::External,
            });
            s.turns.push(ConversationTurn {
                id: tid.clone(),
                project_id: project.clone(),
                session_id: sid.clone(),
                client_message_id: id(),
                input: "Validate frozen requirement".into(),
                input_digest: "c".repeat(64),
                input_snapshot: None,
                code_view: None,
                source_revision: Some(f.revision),
                status: TurnStatus::Completed,
                external_message: Some("fixture-admitted-message".into()),
                external_turn: Some("fixture-turn".into()),
                consumed: true,
                cancel_requested: false,
                started: Some(1),
                finished: Some(2),
                run_id: Some(rid.clone()),
                revision: 2,
            });
            s.executions.push(RuntimeExecution {
                permissions: None,
                kind,
                id: id(),
                session_id: sid.clone(),
                run_id: Some(rid.clone()),
                operation_id: rid.clone(),
                container_id: Some("fixture-owned-container".into()),
                image_digest: format!("sha256:{}", "a".repeat(64)),
                status: if stopped {
                    Status::Released
                } else {
                    Status::Running
                },
                writer_revoked: stopped,
                started: Some(1),
                finished: stopped.then_some(2),
                protocol_artifact: stopped.then(|| protocol.id.clone()),
            });
            s.conversation_events.push(ConversationEvent {
                id: id(),
                session_id: sid.clone(),
                turn_id: tid.clone(),
                sequence: 1,
                deduplication_key: "native-stage-answer".into(),
                kind: ConversationEventKind::Assistant,
                mode: MessageMode::Full,
                provider_item: None,
                content: answer.into(),
                artifact_id: None,
                time: 2,
            });
            Ok(Value::Null)
        })
        .unwrap();
    rid
}
fn native_stage_answer() -> String {
    let checks = ["traceability", "scope", "tests", "failure_paths"]
        .map(|key| json!({"key":key,"passed":true,"evidence":"Explicit fixture input"}));
    json!({"summary":"Recorded native validation","checks":checks}).to_string()
}
#[test]
fn native_run_accepts_structured_output_after_writer_stop_and_protocol_export() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let rid = recorded_native_stage(&e, &native_stage_answer(), true, false);
    e.tick(4).unwrap();
    let s = e.store.snapshot().unwrap();
    let run = s.runs.iter().find(|r| r.id == rid).unwrap();
    assert_eq!(run.status, Status::Succeeded);
    assert!(run.result_artifact.is_some());
    assert!(s.runs.iter().all(|r| r.origin == Origin::External));
}
#[test]
fn native_run_cannot_accept_terminal_output_without_writer_stop_proof() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let rid = recorded_native_stage(&e, &native_stage_answer(), false, false);
    assert!(e.tick(4).is_err());
    let s = e.store.snapshot().unwrap();
    let run = s.runs.iter().find(|r| r.id == rid).unwrap();
    assert_eq!(run.status, Status::Running);
    assert!(run.result.is_none());
}
#[test]
fn native_run_cancellation_discards_a_late_successful_answer() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let rid = recorded_native_stage(&e, &native_stage_answer(), true, true);
    e.tick(4).unwrap();
    let s = e.store.snapshot().unwrap();
    assert_eq!(
        s.runs.iter().find(|r| r.id == rid).unwrap().status,
        Status::Cancelled
    );
    assert!(s.deliveries.is_empty());
}
#[test]
fn native_run_rejects_unstructured_output_without_a_fallback_result() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let rid = recorded_native_stage(&e, "Unstructured provider output", true, false);
    e.tick(4).unwrap();
    let s = e.store.snapshot().unwrap();
    let run = s.runs.iter().find(|r| r.id == rid).unwrap();
    assert_eq!(run.status, Status::Failed);
    assert_eq!(
        run.result.as_ref().unwrap()["failure_type"],
        "adapter_contract"
    );
}

struct StopOnlyTests(std::sync::atomic::AtomicUsize);
impl ports::Tests for StopOnlyTests {
    fn poll_test(&self, _: &TestRequest) -> Result<Option<Value>> {
        panic!("recovery must never redispatch an interrupted test")
    }
    fn stop_test(&self, _: &TestRequest) -> Result<bool> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(true)
    }
    fn cleanup_test(&self, _: &TestRequest) -> Result<()> {
        Ok(())
    }
}
#[test]
fn restarting_native_tests_stops_the_old_writer_without_replaying_or_accepting_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = engine(dir.path());
    let tests = Arc::new(StopOnlyTests(0.into()));
    e.tests = tests.clone();
    let rid = recorded_native_stage_with_kind(
        &e,
        &native_stage_answer(),
        false,
        false,
        RuntimeExecutionKind::Tests,
    );
    e.store
        .transact(None, &mut |s| {
            let run = s.runs.iter().find(|r| r.id == rid).unwrap();
            let execution = s
                .executions
                .iter()
                .find(|x| x.run_id.as_ref() == Some(&rid))
                .unwrap();
            let request = TestRequest {
                operation_id: rid.clone(),
                session_id: Some(run.session_id.clone()),
                phase: "red".into(),
                candidate: "a".repeat(40),
                test_definitions: vec![],
                environment: execution.id.clone(),
                origin: Origin::External,
                definitions: vec![],
                code: None,
                runtime: None,
            };
            s.effects.push(EffectRecord {
                id: format!("{rid}:Test"),
                run_id: rid.clone(),
                kind: EffectKind::Test,
                status: EffectStatus::Pending,
                request: json!({"test":request,"adapter":e.tests.identity()}),
                receipt: None,
                revision: 1,
                created: 1,
                updated: 1,
                reason: "Recorded native test intent".into(),
            });
            Ok(Value::Null)
        })
        .unwrap();
    e.bootstrap().unwrap();
    assert!(e.store.has_pending_work().unwrap());
    e.tick(4).unwrap();
    let s = e.store.snapshot().unwrap();
    assert_eq!(tests.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        s.runs.iter().find(|r| r.id == rid).unwrap().status,
        Status::Interrupted
    );
    let effect = s.effects.iter().find(|x| x.run_id == rid).unwrap();
    assert_eq!(effect.status, EffectStatus::Discarded);
    assert!(effect.receipt.is_none());
    let execution = s
        .executions
        .iter()
        .find(|x| x.run_id.as_ref() == Some(&rid))
        .unwrap();
    assert!(execution.writer_revoked && execution.protocol_artifact.is_some());
    assert_eq!(execution.status, Status::Released);
    assert!(s.deliveries.is_empty());
    assert_eq!(s.runs.len(), 1);
}

#[test]
fn failed_workflow_review_waits_for_explicit_retry_and_preserves_its_binding() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let validation = recorded_native_stage(&e, &native_stage_answer(), true, false);
    e.tick(4).unwrap();
    let state = e.store.snapshot().unwrap();
    let source = state.runs.iter().find(|r| r.id == validation).unwrap();
    let attempt = source.attempt_id.clone().unwrap();
    let fid = state.attempt(&attempt).unwrap().feature_id.clone();
    let project = state.feature(&fid).unwrap().project_id.clone();
    for _ in 0..3 {
        e.evaluate_workflows().unwrap();
        for intent in e
            .pending_workflow_intents()
            .unwrap()
            .into_iter()
            .filter(|r| r.data["method"] == "snapshot.save")
        {
            let result = e.execute(
                "snapshot.save",
                serde_json::from_value(intent.data["command"].clone()).unwrap(),
            );
            e.observe_workflow_intent(&intent.id, &result).unwrap();
            result.unwrap();
        }
    }
    // The native validation has finished; associate its already persisted facts
    // with the workflow execution step, without dispatching a model or Docker.
    e.store
        .transact(None, &mut |s| {
            let record = s
                .governance
                .iter_mut()
                .find(|r| r.kind == "workflow_instance" && r.definition_id == fid)
                .unwrap();
            let mut workflow: WorkflowRuntime = record.decode()?;
            let step = workflow
                .steps
                .iter_mut()
                .find(|step| step.key == "requirement_execute")
                .unwrap();
            step.outputs.insert("attempt".into(), attempt.clone());
            step.status = "active".into();
            record.data = serde_json::to_value(workflow).unwrap();
            record.content_hash = configuration_hash(&record.data);
            record.revision += 1;
            Ok(Value::Null)
        })
        .unwrap();
    e.evaluate_workflows().unwrap();
    let intent = e
        .pending_workflow_intents()
        .unwrap()
        .into_iter()
        .find(|r| r.data["method"] == "delivery.submit")
        .unwrap_or_else(|| {
            let s = e.store.snapshot().unwrap();
            panic!(
                "workflow={} attempt={:?}",
                s.governance
                    .iter()
                    .find(|r| r.kind == "workflow_instance" && r.definition_id == fid)
                    .unwrap()
                    .data,
                s.attempt(&attempt).unwrap().status
            );
        });
    let command: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
    let result = e.execute("delivery.submit", command);
    e.observe_workflow_intent(&intent.id, &result).unwrap();
    let delivery = result.unwrap()["id"].as_str().unwrap().to_owned();
    e.evaluate_workflows().unwrap();
    let mut reviewer = e
        .store
        .snapshot()
        .unwrap()
        .sessions
        .iter()
        .find(|s| s.id == source.session_id)
        .unwrap()
        .clone();
    reviewer.id = id();
    reviewer.agent = "builtin-agent-reviewer-v1".into();
    reviewer.runtime_binding.as_mut().unwrap().work_type = WorkType::Review;
    reviewer.external_session = Some(format!("fixture-reviewer-{}", reviewer.id));
    e.store
        .transact(None, &mut |s| {
            s.sessions.push(reviewer.clone());
            Ok(Value::Null)
        })
        .unwrap();
    let intent = e
        .pending_workflow_intents()
        .unwrap()
        .into_iter()
        .find(|r| r.data["method"] == "review.request")
        .unwrap();
    let mut command: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
    command.payload["review_session"] = json!(reviewer.id);
    let result = e.execute("review.request", command);
    e.observe_workflow_intent(&intent.id, &result).unwrap();
    let review_run = result.unwrap()["id"].as_str().unwrap().to_owned();
    e.store.transact(None, &mut |s| {
        let run = s.runs.iter_mut().find(|r| r.id == review_run).unwrap();
        run.status = Status::Failed; run.started = Some(1); run.finished = Some(2);
        run.result = Some(json!({"failure_type":"adapter_contract","message":"Reviewer output did not match the JSON contract"}));
        Ok(Value::Null)
    }).unwrap();
    let failed = e
        .store
        .snapshot()
        .unwrap()
        .runs
        .iter()
        .find(|r| r.id == review_run)
        .unwrap()
        .clone();
    let workflow = |e: &Engine| -> WorkflowRuntime {
        e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .find(|r| r.kind == "workflow_instance" && r.definition_id == fid)
            .unwrap()
            .decode()
            .unwrap()
    };
    for _ in 0..3 {
        e.evaluate_workflows().unwrap();
    }
    let step = workflow(&e)
        .steps
        .into_iter()
        .find(|step| step.key == "requirement_review")
        .unwrap();
    assert_eq!(step.status, "failed");
    assert!(step.reason.contains("JSON contract"));
    let state = e.store.snapshot().unwrap();
    assert!(!state.accepted(state.delivery(&delivery).unwrap()));
    assert_eq!(
        state.runs.iter().filter(|r| r.node == failed.node).count(),
        1
    );
    e.execute(
        "workflow.step.retry",
        Command {
            command_id: id(),
            target: Some(step.id.clone()),
            expected_revision: Some(step.revision),
            payload: json!({}),
        },
    )
    .unwrap();
    e.evaluate_workflows().unwrap();
    assert_eq!(
        workflow(&e)
            .steps
            .iter()
            .find(|s| s.key == "requirement_review")
            .unwrap()
            .status,
        "active"
    );
    let intent = e
        .pending_workflow_intents()
        .unwrap()
        .into_iter()
        .find(|r| r.data["method"] == "run.retry")
        .unwrap();
    let result = e.execute(
        "run.retry",
        serde_json::from_value(intent.data["command"].clone()).unwrap(),
    );
    e.observe_workflow_intent(&intent.id, &result).unwrap();
    let retry = result.unwrap()["id"].as_str().unwrap().to_owned();
    let state = e.store.snapshot().unwrap();
    let run = state.runs.iter().find(|r| r.id == retry).unwrap();
    assert_eq!(run.retry_of.as_deref(), Some(failed.id.as_str()));
    assert_eq!(json!(run.binding), json!(failed.binding));
    let sid = run.session_id.clone();
    assert_eq!(
        state.sessions.iter().find(|s| s.id == sid).unwrap().agent,
        reviewer.agent
    );
    let mut protocol = e
        .artifacts
        .seal(
            &fid,
            None,
            "protocol",
            "Recorded native Reviewer retry protocol",
        )
        .unwrap();
    protocol.origin = Origin::External;
    let turn = id();
    e.store
        .transact(None, &mut |s| {
            let run = s.runs.iter_mut().find(|r| r.id == retry).unwrap();
            run.status = Status::Running;
            run.started = Some(3);
            let session = s.sessions.iter_mut().find(|s| s.id == sid).unwrap();
            session
                .external_session
                .get_or_insert_with(|| format!("fixture-reviewer-{sid}"));
            session.lifecycle = Some(SessionLifecycle::Idle);
            s.artifacts.push(protocol.clone());
            s.turns.push(ConversationTurn {
                id: turn.clone(),
                project_id: project.clone(),
                session_id: sid.clone(),
                client_message_id: id(),
                input: "Review the frozen requirement delivery".into(),
                input_digest: "d".repeat(64),
                input_snapshot: None,
                code_view: None,
                source_revision: None,
                status: TurnStatus::Completed,
                external_message: Some("fixture-review-message".into()),
                external_turn: Some("fixture-review-turn".into()),
                consumed: true,
                cancel_requested: false,
                started: Some(3),
                finished: Some(4),
                run_id: Some(retry.clone()),
                revision: 1,
            });
            s.executions.push(RuntimeExecution {
                permissions: None,
                kind: RuntimeExecutionKind::Harness,
                id: id(),
                session_id: sid.clone(),
                run_id: Some(retry.clone()),
                operation_id: retry.clone(),
                container_id: Some("fixture-review-container".into()),
                image_digest: format!("sha256:{}", "a".repeat(64)),
                status: Status::Released,
                writer_revoked: true,
                started: Some(3),
                finished: Some(4),
                protocol_artifact: Some(protocol.id.clone()),
            });
            s.conversation_events.push(ConversationEvent {
                id: id(),
                session_id: sid.clone(),
                turn_id: turn.clone(),
                sequence: 1,
                deduplication_key: "native-review-answer".into(),
                kind: ConversationEventKind::Assistant,
                mode: MessageMode::Full,
                provider_item: None,
                content: native_stage_answer(),
                artifact_id: None,
                time: 4,
            });
            Ok(Value::Null)
        })
        .unwrap();
    e.tick(4).unwrap();
    e.evaluate_workflows().unwrap();
    let state = e.store.snapshot().unwrap();
    assert_eq!(
        state.runs.iter().find(|r| r.id == retry).unwrap().status,
        Status::Succeeded
    );
    assert_eq!(
        json!(state.runs.iter().find(|r| r.id == failed.id).unwrap()),
        json!(failed)
    );
    assert_eq!(
        state.runs.iter().filter(|r| r.node == failed.node).count(),
        2
    );
    assert_eq!(
        workflow(&e)
            .steps
            .iter()
            .find(|step| step.key == "requirement_review")
            .unwrap()
            .status,
        "completed"
    );
}
