use devdesk_adapters::*;
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::json;
use std::sync::Arc;

fn engine(path: &std::path::Path) -> Engine {
    let e = Engine::new(
        Arc::new(SqliteStore::open(path).unwrap()),
        Arc::new(FileArtifacts::new(path.into())),
        simulation::adapters(),
    );
    e.bootstrap().unwrap();
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
                agent: "builtin-agent-analyst-v1".into(),
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
                run_id: None,
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
    assert!(s.features.is_empty() && s.runs.is_empty() && s.attempts.is_empty());
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
    assert_eq!(roles.len(), 5);
    assert_ne!(
        roles
            .iter()
            .find(|d| d.agent_profile.as_ref().unwrap().role == AgentRole::Reviewer)
            .unwrap()
            .id,
        roles
            .iter()
            .find(|d| d.agent_profile.as_ref().unwrap().role == AgentRole::Developer)
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
                run_id: None,
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
    assert!(e.store.snapshot().unwrap().runs.is_empty());
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
    let feature = call(
        "feature.create",
        None,
        None,
        json!({"project_id":project,"title":"CSV export","source_requirement":requirement}),
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
        state.feature(&feature).unwrap().source_requirement.as_ref(),
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
    let newer=call("document.revise",Some(&project),Some(2),json!({"kind":"requirement","title":"Export v2","body":"Export with errors","supersedes":requirement})).unwrap()["id"].as_str().unwrap().to_string();
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
            json!({"project_id":other,"title":"invalid","source_requirement":requirement})
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
        s.feature(&feature).unwrap().source_requirement.as_ref(),
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
    assert_eq!(e.store.snapshot().unwrap().runs.len(), 0);
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
fn command(
    method: &str,
    target: Option<&str>,
    revision: Option<u64>,
    payload: serde_json::Value,
) -> (String, Command) {
    (
        method.into(),
        Command {
            command_id: id(),
            target: target.map(str::to_owned),
            expected_revision: revision,
            payload,
        },
    )
}
#[test]
fn structured_proposals_adopt_atomically_then_reject_stale_and_cross_project_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let (pid, sid, tid) = initialize(&e);
    complete_fixture(&e, &sid, &tid);
    let (_, create) = command(
        "proposal.create",
        None,
        None,
        json!({"turn_id":tid,"target":pid,"content":{"kind":"requirement","title":"Calculator","body":"Add integers","acceptance":[{"key":"AC-1","description":"2+3=5"}]}}),
    );
    let proposal = e.execute("proposal.create", create.clone()).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        e.execute("proposal.create", create).unwrap()["id"],
        proposal
    );
    assert!(e.store.snapshot().unwrap().documents.is_empty());
    let (_, adopt) = command("proposal.adopt", Some(&proposal), Some(1), json!({}));
    let result = e.execute("proposal.adopt", adopt.clone()).unwrap();
    assert_eq!(e.execute("proposal.adopt", adopt).unwrap(), result);
    let req = result["objects"][0].as_str().unwrap();
    let s = e.store.snapshot().unwrap();
    assert_eq!(s.documents.len(), 1);
    assert_eq!(s.documents[0].project_id.as_ref(), Some(&pid));
    assert!(s.deliveries.is_empty());
    let (_, feature) = command(
        "proposal.create",
        None,
        None,
        json!({"turn_id":tid,"target":pid,"content":{"kind":"feature","title":"Arithmetic","requirement":req}}),
    );
    let proposal = e.execute("proposal.create", feature).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let revision = e.store.snapshot().unwrap().projects[0].revision;
    let (_, update) = command(
        "project.update",
        Some(&pid),
        Some(revision),
        json!({"title":"Updated project"}),
    );
    e.execute("project.update", update).unwrap();
    let (_, adopt) = command("proposal.adopt", Some(&proposal), Some(1), json!({}));
    assert!(e.execute("proposal.adopt", adopt).is_err());
    assert!(e.store.snapshot().unwrap().features.is_empty());
    let (_, other) = command(
        "project.create",
        None,
        None,
        json!({"title":"Other project"}),
    );
    let other = e.execute("project.create", other).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, bad) = command(
        "proposal.create",
        None,
        None,
        json!({"turn_id":tid,"target":other,"content":{"kind":"feature","title":"Cross project","requirement":req}}),
    );
    assert!(e.execute("proposal.create", bad).is_err());
}
#[test]
fn proposal_schema_failures_and_task_dependency_cycles_never_create_formal_objects() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let (pid, sid, tid) = initialize(&e);
    complete_fixture(&e, &sid, &tid);
    let (_, bad) = command(
        "proposal.create",
        None,
        None,
        json!({"turn_id":tid,"target":pid,"content":{"kind":"requirement","title":"R","body":"R","acceptance":[]}}),
    );
    assert!(e.execute("proposal.create", bad).is_err());
    let (_, bad) = command(
        "proposal.create",
        None,
        None,
        json!({"turn_id":tid,"target":pid,"content":{"kind":"requirement","title":"R","body":"R","acceptance":[{"key":"A","description":"A"}],"execute":"bad"}}),
    );
    assert!(e.execute("proposal.create", bad).is_err());
    let (_, extract) = command(
        "proposal.extract",
        None,
        None,
        json!({"turn_id":tid,"target":pid}),
    );
    assert!(e.execute("proposal.extract", extract).is_err());
    let s = e.store.snapshot().unwrap();
    assert!(s.proposals.is_empty() && s.documents.is_empty() && s.tasks.is_empty());
}

#[test]
fn task_plan_validates_dependency_graph_and_cannot_skip_planning_review() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let (pid, sid, tid) = initialize(&e);
    complete_fixture(&e, &sid, &tid);
    let (_, req) = command(
        "document.revise",
        Some(&pid),
        Some(1),
        json!({"kind":"requirement","title":"R","body":"R","acceptance":[{"key":"A","description":"A"}]}),
    );
    let req = e.execute("document.revise", req).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, feature) = command(
        "feature.create",
        None,
        None,
        json!({"project_id":pid,"title":"F","source_requirement":req}),
    );
    let fid = e.execute("feature.create", feature).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, design) = command(
        "document.revise",
        Some(&fid),
        Some(1),
        json!({"kind":"design","title":"D","body":"D"}),
    );
    let design = e.execute("document.revise", design).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut content = json!({"kind":"task_plan","requirement":req,"design":design,"tasks":[{"key":"one","title":"One","acceptance":["A"],"dependencies":["two"]},{"key":"two","title":"Two","acceptance":["A"],"dependencies":["one"]}]});
    let (_, cycle) = command(
        "proposal.create",
        None,
        None,
        json!({"turn_id":tid,"target":fid,"content":content}),
    );
    assert!(e.execute("proposal.create", cycle).is_err());
    assert!(e.store.snapshot().unwrap().proposals.is_empty());
    content["tasks"][0]["dependencies"] = json!([]);
    let (_, create) = command(
        "proposal.create",
        None,
        None,
        json!({"turn_id":tid,"target":fid,"content":content}),
    );
    let proposal = e.execute("proposal.create", create).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, adopt) = command("proposal.adopt", Some(&proposal), Some(1), json!({}));
    e.execute("proposal.adopt", adopt).unwrap();
    let (_, instantiate) = command("proposal.instantiate", Some(&proposal), Some(2), json!({}));
    assert!(e.execute("proposal.instantiate", instantiate).is_err());
    let s = e.store.snapshot().unwrap();
    assert!(s.tasks.is_empty());
    assert!(s.proposals[0].task_instances.is_empty());
}

struct ConversationFixture {
    store: Arc<SqliteStore>,
    dispatched: std::sync::atomic::AtomicUsize,
    stop_allowed: std::sync::atomic::AtomicBool,
    emit_terminal: std::sync::atomic::AtomicBool,
}
impl ports::Harness for ConversationFixture {
    fn configure(&self, _: &str, _: &str) -> String {
        "fixture".into()
    }
    fn poll(&self, _: &Run, _: &serde_json::Value, _: u64) -> Result<Option<serde_json::Value>> {
        Ok(None)
    }
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
    let mut adapters = simulation::adapters();
    adapters.harness = fixture.clone();
    let e = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(dir.path().into())),
        adapters,
    );
    e.bootstrap().unwrap();
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
                payload: json!({"agent":"builtin-agent-analyst-v1","binding":binding}),
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
