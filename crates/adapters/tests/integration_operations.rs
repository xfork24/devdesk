//! Operation admission and receipt ports only. No Git clone, Docker or network.
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
use devdesk_adapters::{FileArtifacts, SqliteStore, performance};
use devdesk_core::{
    Command, Engine, EngineeringStore, InvocationContext, concurrency::*, integration_operations::*,
};
use devdesk_domain::*;
use serde_json::{Value, json};
use std::sync::{Arc, Barrier, mpsc};
fn fixture(path: &std::path::Path) -> Arc<Engine> {
    Arc::new(Engine::new(
        Arc::new(SqliteStore::open(path).unwrap()),
        Arc::new(FileArtifacts::new(path.into())),
        runtime::adapters(),
    ))
}
fn command(id: &str) -> Command {
    Command {
        command_id: id.into(),
        target: None,
        expected_revision: None,
        payload: json!({"name":"Frozen package","markdown":"# Frozen input"}),
    }
}
fn input(c: &Command) -> Value {
    json!({"package_hash":configuration_hash(&c.payload),"source":"explicit protocol fixture"})
}
fn admit(engine: &Engine, id: &str) -> Box<IntegrationOperation> {
    let c = command(id);
    match engine
        .prepare_integration_operation(
            "skill.import_package",
            &c,
            input(&c),
            &InvocationContext::human(None),
        )
        .unwrap()
    {
        IntegrationOperationAdmission::Admitted(operation) => operation,
        _ => panic!("expected a new admission"),
    }
}
fn count(engine: &Engine, kind: &str) -> u64 {
    let conn = rusqlite::Connection::open(
        std::path::Path::new(&engine.store.root()).join("engineering.sqlite3"),
    )
    .unwrap();
    conn.query_row(
        "SELECT count(*) FROM governance_records WHERE type=?1",
        [kind],
        |r| r.get(0),
    )
    .unwrap()
}
#[test]
fn one_hundred_same_commands_grant_one_external_attempt_and_replay_only_after_completion() {
    let root = tempfile::tempdir().unwrap();
    let engine = fixture(root.path());
    let barrier = Arc::new(Barrier::new(100));
    let threads = (0..100)
        .map(|_| {
            let (engine, barrier) = (engine.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                let c = command("same");
                engine.prepare_integration_operation(
                    "skill.import_package",
                    &c,
                    input(&c),
                    &InvocationContext::human(None),
                )
            })
        })
        .collect::<Vec<_>>();
    let mut admitted = None;
    let mut refused = 0;
    for thread in threads {
        match thread.join().unwrap() {
            Ok(IntegrationOperationAdmission::Admitted(op)) => {
                assert!(admitted.is_none());
                admitted = Some(op)
            }
            Err(error) => {
                assert_eq!(error.code, "needs_reconciliation");
                refused += 1
            }
            Ok(IntegrationOperationAdmission::Replay(_)) => panic!("not completed yet"),
        }
    }
    assert_eq!(refused, 99);
    assert_eq!(count(&engine, "integration_operation"), 1);
    assert_eq!(count(&engine, "command_context"), 1);
    let op = Arc::new(admitted.unwrap());
    let barrier = Arc::new(Barrier::new(100));
    let threads = (0..100)
        .map(|_| {
            let (engine, op, barrier) = (engine.clone(), op.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                engine.finish_integration_operation(&op, &Ok(json!({"id":"verified-result"})))
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap().unwrap();
    }
    assert_eq!(count(&engine, "integration_observation"), 1);
    assert_eq!(engine.store.event_cursor().unwrap(), 3);
    let c = command("same");
    assert_eq!(
        engine
            .replay_integration_operation(
                "skill.import_package",
                &c,
                &InvocationContext::human(None)
            )
            .unwrap()
            .unwrap(),
        json!({"id":"verified-result"})
    );
    assert!(matches!(
        engine
            .prepare_integration_operation(
                "skill.import_package",
                &c,
                json!({"changed":"not re-evaluated"}),
                &InvocationContext::human(None)
            )
            .unwrap(),
        IntegrationOperationAdmission::Replay(_)
    ));
    let mut changed = c;
    changed.payload["markdown"] = json!("other input");
    assert_eq!(
        engine
            .replay_integration_operation(
                "skill.import_package",
                &changed,
                &InvocationContext::human(None)
            )
            .unwrap_err()
            .code,
        "invalid_params"
    );
}
#[test]
fn contradictory_callbacks_preserve_every_fact_and_only_one_original_version_is_projected() {
    let root = tempfile::tempdir().unwrap();
    let engine = fixture(root.path());
    let op = Arc::new(admit(&engine, "competing"));
    let barrier = Arc::new(Barrier::new(100));
    let threads = (0..100)
        .map(|n| {
            let (engine, op, barrier) = (engine.clone(), op.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                engine.finish_integration_operation(&op, &Ok(json!({"observation":n})))
            })
        })
        .collect::<Vec<_>>();
    let mut accepted = 0;
    for thread in threads {
        match thread.join().unwrap() {
            Ok(()) => accepted += 1,
            Err(error) => assert_eq!(error.code, "revision_conflict"),
        }
    }
    assert_eq!(accepted, 1);
    assert_eq!(count(&engine, "integration_observation"), 100);
    let row = engine
        .store
        .read_resources(&[ResourceKey::new("governance", op.id())])
        .unwrap()
        .remove(0);
    assert_eq!(row.revision, 2);
    let record = ConfigurationRecord::from_storage(row.value.unwrap()).unwrap();
    assert_eq!(record.revision, 2);
    assert_eq!(record.data["status"], "succeeded");
}
#[test]
fn independent_intents_and_completion_do_not_take_legacy_writer_or_restore_history() {
    let root = tempfile::tempdir().unwrap();
    let engine = fixture(root.path());
    let (ready, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let old = engine.clone();
    let writer = std::thread::spawn(move || {
        old.store.seed_fixture_state(None, &mut |state| {
            ready.send(()).unwrap();
            blocked.recv().unwrap();
            state.events.push(Event {
                id: 1,
                target: "legacy".into(),
                kind: "test.old".into(),
                detail: "fixture".into(),
                time: 1,
            });
            Ok(Value::Null)
        })
    });
    waiting.recv().unwrap();
    let conn = rusqlite::Connection::open(root.path().join("engineering.sqlite3")).unwrap();
    conn.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('unrestorable','workflow_instance','owner','\"global\"','History',1,'fixture','{}',?1,0,1,1)",[json!({"root_instance_id":"unrestorable-root","plan":{"$source":"missing-frozen-source"},"large":"x".repeat(1_000_000)}).to_string()]).unwrap();
    let barrier = Arc::new(Barrier::new(100));
    let threads = (0..100)
        .map(|n| {
            let (engine, barrier) = (engine.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                let trace = performance::Trace::start();
                let result = (|| {
                    let c = command(&format!("independent-{n}"));
                    let op = match engine.prepare_integration_operation(
                        "skill.import_package",
                        &c,
                        input(&c),
                        &InvocationContext::human(None),
                    )? {
                        IntegrationOperationAdmission::Admitted(op) => op,
                        IntegrationOperationAdmission::Replay(_) => {
                            return Err(err(
                                "fixture_unexpected_replay",
                                "independent new intent replayed",
                            ));
                        }
                    };
                    engine.finish_integration_operation(
                        &op,
                        &Err(err("fixture_unknown", "unknown external result").technical_unknown()),
                    )
                })();
                (result, trace.finish())
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    release.send(()).unwrap();
    writer.join().unwrap().unwrap();
    for (result, report) in results {
        result.unwrap();
        for stage in [
            "project.full_snapshot",
            "project.transaction",
            "source.restore_records",
            "workflow.compile",
        ] {
            assert!(report["spans"][stage].is_null(), "{report}");
        }
    }
    assert_eq!(count(&engine, "integration_operation"), 100);
    assert_eq!(count(&engine, "integration_observation"), 100);
    let row = engine
        .store
        .read_resources(&[ResourceKey::new("governance", "unrestorable")])
        .unwrap()
        .remove(0);
    assert!(row.value.is_some());
}
#[test]
fn stale_completion_journals_its_result_and_wrong_scope_is_rejected_before_journaling() {
    let root = tempfile::tempdir().unwrap();
    let engine = fixture(root.path());
    let op = admit(&engine, "late");
    let row = engine
        .store
        .read_resources(&[ResourceKey::new("governance", op.id())])
        .unwrap()
        .remove(0);
    let mut record = ConfigurationRecord::from_storage(row.value.unwrap()).unwrap();
    record.data["status"] = json!("cancelled");
    record.revision += 1;
    record.content_hash = content_digest(&record.refs, &record.data);
    let value = record.to_storage();
    engine
        .store
        .commit_changes(&PreparedChangeSet {
            command_id: "newer-control".into(),
            fingerprint: "explicit trusted fixture".into(),
            read_set: ReadSet::new(vec![ResourceVersion {
                resource: row.resource.clone(),
                revision: row.revision,
            }])
            .unwrap(),
            changes: vec![ResourceChange {
                resource: row.resource,
                value,
            }],
            result: Value::Null,
            time: 1,
        })
        .unwrap();
    assert_eq!(
        engine
            .finish_integration_operation(&op, &Ok(json!({"late":"receipt"})))
            .unwrap_err()
            .code,
        "revision_conflict"
    );
    assert_eq!(count(&engine, "integration_observation"), 1);
    let other_root = tempfile::tempdir().unwrap();
    let other_store = Arc::new(
        SqliteStore::open_project(other_root.path(), "2c662228-162c-4590-a8c1-3410d4f9fe89")
            .unwrap(),
    );
    let other = Engine::new(
        other_store.clone(),
        Arc::new(FileArtifacts::new(other_store.root().into())),
        runtime::adapters(),
    );
    assert_eq!(
        other
            .finish_integration_operation(&op, &Ok(json!({})))
            .unwrap_err()
            .code,
        "invalid_scope"
    );
    assert_eq!(other.store.event_cursor().unwrap(), 0);
}

#[test]
fn discovery_intent_guards_the_exact_binding_and_service_sources_before_admission() {
    let root = tempfile::tempdir().unwrap();
    let engine = fixture(root.path());
    engine.bootstrap().unwrap();
    let create = |method: &str, payload: Value| {
        engine
            .execute(
                method,
                Command {
                    command_id: devdesk_core::id(),
                    target: None,
                    expected_revision: None,
                    payload,
                },
            )
            .unwrap()
    };
    let source = create(
        "integration.create",
        json!({"name":"Fixture service","data":{"source":{"kind":"http_api","base_url":"https://example.test"}}}),
    );
    let binding = create(
        "integration.binding.create",
        json!({"name":"Fixture binding","data":{"integration_ref":source["id"],"harness_ref":"builtin-harness-codex-managed:1","sandbox_ref":"builtin-sandbox-docker:2","allowed_origins":["https://example.test"],"allowed_tools":[]}}),
    );
    let c = Command {
        command_id: "discovery".into(),
        target: Some(binding["id"].as_str().unwrap().into()),
        expected_revision: Some(1),
        payload: json!({"binding_ref":binding["id"]}),
    };
    let mut input = json!({"binding_ref":binding["id"],"binding_hash":"wrong","source_ref":source["id"],"source_hash":source["content_hash"],"image_id":"fixture image","permission_selection":{"fixture":"not executed"}});
    let before = engine.store.event_cursor().unwrap();
    assert!(
        engine
            .prepare_integration_operation(
                "integration.discover",
                &c,
                input.clone(),
                &InvocationContext::human(None)
            )
            .is_err()
    );
    assert_eq!(engine.store.event_cursor().unwrap(), before);
    input["binding_hash"] = binding["content_hash"].clone();
    let mut obsolete = c.clone();
    obsolete.command_id = "obsolete-discovery".into();
    obsolete.expected_revision = Some(99);
    assert!(
        matches!(engine.prepare_integration_operation("integration.discover",&obsolete,input.clone(),&InvocationContext::human(None)),Err(error) if error.code=="revision_conflict")
    );
    assert_eq!(engine.store.event_cursor().unwrap(), before);
    let operation = match engine
        .prepare_integration_operation(
            "integration.discover",
            &c,
            input,
            &InvocationContext::human(None),
        )
        .unwrap()
    {
        IntegrationOperationAdmission::Admitted(op) => op,
        _ => panic!("expected admission"),
    };
    engine
        .finish_integration_operation(&operation, &Ok(json!({"tools":[]})))
        .unwrap();
    let saved = engine
        .store
        .read_resources(&[ResourceKey::new("governance", operation.id())])
        .unwrap()
        .remove(0);
    let record = ConfigurationRecord::from_storage(saved.value.unwrap()).unwrap();
    assert_eq!(record.data["inputs"]["binding_ref"], binding["id"]);
    assert_eq!(record.data["inputs"]["source_ref"], source["id"]);
}

#[test]
fn external_fact_values_and_source_like_text_are_never_interpreted_as_configuration() {
    let root = tempfile::tempdir().unwrap();
    let engine = fixture(root.path());
    let c = command("literal");
    let mut frozen = input(&c);
    let encoded = "\u{1e}devdesk-source-text:{\"$source\":\"source\",\"ref\":\"absent\"}";
    frozen["source"] = json!(encoded);
    let op = match engine
        .prepare_integration_operation(
            "skill.import_package",
            &c,
            frozen,
            &InvocationContext::human(None),
        )
        .unwrap()
    {
        IntegrationOperationAdmission::Admitted(op) => op,
        _ => panic!("expected admission"),
    };
    let result = json!({"$source":"untrusted-observation","text":encoded});
    engine
        .finish_integration_operation(&op, &Ok(result.clone()))
        .unwrap();
    let state = engine.store.snapshot().unwrap();
    let saved = state.configuration(op.id()).unwrap();
    assert_eq!(saved.data["value"]["value"], result);
    assert_eq!(saved.data["inputs"]["source"], encoded);
    assert_eq!(
        engine
            .replay_integration_operation(
                "skill.import_package",
                &c,
                &InvocationContext::human(None)
            )
            .unwrap()
            .unwrap(),
        result
    );
}

fn discovery_sources(engine: &Engine) -> (Value, Value) {
    engine.bootstrap().unwrap();
    let create = |method, payload| {
        engine
            .execute(
                method,
                Command {
                    command_id: devdesk_core::id(),
                    target: None,
                    expected_revision: None,
                    payload,
                },
            )
            .unwrap()
    };
    let source = create(
        "integration.create",
        json!({"name":"Discovery service","data":{"source":{"kind":"http_api","base_url":"https://discovery.example.test"}}}),
    );
    let binding = create(
        "integration.binding.create",
        json!({"name":"Discovery binding","data":{"integration_ref":source["id"],"harness_ref":"builtin-harness-codex-managed:1","sandbox_ref":"builtin-sandbox-docker:2","allowed_origins":["https://discovery.example.test"],"allowed_tools":[]}}),
    );
    (source, binding)
}
fn admit_discovery(
    engine: &Engine,
    source: &Value,
    binding: &Value,
    id: &str,
) -> (Command, Box<IntegrationOperation>) {
    let command = Command {
        command_id: id.into(),
        target: Some(binding["id"].as_str().unwrap().into()),
        expected_revision: Some(binding["revision"].as_u64().unwrap()),
        payload: json!({"binding_ref":binding["id"]}),
    };
    let input = json!({"binding_ref":binding["id"],"binding_hash":binding["content_hash"],"source_ref":source["id"],"source_hash":source["content_hash"],"image_id":"explicit fixture, no I/O","permission_selection":{"fixture":true}});
    let op = match engine
        .prepare_integration_operation(
            "integration.discover",
            &command,
            input,
            &InvocationContext::human(None),
        )
        .unwrap()
    {
        IntegrationOperationAdmission::Admitted(op) => op,
        _ => panic!("expected fresh intent"),
    };
    (command, op)
}
#[test]
fn discovery_publication_is_atomic_and_one_hundred_callbacks_replay_without_legacy_writer() {
    let root = tempfile::tempdir().unwrap();
    let engine = fixture(root.path());
    let (source, binding) = discovery_sources(&engine);
    let (command, op) = admit_discovery(&engine, &source, &binding, "directory-once");
    let op = Arc::new(op);
    let (ready, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let old = engine.clone();
    let writer = std::thread::spawn(move || {
        old.store.seed_fixture_state(None, &mut |_| {
            ready.send(()).unwrap();
            blocked.recv().unwrap();
            Ok(Value::Null)
        })
    });
    waiting.recv().unwrap();
    let conn = rusqlite::Connection::open(root.path().join("engineering.sqlite3")).unwrap();
    conn.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('unrestorable-discovery','workflow_instance','fixture','\"global\"','History',1,'fixture','{}',?1,0,1,1)",[json!({"root_instance_id":"unrestorable-root","plan":{"$source":"missing-frozen-source"}}).to_string()]).unwrap();
    let result = Arc::new(Ok(
        json!({"tools":[{"name":"sum","inputSchema":{"type":"object"}}]}),
    ));
    let barrier = Arc::new(Barrier::new(100));
    let threads = (0..100)
        .map(|_| {
            let (engine, op, result, barrier) =
                (engine.clone(), op.clone(), result.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                let trace = performance::Trace::start();
                let result = engine.finish_tool_discovery(&op, &result);
                (result, trace.finish())
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    release.send(()).unwrap();
    writer.join().unwrap().unwrap();
    let expected = results[0].0.as_ref().unwrap().clone();
    for (result, trace) in results {
        assert_eq!(result.unwrap(), expected);
        for span in [
            "project.full_snapshot",
            "project.transaction",
            "source.restore_records",
            "workflow.compile",
        ] {
            assert!(trace["spans"][span].is_null(), "{trace}");
        }
    }
    assert_eq!(expected["version"], 1);
    assert_eq!(expected["data"]["diff"]["added"], json!(["sum"]));
    assert_eq!(count(&engine, "integration_discovery"), 1);
    assert_eq!(count(&engine, "integration_observation"), 1);
    let replay = engine
        .replay_integration_operation(
            "integration.discover",
            &command,
            &InvocationContext::human(None),
        )
        .unwrap()
        .unwrap();
    assert_eq!(replay, expected);
    let head = engine
        .store
        .read_resources(&[ResourceKey::new(
            "integration_discovery_head",
            binding["id"].as_str().unwrap(),
        )])
        .unwrap()
        .remove(0);
    assert_eq!(head.revision, 1);
    assert_eq!(head.value.unwrap()["id"], expected["id"]);
    let plan:String=conn.query_row("EXPLAIN QUERY PLAN SELECT version FROM governance_records WHERE type='integration_discovery' AND definition_id=?1 ORDER BY version DESC LIMIT 1",[binding["id"].as_str().unwrap()],|r|r.get(3)).unwrap();
    assert!(plan.contains("idx_governance_type"), "{plan}");
    assert!(!plan.contains("TEMP B-TREE"), "{plan}");
}
#[test]
fn concurrent_discoveries_preserve_conflicting_supplier_facts_and_the_original_directory_baseline()
{
    let root = tempfile::tempdir().unwrap();
    let engine = fixture(root.path());
    let (source, binding) = discovery_sources(&engine);
    let (_, first) = admit_discovery(&engine, &source, &binding, "first");
    let (second_command, second) = admit_discovery(&engine, &source, &binding, "second");
    let a = engine
        .finish_tool_discovery(
            &first,
            &Ok(json!({"tools":[{"name":"old","description":"before"},{"name":"removed"}]})),
        )
        .unwrap();
    let conflict = Ok(json!({"tools":[{"name":"contradictory"}]}));
    for _ in 0..2 {
        assert_eq!(
            engine
                .finish_tool_discovery(&second, &conflict)
                .unwrap_err()
                .code,
            "revision_conflict"
        );
    }
    assert_eq!(count(&engine, "integration_discovery"), 1);
    assert_eq!(count(&engine, "integration_observation"), 2);
    assert_eq!(
        engine
            .replay_integration_operation(
                "integration.discover",
                &second_command,
                &InvocationContext::human(None)
            )
            .unwrap_err()
            .code,
        "needs_reconciliation"
    );
    let (_, third) = admit_discovery(&engine, &source, &binding, "third");
    let b = engine
        .finish_tool_discovery(
            &third,
            &Ok(json!({"tools":[{"name":"old","description":"after"},{"name":"added"}]})),
        )
        .unwrap();
    assert_ne!(a["id"], b["id"]);
    assert_eq!(b["version"], 2);
    assert_eq!(
        b["data"]["diff"],
        json!({"added":["added"],"removed":["removed"],"changed":["old"]})
    );
    let op = engine
        .store
        .read_resources(&[ResourceKey::new("governance", second.id())])
        .unwrap()
        .remove(0);
    let op = ConfigurationRecord::from_storage(op.value.unwrap()).unwrap();
    assert_eq!(op.data["inputs"]["discovery_baseline"]["version"], 0);
    assert_eq!(op.data["status"], "prepared");
}
#[test]
fn invalid_discovery_is_journaled_before_rejection_and_callbacks_cannot_change_namespace() {
    let root = tempfile::tempdir().unwrap();
    let engine = fixture(root.path());
    let (source, binding) = discovery_sources(&engine);
    let (_, op) = admit_discovery(&engine, &source, &binding, "invalid");
    let other_root = tempfile::tempdir().unwrap();
    let other = Engine::new(
        Arc::new(devdesk_adapters::GlobalStore::open(other_root.path()).unwrap()),
        Arc::new(FileArtifacts::new(other_root.path().into())),
        runtime::adapters(),
    );
    assert_eq!(
        other
            .finish_tool_discovery(&op, &Ok(json!({"tools":[]})))
            .unwrap_err()
            .code,
        "invalid_scope"
    );
    assert_eq!(other.store.event_cursor().unwrap(), 0);
    let invalid = json!({"tools":[{"name":"duplicate"},{"name":"duplicate"}],"type":"skill","data":{"$source":"literal"}});
    assert!(
        engine
            .finish_tool_discovery(&op, &Ok(invalid.clone()))
            .is_err()
    );
    assert_eq!(count(&engine, "integration_discovery"), 0);
    assert_eq!(count(&engine, "integration_observation"), 2);
    let conn = rusqlite::Connection::open(root.path().join("engineering.sqlite3")).unwrap();
    let observed:String=conn.query_row("SELECT json_extract(data,'$.value.result.value') FROM governance_records WHERE type='integration_observation' AND json_type(data,'$.value.result')='object'",[],|r|r.get(0)).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&observed).unwrap(), invalid);
}
