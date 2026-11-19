#[macro_use]
#[path = "../../../tests/support/delegating_store.rs"]
mod delegating_store;
#[path = "../../../tests/support/defaults.rs"]
mod default_fixture;
use default_fixture::*;
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
#[path = "../../../tests/support/schemas.rs"]
mod schemas;
use devdesk_adapters::*;
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
use std::sync::Arc;

fn engine(path: &std::path::Path) -> Engine {
    let e = Engine::new(
        Arc::new(SqliteStore::open_fixture(path).unwrap()),
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

#[test]
fn pending_startups_reserve_slots_and_cancel_without_sending_input() {
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    };
    #[derive(Default)]
    struct Deferred {
        ready: AtomicBool,
        starts: Mutex<Vec<ports::SessionStart>>,
        sent: Mutex<Vec<Id>>,
        stopped: Mutex<Vec<Id>>,
    }
    impl ports::Harness for Deferred {
        fn begin_session(&self, r: &ports::SessionStart) -> Result<Option<ports::SessionReceipt>> {
            self.starts.lock().unwrap().push(r.clone());
            Ok(None)
        }
        fn poll_session_start(
            &self,
            _: &Session,
            e: &RuntimeExecution,
        ) -> Result<Option<ports::SessionReceipt>> {
            if !self.ready.load(Ordering::Acquire) {
                return Ok(None);
            }
            Ok(Some(ports::SessionReceipt {
                external_session: format!("native-{}", e.session_id),
                container_id: format!("container-{}", e.id),
                image_digest: e.image_digest.clone(),
                operation_id: e.operation_id.clone(),
            }))
        }
        fn dispatch_turn(&self, r: &ports::TurnDispatch) -> Result<()> {
            self.sent.lock().unwrap().push(r.turn.id.clone());
            Ok(())
        }
        fn poll_session(&self, _: &Session) -> Result<Vec<ports::ConversationObservation>> {
            Ok(vec![])
        }
        fn stop_session(&self, _: &Session, e: &RuntimeExecution) -> Result<ports::WorkerStop> {
            self.stopped.lock().unwrap().push(e.operation_id.clone());
            Ok(ports::WorkerStop {
                operation_id: e.operation_id.clone(),
                writer_revoked: true,
                mode: "explicit deferred fixture".into(),
                requires_successor: true,
            })
        }
        fn cleanup_worker(&self, _: &Session, _: &RuntimeExecution) -> Result<()> {
            Ok(())
        }
    }
    let root = tempfile::tempdir().unwrap();
    let mut e = engine(root.path());
    let harness = Arc::new(Deferred::default());
    e.harness = harness.clone();
    let project = e
        .execute(
            "project.create",
            Command {
                command_id: id(),
                target: None,
                expected_revision: None,
                payload: json!({"title":"startup quota fixture"}),
            },
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for _ in 0..3 {
        runtime::queue_conversation(&e, &project);
    }
    for _ in 0..3 {
        e.tick_conversations(2).unwrap();
    }
    assert_eq!(harness.starts.lock().unwrap().len(), 2);
    assert!(harness.sent.lock().unwrap().is_empty());
    assert_eq!(e.conversation_running().unwrap(), 2);
    let before = e.store.snapshot().unwrap();
    // Admission is bounded and ordered by durable IDs, not fixture creation
    // order. Cancel an actually reserved startup and inspect the queued peer.
    let reserved = before
        .turns
        .iter()
        .filter(|t| t.status == TurnStatus::Dispatching)
        .collect::<Vec<_>>();
    assert_eq!(reserved.len(), 2);
    let cancelled = reserved[0].id.clone();
    let first_turn = reserved[1].id.clone();
    let third = before
        .turns
        .iter()
        .find(|t| t.status == TurnStatus::Queued)
        .unwrap()
        .id
        .clone();
    let turn = before.turns.iter().find(|t| t.id == cancelled).unwrap();
    e.execute(
        "conversation.cancel",
        Command {
            command_id: id(),
            target: Some(cancelled.clone()),
            expected_revision: Some(turn.revision),
            payload: json!({}),
        },
    )
    .unwrap();
    e.tick_conversations(2).unwrap();
    let after = e.store.snapshot().unwrap();
    assert_eq!(
        after
            .turns
            .iter()
            .find(|t| t.id == cancelled)
            .unwrap()
            .status,
        TurnStatus::Cancelled
    );
    assert!(
        !after
            .turns
            .iter()
            .find(|t| t.id == cancelled)
            .unwrap()
            .consumed
    );
    assert_eq!(
        after.turns.iter().find(|t| t.id == third).unwrap().status,
        TurnStatus::Dispatching
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while harness.starts.lock().unwrap().len() < 3 && std::time::Instant::now() < deadline {
        e.tick_conversations(2).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(harness.starts.lock().unwrap().len(), 3);
    harness.ready.store(true, Ordering::Release);
    for _ in 0..3 {
        e.tick_conversations(2).unwrap();
    }
    let sent = harness.sent.lock().unwrap();
    assert_eq!(sent.len(), 2);
    assert!(sent.contains(&first_turn) && sent.contains(&third));
    assert!(!sent.contains(&cancelled));
    drop(sent);
    let state = e.store.snapshot().unwrap();
    assert_eq!(state.executions.len(), 3);
    e.stop_owned_runtime().unwrap();
    assert_eq!(e.conversation_running().unwrap(), 0);
}
fn initialize(e: &Engine) -> (Id, Id, Id) {
    let pid = e
        .execute(
            "project.create",
            Command {
                command_id: id(),
                target: None,
                expected_revision: None,
                payload: json!({"title":"discussion domain fixture"}),
            },
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let sid = e
        .open_prepared_session(
            &runtime::ResourceFixture {
                store: e.store.as_ref(),
            },
            Command {
                command_id: id(),
                target: Some(pid.clone()),
                expected_revision: Some(1),
                payload: json!({"agent":"simplified-agent-analyst:4"}),
            },
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let turn = e.execute("conversation.send",Command {
        command_id:id(), target:Some(sid.clone()), expected_revision:Some(1),
        payload:json!({"client_message_id":"message-1","input":"Discuss a requirement before a Feature exists"}),
    }).unwrap()["id"].as_str().unwrap().to_owned();
    (pid, sid, turn)
}
#[test]
fn discussion_before_feature_survives_restart_without_fabricating_runs() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let (pid, sid, turn) = initialize(&e);
    let s = e.store.snapshot().unwrap();
    assert_eq!(s.sessions[0].project_id.as_ref(), Some(&pid));
    assert_eq!(s.sessions[0].id, sid);
    assert_eq!(s.turns[0].id, turn);
    drop(e);
    let e = engine(dir.path());
    let restored = e.store.snapshot().unwrap();
    assert_eq!(
        serde_json::to_value(&restored.sessions).unwrap(),
        serde_json::to_value(&s.sessions).unwrap()
    );
    assert_eq!(restored.turns.len(), 1);
    assert_eq!(restored.conversation_events.len(), 1);
    let roles: Vec<_> = restored
        .definitions
        .iter()
        .filter(|d| d.agent_profile.is_some())
        .collect();
    assert_eq!(
        roles.len(),
        s.definitions
            .iter()
            .filter(|d| d.agent_profile.is_some())
            .count()
    );
    assert_eq!(
        restored
            .governance
            .iter()
            .filter(|record| record.r#type == "role")
            .count(),
        s.governance
            .iter()
            .filter(|record| record.r#type == "role")
            .count()
    );
    assert_eq!(
        roles
            .iter()
            .filter(|definition| definition.id.starts_with("builtin-agent-sol-high-"))
            .count(),
        s.definitions
            .iter()
            .filter(|d| d.agent_profile.is_some() && d.id.starts_with("builtin-agent-sol-high-"))
            .count()
    );
    assert_ne!(
        roles
            .iter()
            .find(|d| d.agent_profile.as_ref().unwrap().role == "builtin-role-reviewer")
            .unwrap()
            .id,
        roles
            .iter()
            .find(|d| d.agent_profile.as_ref().unwrap().role == "builtin-role-developer")
            .unwrap()
            .id
    );
}
#[test]
fn enforce_busy_cross_project_immutable_binding_and_append_only_messages() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    initialize(&e);
    assert!(
        e.store
            .seed_fixture_state(None, &mut |s| {
                s.sessions[0].runtime_binding.as_mut().unwrap().model = "changed".into();
                Ok(json!(null))
            })
            .is_err()
    );
    assert!(
        e.store
            .seed_fixture_state(None, &mut |s| {
                let mut t = s.turns[0].clone();
                t.id = id();
                t.client_message_id = "another-message".into();
                s.turns.push(t);
                Ok(json!(null))
            })
            .is_err()
    );
    assert!(
        e.store
            .seed_fixture_state(None, &mut |s| {
                s.turns[0].project_id = id();
                Ok(json!(null))
            })
            .is_err()
    );
    assert!(
        e.store
            .seed_fixture_state(None, &mut |s| {
                s.conversation_events[0].content = "rewritten".into();
                Ok(json!(null))
            })
            .is_err()
    );
    assert!(
        e.store
            .seed_fixture_state(None, &mut |s| {
                s.turns[0].status = TurnStatus::Completed;
                Ok(json!(null))
            })
            .is_err()
    );
    assert_eq!(
        e.store.snapshot().unwrap().turns[0].status,
        TurnStatus::Queued
    );
    e.store
        .seed_fixture_state(None, &mut |s| {
            s.turns[0].status = TurnStatus::NeedsReconciliation;
            Ok(json!(null))
        })
        .unwrap();
    assert!(
        e.store
            .seed_fixture_state(None, &mut |s| {
                let mut t = s.turns[0].clone();
                t.id = id();
                t.client_message_id = "resubmit".into();
                s.turns.push(t);
                Ok(json!(null))
            })
            .is_err()
    );
}
#[test]
fn completion_needs_external_session_consumption_and_writer_release_proof() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let (_, sid, _) = initialize(&e);
    e.store
        .seed_fixture_state(None, &mut |s| {
            s.sessions[0].external_session = Some("provider-session-1".into());
            s.turns[0].status = TurnStatus::Completed;
            s.turns[0].external_message = Some("provider-input-1".into());
            s.turns[0].external_turn = Some("provider-turn-1".into());
            s.turns[0].consumed = true;
            s.turns[0].started = Some(1);
            s.turns[0].finished = Some(2);
            s.executions.push(RuntimeExecution {
                permissions: None,
                kind: RuntimeExecutionKind::Harness,
                protocol_artifact: None,
                id: id(),
                session_id: sid.clone(),

                operation_id: id(),
                container_id: Some("fixture-container".into()),
                image_digest: "a".repeat(64),
                status: Status::Running,
                writer_revoked: false,
                started: Some(1),
                finished: None,
            });
            Ok(json!(null))
        })
        .unwrap();
    assert!(
        e.store
            .seed_fixture_state(None, &mut |s| {
                s.sessions[0].external_session = Some("different-provider-session".into());
                Ok(json!(null))
            })
            .is_err()
    );
    assert!(
        e.store
            .seed_fixture_state(None, &mut |s| {
                s.turns[0].status = TurnStatus::Queued;
                Ok(json!(null))
            })
            .is_err()
    );
    assert!(
        e.store
            .seed_fixture_state(None, &mut |s| {
                s.executions[0].status = Status::Released;
                Ok(json!(null))
            })
            .is_err()
    );
    e.store
        .seed_fixture_state(None, &mut |s| {
            s.executions[0].status = Status::Released;
            s.executions[0].writer_revoked = true;
            s.executions[0].finished = Some(3);
            Ok(json!(null))
        })
        .unwrap();
}

#[test]
fn project_requirement_and_snapshot_have_real_owner_then_feature_pins_the_revision() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let call = |method: &str, target: Option<&str>, revision: Option<u64>, payload| {
        e.execute(
            method,
            Command {
                command_id: id(),
                target: target.map(str::to_owned),
                expected_revision: revision,
                payload,
            },
        )
    };
    let project = call(
        "project.create",
        None,
        None,
        json!({"title":"Requirement before Feature"}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let requirement=call("document.revise",Some(&project),Some(1),json!({"kind":"requirement","title":"Export","body":"Export rows","acceptance":[{"key":"EMPTY","description":"Empty input keeps the header"}]})).unwrap()["id"].as_str().unwrap().to_string();
    assert!(e.store.snapshot().unwrap().features.is_empty());
    let snapshot = call(
        "snapshot.save",
        Some(&project),
        Some(2),
        json!({"documents":[requirement]}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    runtime::configure_project(&e, &project);
    let feature = call(
        "feature.create",
        None,
        None,
        json!({"project_id":project,"title":"CSV export","source_materials":[requirement]}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let state = e.store.snapshot().unwrap();
    let document = state
        .documents
        .iter()
        .find(|d| d.id == requirement)
        .unwrap();
    assert!(document.feature_id.is_none());
    assert_eq!(document.project_id.as_ref(), Some(&project));
    assert_eq!(
        state.feature(&feature).unwrap().source_materials.first(),
        Some(&requirement)
    );
    assert!(state.document_in_feature(document, &feature));
    let saved = state.snapshots.iter().find(|s| s.id == snapshot).unwrap();
    assert!(saved.feature_id.is_none());
    assert_eq!(saved.project_id.as_ref(), Some(&project));
    assert!(
        state
            .artifacts
            .iter()
            .all(|a| a.feature_id.is_none() && a.project_id.as_ref() == Some(&project))
    );
    let newer=call("document.revise",Some(&project),Some(state.projects.iter().find(|p| p.id == project).unwrap().revision),json!({"kind":"requirement","title":"Export v2","body":"Export with errors","supersedes":requirement})).unwrap()["id"].as_str().unwrap().to_string();
    let state = e.store.snapshot().unwrap();
    assert!(!state.document_in_feature(
        state.documents.iter().find(|d| d.id == newer).unwrap(),
        &feature
    ));
    let other_directory = tempfile::tempdir().unwrap();
    let other_engine = engine(other_directory.path());
    let other = other_engine
        .execute(
            "project.create",
            Command {
                command_id: id(),
                target: None,
                expected_revision: None,
                payload: json!({"title":"Other project"}),
            },
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        call(
            "feature.create",
            None,
            None,
            json!({"project_id":other,"title":"invalid","source_materials":[requirement]})
        )
        .is_err()
    );
    assert!(
        call(
            "snapshot.save",
            Some(&other),
            Some(1),
            json!({"documents":[requirement]})
        )
        .is_err()
    );
    drop(e);
    let reopened = engine(dir.path());
    let s = reopened.store.snapshot().unwrap();
    assert_eq!(
        s.feature(&feature).unwrap().source_materials.first(),
        Some(&requirement)
    );
    assert_eq!(
        s.documents
            .iter()
            .filter(|d| d.project_id.as_ref() == Some(&project))
            .count(),
        2
    );
}

#[test]
fn conversation_commands_are_idempotent_and_switching_creates_a_snapshot_successor() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let (pid, sid, first_turn) = initialize(&e);
    let call = |method: &str, target: &str, revision: u64, payload| {
        e.execute(
            method,
            Command {
                command_id: id(),
                target: Some(target.into()),
                expected_revision: Some(revision),
                payload,
            },
        )
    };
    call("conversation.cancel", &first_turn, 1, json!({})).unwrap();
    let first = e.store.snapshot().unwrap().sessions[0].clone();
    let binding = first.runtime_binding.clone().unwrap();
    let original = Command {
        command_id: id(),
        target: Some(pid.clone()),
        expected_revision: Some(1),
        payload: json!({"agent":first.agent}),
    };
    let reused = e
        .open_prepared_session(
            &runtime::ResourceFixture {
                store: e.store.as_ref(),
            },
            original.clone(),
        )
        .unwrap();
    assert_eq!(reused["id"], sid);
    assert_eq!(reused["reused"], true);
    assert_eq!(
        e.open_prepared_session(
            &runtime::ResourceFixture {
                store: e.store.as_ref()
            },
            original.clone()
        )
        .unwrap(),
        reused
    );
    let mut forged = original;
    forged.command_id = id();
    forged.payload["binding"] = json!(binding);
    assert!(
        e.open_prepared_session(
            &runtime::ResourceFixture {
                store: e.store.as_ref()
            },
            forged
        )
        .is_err()
    );
    let payload = json!({"input":"Discuss acceptance criteria","client_message_id":"frontend-message","experience_refs":[{"source_project_id":pid,"snapshot_id":"historical-snapshot"}]});
    let submitted = call("conversation.send", &sid, first.revision, payload.clone()).unwrap();
    let replay = call("conversation.send", &sid, first.revision, payload.clone()).unwrap();
    assert_eq!(submitted["id"], replay["id"]);
    let state = e.store.snapshot().unwrap();
    let context = state
        .governance
        .iter()
        .find(|r| r.r#type == "conversation_context" && r.definition_id == submitted["id"])
        .unwrap();
    assert_eq!(context.data["experience_refs"], payload["experience_refs"]);
    let mut changed_references = payload;
    changed_references["experience_refs"] = json!([]);
    assert!(
        call(
            "conversation.send",
            &sid,
            first.revision,
            changed_references
        )
        .is_err()
    );
    assert!(
        call(
            "conversation.send",
            &sid,
            2,
            json!({"input":"changed","client_message_id":"frontend-message"})
        )
        .is_err()
    );
    call(
        "conversation.cancel",
        submitted["id"].as_str().unwrap(),
        1,
        json!({}),
    )
    .unwrap();
    let next = e
        .open_prepared_session(
            &runtime::ResourceFixture {
                store: e.store.as_ref(),
            },
            Command {
                command_id: id(),
                target: Some(pid.clone()),
                expected_revision: Some(1),
                payload: json!({"agent":first.agent,"force_new":true}),
            },
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let state = e.store.snapshot().unwrap();
    let successor = state.sessions.iter().find(|s| s.id == next).unwrap();
    assert_eq!(successor.predecessor.as_ref(), Some(&sid));
    assert!(successor.snapshot_id.is_some());
    let source = state
        .snapshots
        .iter()
        .find(|s| Some(&s.id) == successor.snapshot_id.as_ref())
        .unwrap();
    assert!(source.content.contains("Discuss acceptance criteria"));
    assert_eq!(source.capture.sessions, vec![sid.clone()]);
    assert_eq!(source.runtime_bindings, vec![binding.clone()]);
    let back = e
        .open_prepared_session(
            &runtime::ResourceFixture {
                store: e.store.as_ref(),
            },
            Command {
                command_id: id(),
                target: Some(pid.clone()),
                expected_revision: Some(1),
                payload: json!({"agent":first.agent,"force_new":true}),
            },
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(back, sid);
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .sessions
            .iter()
            .find(|s| s.id == back)
            .unwrap()
            .predecessor
            .as_ref(),
        Some(&next)
    );
    let artifacts: Vec<_> = e
        .store
        .snapshot()
        .unwrap()
        .artifacts
        .into_iter()
        .filter(|artifact| artifact.relative_path.contains("/snapshots/"))
        .collect();
    assert!(!artifacts.is_empty());
    for artifact in artifacts {
        let snapshot = serde_json::from_slice(&e.artifacts.read(&artifact).unwrap()).unwrap();
        schemas::SCHEMAS.assert_valid("manifest.schema.json", &snapshot);
        assert_eq!(artifact.origin, Origin::External);
    }
}

fn complete_fixture(e: &Engine, sid: &str, tid: &str) {
    e.store
        .seed_fixture_state(None, &mut |s| {
            s.sessions
                .iter_mut()
                .find(|v| v.id == sid)
                .unwrap()
                .external_session = Some(format!("native-{sid}"));
            let t = s.turns.iter_mut().find(|t| t.id == tid).unwrap();
            t.status = TurnStatus::Completed;
            t.consumed = true;
            t.finished = Some(2);
            Ok(json!({"ok":true}))
        })
        .unwrap();
}

#[test]
fn generic_proposal_adopts_typed_material_with_sources_without_creating_business_objects() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let (project, session, turn) = initialize(&e);
    complete_fixture(&e, &session, &turn);
    let material = e
        .execute(
            "document.revise",
            Command {
                command_id: id(),
                target: Some(project.clone()),
                expected_revision: Some(1),
                payload: json!({"kind":"source","title":"原始材料","body":"已记录输入"}),
            },
        )
        .unwrap();
    let draft=e.execute("proposal.create",Command{command_id:id(),target:None,expected_revision:None,payload:json!({"turn_id":turn,"content":{"delivery_ref":"fixture-delivery-object:1","title":"任意业务结果","value":{"risks":["scope"]},"input_refs":[material["id"]]}})}).unwrap();
    let adopted = e
        .execute(
            "proposal.adopt",
            Command {
                command_id: id(),
                target: Some(draft["id"].as_str().unwrap().into()),
                expected_revision: Some(1),
                payload: json!({}),
            },
        )
        .unwrap();
    let state = e.store.snapshot().unwrap();
    let document = state
        .documents
        .iter()
        .find(|d| Some(d.id.as_str()) == adopted["objects"][0].as_str())
        .unwrap();
    assert_eq!(
        document.input_refs,
        vec![material["id"].as_str().unwrap().to_string()]
    );
    assert_eq!(document.kind, "fixture-delivery-object:1");
    assert!(document.body.contains("risks"));
    assert!(state.features.is_empty());
    assert!(state.tasks.is_empty());
    assert!(
        e.execute(
            "proposal.adopt",
            Command {
                command_id: id(),
                target: Some(draft["id"].as_str().unwrap().into()),
                expected_revision: Some(1),
                payload: json!({})
            }
        )
        .is_err()
    );
}
struct ConversationFixture {
    store: Arc<SqliteStore>,
    dispatched: std::sync::atomic::AtomicUsize,
    stop_allowed: std::sync::atomic::AtomicBool,
    emit_terminal: std::sync::atomic::AtomicBool,
    log_allowed: std::sync::atomic::AtomicBool,
    cleaned: std::sync::atomic::AtomicUsize,
}
impl ports::Harness for ConversationFixture {
    fn open_session(&self, r: &ports::SessionStart) -> Result<ports::SessionReceipt> {
        assert!(
            self.store
                .snapshot()?
                .executions
                .iter()
                .any(|e| e.operation_id == r.execution.operation_id)
        );
        Ok(ports::SessionReceipt {
            external_session: format!("fixture-native-{}", r.session.id),
            container_id: "fixture-container".into(),
            image_digest: r.execution.image_digest.clone(),
            operation_id: r.execution.operation_id.clone(),
        })
    }
    fn dispatch_turn(&self, r: &ports::TurnDispatch) -> Result<()> {
        assert_eq!(
            self.store
                .snapshot()?
                .turns
                .iter()
                .find(|t| t.id == r.turn.id)
                .unwrap()
                .status,
            TurnStatus::Dispatching
        );
        self.dispatched
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    fn poll_session(&self, s: &Session) -> Result<Vec<ports::ConversationObservation>> {
        if !self.emit_terminal.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(vec![]);
        }
        let tid = self
            .store
            .snapshot()?
            .turns
            .iter()
            .find(|t| t.session_id == s.id && t.status.active())
            .unwrap()
            .id
            .clone();
        Ok(vec![
            ports::ConversationObservation::Admitted {
                turn: tid.clone(),
                message: None,
                external_turn: Some("fixture-turn".into()),
            },
            ports::ConversationObservation::Consumed {
                turn: tid.clone(),
                message: None,
            },
            ports::ConversationObservation::Terminal {
                turn: tid,
                outcome: ports::ProviderTerminal::Completed,
                reason: "fixture terminal".into(),
                quiescent: true,
            },
        ])
    }
    fn stop_session(&self, _: &Session, e: &RuntimeExecution) -> Result<ports::WorkerStop> {
        require(
            self.stop_allowed.load(std::sync::atomic::Ordering::SeqCst),
            "fixture stop unavailable",
        )?;
        Ok(ports::WorkerStop {
            operation_id: e.operation_id.clone(),
            writer_revoked: true,
            mode: "fixture".into(),
            requires_successor: true,
        })
    }
    fn inspect_worker_stop(
        &self,
        session: &Session,
        execution: &RuntimeExecution,
    ) -> Result<Option<ports::WorkerStop>> {
        if self.stop_allowed.load(std::sync::atomic::Ordering::SeqCst) {
            Ok(Some(ports::WorkerStop {
                operation_id: execution.operation_id.clone(),
                writer_revoked: true,
                mode: format!("fixture-readonly:{}", session.id),
                requires_successor: true,
            }))
        } else {
            Ok(None)
        }
    }
    fn session_log(&self, _: &RuntimeExecution) -> Result<Option<String>> {
        require(
            self.log_allowed.load(std::sync::atomic::Ordering::SeqCst),
            "fixture log temporarily unavailable",
        )?;
        Ok(Some("sanitized fixture frames".into()))
    }
    fn cleanup_worker(&self, _: &Session, e: &RuntimeExecution) -> Result<()> {
        let s = self.store.snapshot()?;
        let row = s.executions.iter().find(|v| v.id == e.id).unwrap();
        self.cleaned
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert!(row.writer_revoked && row.protocol_artifact.is_some());
        assert!(
            s.artifacts
                .iter()
                .any(|a| Some(&a.id) == row.protocol_artifact.as_ref())
        );
        Ok(())
    }
}
#[test]
fn coordinator_never_resends_unknown_input_or_finishes_without_stop_and_export() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open_fixture(dir.path()).unwrap());
    let fixture = Arc::new(ConversationFixture {
        store: store.clone(),
        dispatched: 0.into(),
        stop_allowed: false.into(),
        emit_terminal: true.into(),
        log_allowed: true.into(),
        cleaned: 0.into(),
    });
    let mut adapters = runtime::adapters();
    adapters.harness = fixture.clone();
    let e = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(dir.path().into())),
        adapters,
    );
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    assert!(!store.has_pending_work().unwrap());
    let (_, sid, tid) = initialize(&e);
    assert!(
        store.has_pending_work().unwrap(),
        "queued input must keep background scheduling eligible"
    );
    store
        .seed_fixture_state(None, &mut |s| {
            s.sessions
                .iter_mut()
                .find(|v| v.id == sid)
                .unwrap()
                .runtime_binding
                .as_mut()
                .unwrap()
                .installation
                .artifact = RuntimeArtifact::DockerImage {
                image_id: format!("sha256:{}", "c".repeat(64)),
            };
            Ok(json!({"ok":true}))
        })
        .unwrap_err();
    // Bind a valid new Session through the public resolved command rather than changing its frozen metadata.
    let state = store.snapshot().unwrap();
    let pid = state.projects[0].id.clone();
    e.execute(
        "conversation.cancel",
        Command {
            command_id: id(),
            target: Some(tid),
            expected_revision: Some(1),
            payload: json!({}),
        },
    )
    .unwrap();
    let opened = e
        .open_prepared_session(
            &runtime::ResourceFixture {
                store: e.store.as_ref(),
            },
            Command {
                command_id: id(),
                target: Some(pid),
                expected_revision: Some(1),
                payload: json!({"agent":"simplified-agent-analyst:4","force_new":true}),
            },
        )
        .unwrap();
    let sid = opened["id"].as_str().unwrap();
    let sent = e
        .execute(
            "conversation.send",
            Command {
                command_id: id(),
                target: Some(sid.into()),
                expected_revision: Some(1),
                payload: json!({"client_message_id":"single-input","input":"fixture input"}),
            },
        )
        .unwrap();
    let tid = sent["id"].as_str().unwrap();
    e.tick_conversations(2).unwrap();
    e.tick_conversations(2).unwrap();
    assert_eq!(
        fixture.dispatched.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    let state = store.snapshot().unwrap();
    assert_eq!(
        state.turns.iter().find(|t| t.id == tid).unwrap().status,
        TurnStatus::NeedsReconciliation
    );
    assert!(!state.executions[0].writer_revoked);
    assert!(
        store.has_pending_work().unwrap(),
        "unknown writer must remain recoverable even with no running Run"
    );
    fixture
        .stop_allowed
        .store(true, std::sync::atomic::Ordering::SeqCst);
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    e.tick_conversations(2).unwrap();
    assert_eq!(
        fixture.dispatched.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    let state = store.snapshot().unwrap();
    let turn = state.turns.iter().find(|t| t.id == tid).unwrap();
    e.execute(
        "conversation.cancel",
        Command {
            command_id: id(),
            target: Some(tid.into()),
            expected_revision: Some(turn.revision),
            payload: json!({}),
        },
    )
    .unwrap();
    e.tick_conversations(2).unwrap();
    let state = store.snapshot().unwrap();
    assert_eq!(
        state.turns.iter().find(|t| t.id == tid).unwrap().status,
        TurnStatus::Cancelled
    );
    assert!(state.executions[0].writer_revoked);
    assert!(state.executions[0].protocol_artifact.is_some());
    assert!(
        !store.has_pending_work().unwrap(),
        "stopped and sealed idle histories do not need continuous full-state loading"
    );
}

#[test]
fn failed_log_export_retains_the_stopped_worker_and_recovers_without_resending_input() {
    use std::sync::atomic::Ordering::SeqCst;
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open_fixture(dir.path()).unwrap());
    let fixture = Arc::new(ConversationFixture {
        store: store.clone(),
        dispatched: 0.into(),
        stop_allowed: true.into(),
        emit_terminal: true.into(),
        log_allowed: false.into(),
        cleaned: 0.into(),
    });
    let mut adapters = runtime::adapters();
    adapters.harness = fixture.clone();
    let e = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(dir.path().into())),
        adapters,
    );
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    let (_, _, tid) = initialize(&e);
    for _ in 0..4 {
        let _ = e.tick_conversations(2);
    }
    assert_eq!(fixture.dispatched.load(SeqCst), 1);
    assert_eq!(fixture.cleaned.load(SeqCst), 0);
    let before = store.snapshot().unwrap();
    let worker = before
        .executions
        .iter()
        .find(|w| w.kind == RuntimeExecutionKind::Harness)
        .unwrap();
    assert!(worker.writer_revoked && worker.protocol_artifact.is_none());
    assert!(store.has_pending_work().unwrap());
    fixture.log_allowed.store(true, SeqCst);
    for _ in 0..8 {
        e.tick_conversations(2).unwrap();
    }
    let after = store.snapshot().unwrap();
    assert!(
        after
            .executions
            .iter()
            .find(|w| w.id == worker.id)
            .unwrap()
            .protocol_artifact
            .is_some()
    );
    assert_eq!(fixture.dispatched.load(SeqCst), 1);
    assert_eq!(fixture.cleaned.load(SeqCst), 1);
    assert!(
        after
            .turns
            .iter()
            .any(|t| t.id == tid && t.status == TurnStatus::Completed)
    );
    assert!(
        after
            .governance
            .iter()
            .any(|r| r.r#type == "worker_stop_observation"
                && r.data["value"]["log_error"].is_object())
    );
}

#[test]
fn global_agent_discussion_has_no_project_and_persists_identity_messages_and_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    struct CompletionHarness(std::sync::Mutex<Option<Id>>);
    impl ports::Harness for CompletionHarness {
        fn open_session(&self, r: &ports::SessionStart) -> Result<ports::SessionReceipt> {
            Ok(ports::SessionReceipt {
                external_session: format!("global-fixture-{}", r.session.id),
                container_id: "offline".into(),
                image_digest: r.execution.image_digest.clone(),
                operation_id: r.execution.operation_id.clone(),
            })
        }
        fn dispatch_turn(&self, r: &ports::TurnDispatch) -> Result<()> {
            *self.0.lock().unwrap() = Some(r.turn.id.clone());
            Ok(())
        }
        fn poll_session(&self, _: &Session) -> Result<Vec<ports::ConversationObservation>> {
            let Some(turn) = self.0.lock().unwrap().take() else {
                return Ok(vec![]);
            };
            Ok(vec![
                ports::ConversationObservation::Admitted {
                    turn: turn.clone(),
                    message: None,
                    external_turn: Some("offline-turn".into()),
                },
                ports::ConversationObservation::Consumed {
                    turn: turn.clone(),
                    message: None,
                },
                ports::ConversationObservation::Message {
                    turn: turn.clone(),
                    key: "answer".into(),
                    content: "Completed cross-project work".into(),
                    kind: ConversationEventKind::Assistant,
                    mode: MessageMode::Full,
                    item: "answer".into(),
                },
                ports::ConversationObservation::Terminal {
                    turn,
                    outcome: ports::ProviderTerminal::Completed,
                    reason: "completed".into(),
                    quiescent: true,
                },
            ])
        }
        fn stop_session(&self, _: &Session, e: &RuntimeExecution) -> Result<ports::WorkerStop> {
            Ok(ports::WorkerStop {
                operation_id: e.operation_id.clone(),
                writer_revoked: true,
                requires_successor: false,
                mode: "offline-fixture".into(),
            })
        }
    }
    let mut adapters = runtime::adapters();
    adapters.harness = Arc::new(CompletionHarness(std::sync::Mutex::new(None)));
    let e = Engine::new(
        Arc::new(SqliteStore::open(dir.path()).unwrap()),
        Arc::new(FileArtifacts::new(dir.path().into())),
        adapters,
    );
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    let agent = default_agent_id("analyst");
    let instance = e.execute("agent.instance.create",Command{command_id:id(),target:None,expected_revision:None,payload:json!({"name":"Global analyst","scope":"global","agent_revision_id":agent})}).unwrap()["id"].as_str().unwrap().to_owned();
    let opened = e
        .open_prepared_session(
            &runtime::ResourceFixture {
                store: e.store.as_ref(),
            },
            Command {
                command_id: id(),
                target: Some("global".into()),
                expected_revision: None,
                payload: json!({"agent":agent,"agent_instance_id":instance}),
            },
        )
        .unwrap();
    let sid = opened["id"].as_str().unwrap();
    let c = Command {
        command_id: id(),
        target: Some(sid.into()),
        expected_revision: Some(1),
        payload: json!({"client_message_id":id(),"input":"Remember this cross-project planning question"}),
    };
    let sent = e.execute("conversation.send", c.clone()).unwrap();
    assert_eq!(e.execute("conversation.send", c).unwrap(), sent);
    e.tick_conversations(0).unwrap();
    let state = e.store.snapshot().unwrap();
    assert!(state.projects.is_empty());
    assert_eq!(state.sessions[0].scope, Some(SessionScope::Global));
    assert!(!state.snapshots.is_empty());
    assert!(state.governance.iter().any(|r| r.r#type == "experience"
        && r.scope == Scope::global()
        && r.data["agent_instance_id"] == instance));
    e.tick_conversations(1).unwrap();
    e.tick_conversations(1).unwrap();
    e.tick_conversations(0).unwrap();
    let completed = e.store.snapshot().unwrap();
    assert!(
        completed
            .snapshots
            .iter()
            .any(|snap| snap.kind == "conversation_completed"
                && snap.content.contains("Completed cross-project work"))
    );
    drop(e);
    let e = Engine::new(
        Arc::new(SqliteStore::open(dir.path()).unwrap()),
        Arc::new(FileArtifacts::new(dir.path().into())),
        runtime::adapters(),
    );
    e.bootstrap().unwrap();
    let detail = e.query("query.session", &json!({"id":sid}), 1).unwrap();
    assert!(
        detail["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["content"] == "Remember this cross-project planning question")
    );
    assert_eq!(detail["agent_instance_id"], instance);
    let owned = e
        .query(
            "query.sessions",
            &json!({"owner_id":"global","agent_instance_id":instance}),
            1,
        )
        .unwrap();
    assert_eq!(owned["total"], 1);
    assert_eq!(owned["sessions"][0]["agent_instance_id"], instance);
    assert_eq!(
        e.query(
            "query.sessions",
            &json!({"owner_id":"global","agent_instance_id":id()}),
            1
        )
        .unwrap()["total"],
        0
    );
    assert_eq!(detail["session"]["scope"], "global");
    assert!(e.store.snapshot().unwrap().projects.is_empty());
}

#[test]
fn turn_diagnostics_include_final_messages_and_stop_proof_without_stream_fragments() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let (pid, sid, tid) = initialize(&e);
    e.store.seed_fixture_state(None,&mut |s| {
        for (sequence,mode,content,kind) in [(2,MessageMode::Delta,"part",ConversationEventKind::Assistant),(3,MessageMode::Delta,"more",ConversationEventKind::Assistant),
            (4,MessageMode::Full,"final answer",ConversationEventKind::Assistant),
            (5,MessageMode::Full,r#"{"operation_id":"worker","writer_revoked":true,"mode":"normal_stop","requires_successor":false}"#,ConversationEventKind::Status)] {
            s.conversation_events.push(ConversationEvent {id:id(),session_id:sid.clone(),turn_id:tid.clone(),sequence,deduplication_key:format!("message-{sequence}"),kind,content:content.into(),artifact_id:None,time:sequence,mode,provider_item:Some("message".into())});
        } Ok(json!(null))
    }).unwrap();
    let detail = e
        .query(
            "query.session",
            &json!({"id":sid,"project_id":pid,"diagnostic_turn_id":tid}),
            1,
        )
        .unwrap();
    assert_eq!(detail["records"].as_array().unwrap().len(), 3);
    assert_eq!(detail["stop_receipts"][0]["writer_revoked"], true);
    assert!(!detail.to_string().contains("\"mode\":\"delta\""));
    assert!(
        e.query(
            "query.session",
            &json!({"id":sid,"diagnostic_turn_id":id()}),
            1
        )
        .is_err()
    );
    assert!(
        e.query(
            "query.session",
            &json!({"id":sid,"project_id":id(),"diagnostic_turn_id":tid}),
            1
        )
        .is_err()
    );
}

#[test]
fn local_busy_journaling_and_projection_do_not_interrupt_or_resend_a_model_turn() {
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    struct BusyStore {
        inner: Arc<dyn EngineeringStore>,
        journals: AtomicUsize,
        projection: AtomicBool,
    }
    impl EngineeringStore for BusyStore {
        fn root(&self) -> String {
            self.inner.root()
        }
        fn snapshot(&self) -> Result<EngineeringState> {
            self.inner.snapshot()
        }
        delegated_native_store!();
        fn submit_changes(
            &self,
            changes: &concurrency::PreparedChangeSet,
        ) -> Result<concurrency::CommitOutcome> {
            if changes.command_id.starts_with("provider-poll-observe:")
                && self
                    .journals
                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                    .is_ok()
            {
                return Err(err("storage_busy", "injected local journal contention"));
            }
            if changes
                .changes
                .iter()
                .any(|c| c.value["type"] == "provider_observation_batch")
                && self.projection.swap(false, Ordering::SeqCst)
            {
                return Err(err("storage_busy", "injected local projection contention"));
            }
            self.inner.submit_changes(changes)
        }
    }
    #[derive(Default)]
    struct OnceHarness {
        turn: Mutex<Option<Id>>,
        sends: AtomicUsize,
        batches: AtomicUsize,
    }
    impl ports::Harness for OnceHarness {
        fn open_session(&self, r: &ports::SessionStart) -> Result<ports::SessionReceipt> {
            Ok(ports::SessionReceipt {
                external_session: "original-session".into(),
                container_id: "fixture".into(),
                image_digest: r.execution.image_digest.clone(),
                operation_id: r.execution.operation_id.clone(),
            })
        }
        fn dispatch_turn(&self, r: &ports::TurnDispatch) -> Result<()> {
            self.sends.fetch_add(1, Ordering::SeqCst);
            *self.turn.lock().unwrap() = Some(r.turn.id.clone());
            Ok(())
        }
        fn poll_session(&self, _: &Session) -> Result<Vec<ports::ConversationObservation>> {
            let Some(turn) = self.turn.lock().unwrap().take() else {
                return Ok(vec![]);
            };
            self.batches.fetch_add(1, Ordering::SeqCst);
            Ok(vec![
                ports::ConversationObservation::Admitted {
                    turn: turn.clone(),
                    message: None,
                    external_turn: Some("original-turn".into()),
                },
                ports::ConversationObservation::Consumed {
                    turn: turn.clone(),
                    message: None,
                },
                ports::ConversationObservation::Message {
                    turn: turn.clone(),
                    key: "original-answer".into(),
                    content: "Only the original answer".into(),
                    kind: ConversationEventKind::Assistant,
                    mode: MessageMode::Full,
                    item: "answer".into(),
                },
                ports::ConversationObservation::Terminal {
                    turn,
                    outcome: ports::ProviderTerminal::Completed,
                    reason: "completed".into(),
                    quiescent: true,
                },
            ])
        }
        fn stop_session(&self, _: &Session, e: &RuntimeExecution) -> Result<ports::WorkerStop> {
            Ok(ports::WorkerStop {
                operation_id: e.operation_id.clone(),
                writer_revoked: true,
                mode: "fixture completed".into(),
                requires_successor: false,
            })
        }
        fn cleanup_worker(&self, _: &Session, _: &RuntimeExecution) -> Result<()> {
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let mut e = engine(dir.path());
    let project = e
        .execute(
            "project.create",
            Command {
                command_id: id(),
                target: None,
                expected_revision: None,
                payload: json!({"title":"Local contention"}),
            },
        )
        .unwrap();
    let (_, turn) = runtime::queue_conversation(&e, project["id"].as_str().unwrap());
    let harness = Arc::new(OnceHarness::default());
    e.harness = harness.clone();
    let storage = Arc::new(BusyStore {
        inner: e.store.clone(),
        journals: AtomicUsize::new(3),
        projection: AtomicBool::new(true),
    });
    e.store = storage.clone();
    for _ in 0..30 {
        e.tick_conversations(4).unwrap();
    }
    let state = e.store.snapshot().unwrap();
    let actual = state.turns.iter().find(|t| t.id == turn).unwrap();
    assert_eq!(actual.status, TurnStatus::Completed);
    assert!(!actual.cancel_requested);
    assert_eq!(harness.sends.load(Ordering::SeqCst), 1);
    assert_eq!(harness.batches.load(Ordering::SeqCst), 1);
    assert_eq!(storage.journals.load(Ordering::SeqCst), 0);
    assert!(!storage.projection.load(Ordering::SeqCst));
    assert_eq!(
        state
            .conversation_events
            .iter()
            .filter(|e| e.turn_id == turn && e.kind == ConversationEventKind::Assistant)
            .count(),
        1
    );
}
