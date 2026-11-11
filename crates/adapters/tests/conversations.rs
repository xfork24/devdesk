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
use serde_json::json;
use std::sync::Arc;

fn engine(path: &std::path::Path) -> Engine {
    let e = Engine::new(
        Arc::new(SqliteStore::open(path).unwrap()),
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
    let sid = id();
    let turn = id();
    e.store
        .transact(None, &mut |s| {
            s.sessions.push(Session {
                id: sid.clone(),
                owner_id: pid.clone(),
                agent: "builtin-agent-sol-high-analyst:3".into(),
                harness: "fixture-runtime".into(),
                sandbox: "docker-fixture".into(),
                snapshot_id: None,
                instance: None,
                origin: Origin::External,
                project_id: Some(pid.clone()),
                feature_id: None,
                task_id: None,
                scope: Some(SessionScope::Project),
                lifecycle: Some(SessionLifecycle::Prepared),
                runtime_binding: Some(RuntimeBinding {
                    model_connection: None,
                    model_profile_digest: None,
                    agent_configuration: None,
                    model_parameters: Default::default(),
                    permissions: None,
                    installation: RuntimeInstallation {
                        id: "fixture-runtime".into(),
                        key: RuntimeKey {
                            provider: HarnessProvider::Codex,
                            version: "fixture".into(),
                            os: "linux".into(),
                            arch: "aarch64".into(),
                        },
                        artifact: RuntimeArtifact::OciImage {
                            reference: format!("example.org/runtime@sha256:{}", "a".repeat(64)),
                        },
                        source: "synthetic domain fixture".into(),
                        license: "test".into(),
                    },
                    adapter_version: "fixture/v1".into(),
                    model: "fixture-model".into(),
                    tool_policy: ToolPolicy::ReadOnly,
                    writable_paths: vec![],
                    read_only_paths: vec![],
                    work_type: WorkType::Discussion,
                    configuration_digest: "a".repeat(64),
                    credential_ref: None,
                }),
                external_session: None,
                predecessor: None,
                revision: 1,
            });
            s.turns.push(ConversationTurn {
                id: turn.clone(),
                project_id: pid.clone(),
                session_id: sid.clone(),
                client_message_id: "message-1".into(),
                input: "Discuss a requirement before a Feature exists".into(),
                input_digest: "b".repeat(64),
                input_snapshot: None,
                code_view: None,
                source_revision: None,
                status: TurnStatus::Queued,
                external_message: None,
                external_turn: None,
                consumed: false,
                cancel_requested: false,
                started: None,
                finished: None,

                revision: 1,
            });
            s.conversation_events.push(ConversationEvent {
                mode: MessageMode::Full,
                provider_item: None,
                id: id(),
                session_id: sid.clone(),
                turn_id: turn.clone(),
                sequence: 1,
                deduplication_key: "input-1".into(),
                kind: ConversationEventKind::User,
                content: "requirement input".into(),
                artifact_id: None,
                time: 1,
            });
            Ok(json!({"ok":true}))
        })
        .unwrap();
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
            .filter(|record| record.kind == "role")
            .count(),
        6
    );
    assert_eq!(
        roles
            .iter()
            .filter(|definition| definition.id.starts_with("builtin-agent-sol-high-"))
            .count(),
        6
    );
    assert_ne!(
        roles
            .iter()
            .find(
                |d| d.agent_profile.as_ref().unwrap().role == String::from("builtin-role-reviewer")
            )
            .unwrap()
            .id,
        roles
            .iter()
            .find(|d| d.agent_profile.as_ref().unwrap().role
                == String::from("builtin-role-developer"))
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
            .transact(None, &mut |s| {
                s.sessions[0].runtime_binding.as_mut().unwrap().model = "changed".into();
                Ok(json!(null))
            })
            .is_err()
    );
    assert!(
        e.store
            .transact(None, &mut |s| {
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
            .transact(None, &mut |s| {
                s.turns[0].project_id = id();
                Ok(json!(null))
            })
            .is_err()
    );
    assert!(
        e.store
            .transact(None, &mut |s| {
                s.conversation_events[0].content = "rewritten".into();
                Ok(json!(null))
            })
            .is_err()
    );
    assert!(
        e.store
            .transact(None, &mut |s| {
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
        .transact(None, &mut |s| {
            s.turns[0].status = TurnStatus::NeedsReconciliation;
            Ok(json!(null))
        })
        .unwrap();
    assert!(
        e.store
            .transact(None, &mut |s| {
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
        .transact(None, &mut |s| {
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
            .transact(None, &mut |s| {
                s.sessions[0].external_session = Some("different-provider-session".into());
                Ok(json!(null))
            })
            .is_err()
    );
    assert!(
        e.store
            .transact(None, &mut |s| {
                s.turns[0].status = TurnStatus::Queued;
                Ok(json!(null))
            })
            .is_err()
    );
    assert!(
        e.store
            .transact(None, &mut |s| {
                s.executions[0].status = Status::Released;
                Ok(json!(null))
            })
            .is_err()
    );
    e.store
        .transact(None, &mut |s| {
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
    let other = call(
        "project.create",
        None,
        None,
        json!({"title":"Other project"}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
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
        payload: json!({"agent":first.agent,"model":"fixture-model"}),
    };
    let fingerprint = command_fingerprint("session.open", &original);
    let mut resolved = original.clone();
    resolved.payload["binding"] = json!(binding);
    let reused = e
        .execute_resolved("session.open", resolved.clone(), &fingerprint)
        .unwrap();
    assert_eq!(reused["id"], sid);
    assert_eq!(reused["reused"], true);
    resolved.payload["binding"]["model"] = json!("different-default");
    assert_eq!(
        e.execute_resolved("session.open", resolved, &fingerprint)
            .unwrap(),
        reused
    );
    let payload =
        json!({"input":"Discuss acceptance criteria","client_message_id":"frontend-message"});
    let submitted = call("conversation.send", &sid, 1, payload.clone()).unwrap();
    let replay = call("conversation.send", &sid, 1, payload).unwrap();
    assert_eq!(submitted["id"], replay["id"]);
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
    let mut dsh = binding.clone();
    dsh.installation.id = "fixture-dsh-installation".into();
    dsh.installation.key.provider = HarnessProvider::Dsh;
    dsh.adapter_version = "dsh-fixture/v1".into();
    let next = call(
        "session.open",
        &pid,
        1,
        json!({"agent":first.agent,"binding":dsh}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
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
    let back = call(
        "session.open",
        &pid,
        1,
        json!({"agent":first.agent,"binding":binding}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
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
        .transact(None, &mut |s| {
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
    let draft=e.execute("proposal.create",Command{command_id:id(),target:None,expected_revision:None,payload:json!({"turn_id":turn,"content":{"delivery_ref":"builtin-delivery-object:1","title":"任意业务结果","value":{"risks":["scope"]},"input_refs":[material["id"]]}})}).unwrap();
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
    assert_eq!(document.kind, "builtin-delivery-object:1");
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
    fn session_log(&self, _: &RuntimeExecution) -> Result<Option<String>> {
        Ok(Some("sanitized fixture frames".into()))
    }
    fn cleanup_worker(&self, _: &Session, e: &RuntimeExecution) -> Result<()> {
        let s = self.store.snapshot()?;
        let row = s.executions.iter().find(|v| v.id == e.id).unwrap();
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
    let store = Arc::new(SqliteStore::open(dir.path()).unwrap());
    let fixture = Arc::new(ConversationFixture {
        store: store.clone(),
        dispatched: 0.into(),
        stop_allowed: false.into(),
        emit_terminal: true.into(),
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
        .transact(None, &mut |s| {
            s.sessions
                .iter_mut()
                .find(|v| v.id == sid)
                .unwrap()
                .runtime_binding
                .as_mut()
                .unwrap()
                .installation
                .artifact = RuntimeArtifact::DockerImage {
                image_id: format!("sha256:{}", "a".repeat(64)),
            };
            Ok(json!({"ok":true}))
        })
        .unwrap_err();
    // Bind a valid new Session through the public resolved command rather than changing its frozen metadata.
    let state = store.snapshot().unwrap();
    let pid = state.projects[0].id.clone();
    let mut binding = state.sessions[0].runtime_binding.clone().unwrap();
    // Different distribution artifacts require distinct immutable installation identities.
    binding.installation.id = id();
    binding.installation.key.version = "fixture-docker-v1".into();
    binding.installation.artifact = RuntimeArtifact::DockerImage {
        image_id: format!("sha256:{}", "a".repeat(64)),
    };
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
        .execute(
            "session.open",
            Command {
                command_id: id(),
                target: Some(pid),
                expected_revision: Some(1),
                payload: json!({"agent":"builtin-agent-sol-high-analyst:3","binding":binding}),
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
    let instance = e.execute("agent.instance.create",Command{command_id:id(),target:None,expected_revision:None,payload:json!({"name":"Global analyst","scope":{"kind":"global"},"agent_revision_id":agent})}).unwrap()["id"].as_str().unwrap().to_owned();
    let binding = RuntimeBinding {
        model_connection: None,
        model_profile_digest: None,
        agent_configuration: None,
        model_parameters: Default::default(),
        permissions: None,
        installation: RuntimeInstallation {
            id: "global-fixture".into(),
            key: RuntimeKey {
                provider: HarnessProvider::Codex,
                version: "0.167.0".into(),
                os: "linux".into(),
                arch: "aarch64".into(),
            },
            artifact: RuntimeArtifact::DockerImage {
                image_id: format!("sha256:{}", "a".repeat(64)),
            },
            source: "offline fixture".into(),
            license: "test".into(),
        },
        adapter_version: "fixture:native-idle-harness:v1".into(),
        model: "gpt-6.1-sol".into(),
        tool_policy: ToolPolicy::ReadOnly,
        writable_paths: vec![],
        read_only_paths: vec![],
        work_type: WorkType::Discussion,
        configuration_digest: "a".repeat(64),
        credential_ref: None,
    };
    let opened = e
        .execute(
            "session.open",
            Command {
                command_id: id(),
                target: Some("global".into()),
                expected_revision: None,
                payload: json!({"agent":agent,"agent_instance_id":instance,"binding":binding}),
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
    let state = e.store.snapshot().unwrap();
    assert!(state.projects.is_empty());
    assert_eq!(state.sessions[0].scope, Some(SessionScope::Global));
    assert!(!state.snapshots.is_empty());
    assert!(state.governance.iter().any(|r| r.kind == "experience"
        && r.scope == Scope::Global
        && r.data["agent_instance_id"] == instance));
    e.tick_conversations(1).unwrap();
    e.tick_conversations(1).unwrap();
    let completed = e.store.snapshot().unwrap();
    assert!(
        completed
            .snapshots
            .iter()
            .any(|snap| snap.kind == "conversation_completed"
                && snap.content.contains("Completed cross-project work"))
    );
    drop(e);
    let e = engine(dir.path());
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
