#[path = "../../../tests/support/permissions.rs"]
mod permission_fixture;
// Real SQLite/Artifact/Engine round trips; an explicit recording port never calls a model.
use devdesk_adapters::{FileArtifacts, SqliteStore};
use devdesk_core::{ports::*, *};
use devdesk_domain::*;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
#[path = "../../../tests/support/schemas.rs"]
mod schemas;

#[derive(Default)]
struct Recorder {
    calls: Mutex<Vec<TurnDispatch>>,
    starts: Mutex<Vec<SessionStart>>,
    pending: Mutex<HashMap<Id, Vec<ConversationObservation>>>,
}
impl devdesk_core::ports::Harness for Recorder {
    fn identity(&self) -> String {
        "fixture:task-recorder".into()
    }
    fn open_session(&self, r: &SessionStart) -> Result<SessionReceipt> {
        self.starts.lock().unwrap().push(r.clone());
        if let Some(base) = &r.task_base {
            assert_eq!(base.agent.id, r.agent.id);
        }
        Ok(SessionReceipt {
            external_session: format!("fixture-{}", r.session.id),
            container_id: "fixture-container".into(),
            image_digest: r.execution.image_digest.clone(),
            operation_id: r.execution.operation_id.clone(),
        })
    }
    fn dispatch_turn(&self, r: &TurnDispatch) -> Result<()> {
        self.calls.lock().unwrap().push(r.clone());
        self.pending.lock().unwrap().insert(
            r.session.id.clone(),
            vec![
                ConversationObservation::Admitted {
                    turn: r.turn.id.clone(),
                    message: Some("fixture-consumed".into()),
                    external_turn: Some(format!("fixture-{}", r.turn.id)),
                },
                ConversationObservation::Consumed {
                    turn: r.turn.id.clone(),
                    message: Some("fixture-consumed".into()),
                },
                ConversationObservation::Terminal {
                    turn: r.turn.id.clone(),
                    outcome: ProviderTerminal::Completed,
                    reason: "explicit fixture".into(),
                    quiescent: true,
                },
            ],
        );
        Ok(())
    }
    fn poll_session(&self, s: &Session) -> Result<Vec<ConversationObservation>> {
        Ok(self
            .pending
            .lock()
            .unwrap()
            .remove(&s.id)
            .unwrap_or_default())
    }
    fn stop_session(&self, _: &Session, e: &RuntimeExecution) -> Result<WorkerStop> {
        Ok(WorkerStop {
            operation_id: e.operation_id.clone(),
            writer_revoked: true,
            mode: "fixture".into(),
            requires_successor: false,
        })
    }
}
fn engine(path: &std::path::Path, port: Arc<Recorder>) -> Engine {
    let mut adapters = runtime::adapters();
    adapters.harness = port;
    let e = Engine::new(
        Arc::new(SqliteStore::open(path).unwrap()),
        Arc::new(FileArtifacts::new(path.into())),
        adapters,
    );
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    e
}
fn command(e: &Engine, method: &str, target: Option<&str>, payload: Value) -> Result<Value> {
    let s = e.store.snapshot()?;
    let revision = target.and_then(|id| {
        s.governance
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.revision)
            .or_else(|| s.projects.iter().find(|r| r.id == id).map(|r| r.revision))
            .or_else(|| s.features.iter().find(|r| r.id == id).map(|r| r.revision))
            .or_else(|| s.tasks.iter().find(|r| r.id == id).map(|r| r.revision))
            .or_else(|| s.sessions.iter().find(|r| r.id == id).map(|r| r.revision))
            .or_else(|| s.proposals.iter().find(|r| r.id == id).map(|r| r.revision))
    });
    e.execute(
        method,
        Command {
            command_id: id(),
            target: target.map(str::to_owned),
            expected_revision: revision,
            payload,
        },
    )
}
fn project(e: &Engine) -> Id {
    let p = command(
        e,
        "project.create",
        None,
        json!({"title":"Task v3 fixture"}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let defaults = e.configuration_defaults(Some(&p)).unwrap();
    assert_eq!(defaults["workflow_ref"], DEFAULT_FEATURE_WORKFLOW_V3_ID);
    command(e,"project.configuration.update",Some(&p),json!({"workflow_ref":defaults["workflow_ref"],"role_agent_bindings":defaults["role_agent_bindings"]})).unwrap();
    p
}
fn feature(e: &Engine, p: &str) -> Id {
    command(
        e,
        "feature.create",
        None,
        json!({"project_id":p,"title":"Frozen task"}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}
fn binding() -> RuntimeBinding {
    RuntimeBinding {
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
            artifact: RuntimeArtifact::DockerImage {
                image_id: format!("sha256:{}", "a".repeat(64)),
            },
            source: "explicit recording port; no Docker/model".into(),
            license: "test".into(),
        },
        adapter_version: "fixture:v3".into(),
        model: "fixture".into(),
        tool_policy: ToolPolicy::ReadOnly,
        writable_paths: vec![],
        read_only_paths: vec![],
        work_type: WorkType::Discussion,
        configuration_digest: "a".repeat(64),
        credential_ref: None,
    }
}
fn open(e: &Engine, fid: &str, key: &str, node: Option<&str>) -> Id {
    let s = e.store.snapshot().unwrap();
    let task = s.tasks.iter().find(|t| t.id == fid);
    let lock = s
        .feature_configuration(task.map_or(fid, |t| t.feature_id.as_str()))
        .unwrap();
    let rt = s
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.definition_id == fid)
        .unwrap()
        .decode::<WorkflowRuntime>()
        .unwrap();
    let plan = if task.is_some() {
        &lock.task_plan
    } else {
        &lock.plan
    };
    let step = plan.steps.iter().find(|v| v.key == key).unwrap_or_else(|| {
        panic!(
            "Missing step {key}; available {:?}",
            plan.steps.iter().map(|s| &s.key).collect::<Vec<_>>()
        )
    });
    let role = node
        .and_then(|n| step.execution_recipe.iter().find(|v| v.key == n))
        .and_then(|n| n.role_ref.as_ref())
        .or(step.role_ref.as_ref())
        .unwrap();
    let actor = &lock.role_agent_bindings[role];
    let instance = &rt.steps.iter().find(|s| s.key == key).unwrap().id;
    let mut runtime = binding();
    if task.is_some() && node == Some("docs") {
        runtime.read_only_paths = vec!["tests/frozen".into()];
        runtime.configuration_digest = "b".repeat(64);
    }
    command(e,"session.open",Some(fid),json!({"agent":actor,"binding":runtime,"workflow_step_instance_id":instance,"workflow_recipe_node":node})).unwrap()["id"].as_str().unwrap().to_owned()
}
fn send(e: &Engine, sid: &str, text: &str) -> Value {
    command(
        e,
        "conversation.send",
        Some(sid),
        json!({"input":text,"client_message_id":id()}),
    )
    .unwrap()
}

#[test]
fn v3_contracts_validate_every_agent_lane_and_round_trip_current_bytes() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path(), Arc::default());
    let s = e.store.snapshot().unwrap();
    let old = s
        .configuration(DEFAULT_FEATURE_WORKFLOW_V3_ID)
        .unwrap()
        .data
        .clone();
    let mut doc: WorkflowDocument = s
        .configuration(DEFAULT_FEATURE_WORKFLOW_V3_ID)
        .unwrap()
        .decode()
        .unwrap();
    assert!(validate_workflow(&doc).is_ok());
    let old_typed: WorkflowDocument = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(json!(old_typed), old);
    let index = doc
        .steps
        .iter()
        .position(|s| s.kind == "agent.interact")
        .unwrap();
    let mut retired = json!(doc);
    retired["steps"][index]["interaction"]["goal"] = json!("Retired discussion goal");
    assert!(serde_json::from_value::<WorkflowDocument>(retired).is_err());
    doc.steps[index].task_instructions = None;
    assert!(validate_workflow(&doc).is_err());
    let mut raw = s
        .configuration(DEFAULT_FEATURE_WORKFLOW_V3_ID)
        .unwrap()
        .data
        .clone();
    raw["steps"][index]["task_instructions"] = Value::Null;
    assert!(serde_json::from_value::<WorkflowDocument>(raw).is_err());
    let mut doc: WorkflowDocument = s
        .configuration(DEFAULT_TASK_WORKFLOW_V3_ID)
        .unwrap()
        .decode()
        .unwrap();
    let execute = doc
        .steps
        .iter_mut()
        .find(|s| s.kind == "attempt.execute")
        .unwrap();
    let node = execute
        .execution_recipe
        .iter_mut()
        .find(|n| n.key == "red")
        .unwrap();
    node.task_instructions = Some("This field must not pretend to control the test runner".into());
    assert!(
        validate_workflow(&doc)
            .unwrap_err()
            .message
            .contains("系统执行节点")
    );
    let mut doc: WorkflowDocument = s
        .configuration(DEFAULT_FEATURE_WORKFLOW_V3_ID)
        .unwrap()
        .decode()
        .unwrap();
    doc.steps[index].task_instructions = Some("🙂".repeat(TASK_TEXT_LIMIT / 4 + 1));
    assert!(validate_workflow(&doc).is_err());
    let mut repeated = doc.steps[index].clone();
    repeated.task_instructions = Some("a".repeat(TASK_TEXT_LIMIT));
    doc.steps = (0..33)
        .map(|i| {
            let mut step = repeated.clone();
            step.key = format!("task-{i}");
            step
        })
        .collect();
    assert!(
        validate_workflow_tasks(&doc)
            .unwrap_err()
            .message
            .contains("1 MiB")
    );
    let contracts = workflow_action_contracts();
    assert!(
        !contracts["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"] == "merge.prepare")
            .unwrap()["requires_task"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(
        s.configuration(DEFAULT_FEATURE_WORKFLOW_V3_ID)
            .unwrap()
            .data,
        old
    );
}

#[test]
fn task_markdown_round_trips_through_revision_preview_and_real_storage() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path(), Arc::default());
    let text = "## 本次任务\n\n保留逗号, 与空行。\n```js\nconsole.log('{{literal}}')\n```\n🙂";
    let mut doc = e
        .store
        .snapshot()
        .unwrap()
        .configuration(DEFAULT_FEATURE_WORKFLOW_V3_ID)
        .unwrap()
        .data
        .clone();
    let index = doc["steps"]
        .as_array()
        .unwrap()
        .iter()
        .position(|s| s["kind"] == "agent.interact")
        .unwrap();
    let key = doc["steps"][index]["key"].clone();
    doc["steps"][index]["task_instructions"] = json!(text);
    let row = command(
        &e,
        "workflow.create",
        None,
        json!({"name":"Literal task","document":doc}),
    )
    .unwrap();
    let published = command(&e, "workflow.publish", row["id"].as_str(), json!({})).unwrap();
    assert_eq!(published["data"]["steps"][index]["task_instructions"], text);
    let preview = e
        .query(
            "query.workflow_task_preview",
            &json!({"id":published["id"],"path":key}),
            4,
        )
        .unwrap();
    assert_eq!(preview["task"]["instructions"], text);
    assert_eq!(preview["side_effects"], false);
    assert!(preview["base_context"].is_null());
    assert!(
        command(
            &e,
            "workflow.task_upgrade",
            Some(DEFAULT_FEATURE_WORKFLOW_V3_ID),
            json!({})
        )
        .is_err()
    );
}

#[test]
fn every_agent_step_and_feature_recipe_dispatches_its_task_role_skills_and_specs() {
    let root = tempfile::tempdir().unwrap();
    let port = Arc::new(Recorder::default());
    let e = engine(root.path(), port.clone());
    let p = project(&e);
    for (key, node) in [
        ("requirement_discussion", None),
        ("requirement_execute", Some("requirement_validate")),
        ("solution_execute", Some("plan_validate")),
        ("solution_execute", Some("test_plan_validate")),
        ("closure", Some("test_author")),
        ("closure", Some("docs")),
        ("requirement_review", None),
        ("merge_review", None),
    ] {
        let f = feature(&e, &p);
        let sid = open(&e, &f, key, node);
        let sent = send(&e, &sid, "Fixture user input, not a business task fallback");
        let record = e
            .query("query.execution_task", &json!({"turn_id":sent["id"]}), 4)
            .unwrap();
        assert_eq!(
            record["task"]["content"]["source"]["recipe_node"],
            json!(node)
        );
        assert!(
            record["task"]["content"]["task"]["instructions"]
                .as_str()
                .unwrap()
                .starts_with('#')
        );
        assert!(
            !record["task"]["base"]["skills"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            !record["task"]["content"]["specifications"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        schemas::SCHEMAS.assert_valid("agent-task.schema.json", &record["task"]);
        e.tick_conversations(4).unwrap();
        e.tick_conversations(4).unwrap();
        let session_detail = e.query("query.session", &json!({"id":sid}), 4).unwrap();
        if key != "requirement_discussion" {
            let error = command(
                &e,
                "proposal.extract",
                None,
                json!({"turn_id":sent["id"],"target":f}),
            )
            .unwrap_err();
            assert!(error.message.contains("正式交付或评审结果不属于需求提案"));
            assert_ne!(
                session_detail["output_contract"]["id"],
                "proposal-content/v1"
            );
        }
        let captured = port.calls.lock().unwrap().last().unwrap().clone();
        let frozen = captured.task.as_ref().unwrap();
        assert!(
            !frozen.content["quality_basis"]["skills"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        if frozen.content["output_contract"]["id"] == STAGE_ANSWER_CONTRACT {
            assert_eq!(
                frozen.content["output_contract"]["schema"]["properties"]["checks"]["items"]["properties"]
                    ["passed"]["type"],
                "boolean"
            );
            assert!(
                frozen.content["output_contract"]["required_fields"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("approve"))
            );
            for renderer in ["agent-task/v1", "agent-task/v2"] {
                let mut old_task = record["task"].clone();
                old_task["renderer_version"] = json!(renderer);
                assert!(
                    !schemas::SCHEMAS
                        .validator("agent-task.schema.json")
                        .is_valid(&old_task)
                );
                let mut old_definition = frozen.definition.clone();
                old_definition.renderer_version = renderer.into();
                assert!(old_definition.decode_step().is_err());
            }
        }
        let native = devdesk_adapters::harness::provider_turn_text(&captured).unwrap();
        assert_eq!(native, frozen.rendered_input);
        assert_eq!(
            serde_json::from_str::<Value>(&native).unwrap()["task"],
            record["task"]["content"]["task"]
        );
        assert_eq!(frozen.base.agent.id, captured.session.agent);
        assert_eq!(
            e.store
                .snapshot()
                .unwrap()
                .turns
                .iter()
                .find(|t| Some(t.id.as_str()) == sent["id"].as_str())
                .unwrap()
                .status,
            TurnStatus::Completed
        );
    }
    assert_eq!(port.calls.lock().unwrap().len(), 8);
}

#[test]
fn queued_task_survives_restart_directory_changes_and_continuation_without_rereading_workflow() {
    let root = tempfile::tempdir().unwrap();
    let port = Arc::new(Recorder::default());
    let e = engine(root.path(), port.clone());
    let p = project(&e);
    let f = feature(&e, &p);
    let sid = open(&e, &f, "requirement_discussion", None);
    e.store
        .transact(None, &mut |s| {
            let c = s.contexts.iter_mut().find(|c| c.owner_id == f).unwrap();
            c.content = "Context at input acceptance".into();
            c.revision += 1;
            Ok(Value::Null)
        })
        .unwrap();
    let sent = send(&e, &sid, "First user input");
    let before = e
        .query("query.execution_task", &json!({"turn_id":sent["id"]}), 4)
        .unwrap();
    assert_eq!(
        before["task"]["content"]["context"]["content"],
        "Context at input acceptance"
    );
    e.store
        .transact(None, &mut |s| {
            let c = s.contexts.iter_mut().find(|c| c.owner_id == f).unwrap();
            c.content = "Changed after input was queued".into();
            c.revision += 1;
            Ok(Value::Null)
        })
        .unwrap();
    let draft = command(
        &e,
        "workflow.draft.create",
        None,
        json!({"base_revision_id":DEFAULT_FEATURE_WORKFLOW_V3_ID}),
    )
    .unwrap();
    let mut doc = draft["data"]["document"].clone();
    let step = doc["steps"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["key"] == "requirement_discussion")
        .unwrap();
    step["task_instructions"] = json!("NEW WORKFLOW TASK MUST NOT LEAK INTO QUEUED INPUT");
    command(
        &e,
        "workflow.draft.update",
        draft["id"].as_str(),
        json!({"name":"Changed workflow","document":doc}),
    )
    .unwrap();
    command(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    drop(e);
    let e = engine(root.path(), port.clone());
    assert_eq!(
        e.query("query.execution_task", &json!({"turn_id":sent["id"]}), 4)
            .unwrap(),
        before
    );
    e.tick_conversations(4).unwrap();
    e.tick_conversations(4).unwrap();
    assert_eq!(
        port.calls.lock().unwrap()[0].task.as_ref().unwrap().digest,
        before["task"]["digest"]
    );
    let next = send(&e, &sid, "Additional user facts");
    let after = e
        .query("query.execution_task", &json!({"turn_id":next["id"]}), 4)
        .unwrap();
    assert_eq!(
        after["task"]["definition_digest"],
        before["task"]["definition_digest"]
    );
    assert_ne!(
        after["task"]["input_digest"],
        before["task"]["input_digest"]
    );
    assert_eq!(after["task"]["content"]["input"], "Additional user facts");
    assert!(
        !after["task"]["rendered_input"]
            .as_str()
            .unwrap()
            .contains("NEW WORKFLOW TASK")
    );
}

fn task_fixture(e: &Engine, f: &str) -> Id {
    let f = f.to_owned();
    let tid = id();
    e.store
        .transact(None, &mut |s| {
            let feature = s.feature(&f)?.clone();
            let lock = s.feature_configuration(&f).unwrap();
            let mut record = s
                .governance
                .iter()
                .find(|r| r.kind == "workflow_instance" && r.definition_id == f)
                .unwrap()
                .clone();
            let mut runtime: WorkflowRuntime = record.decode()?;
            runtime.owner_id = tid.clone();
            runtime.task_id = Some(tid.clone());
            runtime.workflow_ref = DEFAULT_TASK_WORKFLOW_V3_ID.into();
            runtime.steps = lock
                .task_plan
                .steps
                .iter()
                .map(|step| WorkflowStepRuntime {
                    id: id(),
                    key: step.key.clone(),
                    round: 1,
                    status: "pending".into(),
                    input_hash: String::new(),
                    outputs: Default::default(),
                    reason: String::new(),
                    revision: 1,
                    confirmation: None,
                })
                .collect();
            record.id = id();
            record.definition_id = tid.clone();
            record.data = json!(runtime);
            record.content_hash = configuration_hash(&record.data);
            s.governance.push(record);
            let snapshot = id();
            s.snapshots.push(Snapshot {
                id: snapshot.clone(),
                project_id: None,
                feature_id: Some(f.clone()),
                task_id: None,
                kind: "explicit-fixture".into(),
                content: "Frozen Task fixture context".into(),
                binding: feature.binding.clone(),
                documents: vec![],
                delivery_id: None,
                inherited_from: None,
                capture: SnapshotCapture {
                    attempt: None,
                    runs: vec![],
                    sessions: vec![],
                    resources: vec![],
                    captured_at: Some(1),
                    reason: "Explicit test fixture; no accepted delivery claim".into(),
                },
                runtime_bindings: vec![],
                execution_instances: vec![],
            });
            s.tasks.push(Task {
                id: tid.clone(),
                feature_id: f.clone(),
                title: "Explicit Task fixture".into(),
                status: Status::Ready,
                revision: 1,
                binding: feature.binding,
                inherited_snapshot: snapshot,
                dependencies: vec![],
                worktree: "fixture".into(),
                scope: TaskScope::default(),
            });
            s.contexts.push(Context {
                id: id(),
                owner_id: tid.clone(),
                content: format!("Task context {tid}"),
                revision: 1,
            });
            Ok(Value::Null)
        })
        .unwrap();
    tid
}

#[test]
fn task_attempt_freezes_each_node_scope_before_dispatch_on_shared_sessions() {
    let root = tempfile::tempdir().unwrap();
    let port = Arc::new(Recorder::default());
    let e = engine(root.path(), port.clone());
    let p = project(&e);
    let f = feature(&e, &p);
    let tid = task_fixture(&e, &f);
    let upstream = e
        .artifacts
        .seal(&f, None, "deliveries", "# Accepted upstream fixture")
        .unwrap();
    // Explicit accepted upstream facts; this test exercises the real Attempt command,
    // scheduler and native dispatch without executing Git, tests or a model.
    e.store
        .transact(None, &mut |s| {
            s.artifacts.push(upstream.clone());
            let feature = s.feature(&f)?.clone();
            let mut docs = vec![];
            for kind in ["requirement", "design"] {
                let did = id();
                s.documents.push(Document {
                    id: did.clone(),
                    feature_id: Some(f.clone()),
                    project_id: None,
                    task_id: None,
                    kind: kind.into(),
                    title: kind.into(),
                    body: format!("# Fixture {kind}"),
                    version: 1,
                    supersedes: None,
                    requirement_id: None,
                    design_id: None,
                    origin: Origin::Human,
                    acceptance: vec![],
                    verifies: vec![],
                    artifact_id: None,
                    test_recipe: None,
                });
                docs.push(did);
            }
            let task = s.tasks.iter_mut().find(|t| t.id == tid).unwrap();
            task.scope.requirement = Some(docs[0].clone());
            task.scope.design = Some(docs[1].clone());
            let aid = id();
            s.attempts.push(Attempt {
                id: aid.clone(),
                feature_id: f.clone(),
                task_id: None,
                phase: "planning".into(),
                baseline: Some("a".repeat(40)),
                source_base: Some("a".repeat(40)),
                live_sessions: Default::default(),
                git_context: None,
                status: Status::Succeeded,
                input_version: feature.input_version,
                documents: docs.clone(),
                outputs: vec![],
                scope: None,
                binding: feature.binding,
                context_content: "Accepted fixture".into(),
                context_revision: 1,
                paused: false,
                revision: 1,
            });
            for kind in ["requirement", "plan"] {
                let delivery = id();
                s.deliveries.push(Delivery {
                    id: delivery.clone(),
                    feature_id: f.clone(),
                    task_id: None,
                    attempt_id: Some(aid.clone()),
                    merge_id: None,
                    kind: kind.into(),
                    input_version: feature.input_version,
                    documents: docs.clone(),
                    evidence: vec![],
                    artifact_id: upstream.id.clone(),
                    source_deliveries: vec![],
                    candidate: Some("a".repeat(40)),
                    stale: false,
                    origin: Origin::External,
                });
                s.reviews.push(Review {
                    id: id(),
                    delivery_id: delivery,
                    decision: Decision::Pass,
                    checks: vec![],
                    actor: "human:fixture".into(),
                    run_id: None,
                    reason: "Explicit accepted upstream fixture".into(),
                    assignment: None,
                });
            }
            for owner in [&f, &tid] {
                let worktree = RepositoryWorktree {
                    id: id(),
                    project_id: p.clone(),
                    owner_id: owner.clone(),
                    kind: if owner == &tid {
                        WorktreeKind::Task
                    } else {
                        WorktreeKind::Feature
                    },
                    branch: "fixture".into(),
                    path: format!("fixture-{owner}"),
                    base: "a".repeat(40),
                    operation_id: id(),
                };
                s.repository_operations.push(RepositoryOperation {
                    id: worktree.operation_id.clone(),
                    project_id: p.clone(),
                    action: RepositoryAction::CreateWorktree {
                        owner: owner.clone(),
                        worktree_kind: worktree.kind,
                        base: worktree.base.clone(),
                    },
                    status: RepositoryOperationStatus::Completed,
                    receipt: Some(RepositoryReceipt::Worktree {
                        worktree: worktree.clone(),
                    }),
                    reason: String::new(),
                    revision: 1,
                });
                s.repository_worktrees.push(worktree);
                s.heads.push(GitHead {
                    id: owner.clone(),
                    project_id: p.clone(),
                    revision: "a".repeat(40),
                });
            }
            Ok(Value::Null)
        })
        .unwrap();
    let state = e.store.snapshot().unwrap();
    let lock = state.feature_configuration(&f).unwrap();
    let step = lock
        .task_plan
        .steps
        .iter()
        .find(|step| step.kind == "attempt.execute")
        .unwrap();
    let runtime: WorkflowRuntime = state
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.definition_id == tid)
        .unwrap()
        .decode()
        .unwrap();
    let instance = &runtime.steps.iter().find(|s| s.key == step.key).unwrap().id;
    let mut sessions = serde_json::Map::new();
    for node in &step.execution_recipe {
        let role = node.role_ref.as_ref().or(step.role_ref.as_ref()).unwrap();
        let mut runtime = binding();
        match node.key.as_str() {
            "implement" => {
                runtime.work_type = WorkType::Implementation;
                runtime.tool_policy = ToolPolicy::TaskWrite;
                runtime.writable_paths = vec![".".into()];
                runtime.read_only_paths = vec!["tests".into()];
            }
            "test_author" => {
                runtime.work_type = WorkType::Testing;
                runtime.tool_policy = ToolPolicy::TestWrite;
                runtime.writable_paths = vec!["tests/task-suite".into()];
            }
            _ => {}
        }
        runtime.configuration_digest = task_content_hash(&json!(&runtime));
        let opened = command(
            &e,
            "session.open",
            Some(&tid),
            json!({
                "agent":lock.role_agent_bindings[role], "binding":runtime,
                "workflow_step_instance_id":instance, "workflow_recipe_node":node.key,
            }),
        )
        .unwrap();
        sessions.insert(node.key.clone(), opened["id"].clone());
    }
    assert_eq!(sessions["implement"], sessions["docs"]);
    assert_eq!(sessions["test_author"], sessions["red"]);
    assert_eq!(sessions["test_author"], sessions["green"]);
    assert_eq!(sessions["test_author"], sessions["verify"]);
    let attempt = command(
        &e,
        "attempt.start",
        Some(&tid),
        json!({"live_sessions":sessions,"workflow_step_instance_id":instance}),
    )
    .unwrap();
    let state = e.store.snapshot().unwrap();
    let runs: Vec<_> = state
        .runs
        .iter()
        .filter(|r| r.attempt_id.as_deref() == attempt["id"].as_str())
        .cloned()
        .collect();
    assert_eq!(runs.len(), step.execution_recipe.len());
    for run in &runs {
        let frozen = state
            .governance
            .iter()
            .find(|r| r.kind == "execution_task_binding" && r.definition_id == run.id)
            .unwrap();
        assert_eq!(frozen.data["recipe_node"], run.node);
        assert_eq!(frozen.data["session_id"], run.session_id);
        assert_eq!(
            frozen.data["runtime_binding"]["tool_policy"],
            match run.node.as_str() {
                "implement" => "task_write",
                "test_author" => "test_write",
                _ => "read_only",
            }
        );
    }
    let implementation = runs.iter().find(|r| r.node == "implement").unwrap();
    let frozen = state
        .governance
        .iter()
        .find(|r| r.kind == "execution_task_binding" && r.definition_id == implementation.id)
        .unwrap()
        .clone();
    // Opening docs again must not replace permissions already frozen for implementation.
    assert_eq!(
        open(&e, &tid, &step.key, Some("docs")),
        implementation.session_id
    );
    e.store
        .transact(None, &mut |s| {
            s.attempts
                .iter_mut()
                .find(|a| Some(a.id.as_str()) == attempt["id"].as_str())
                .unwrap()
                .baseline = Some("a".repeat(40));
            for run in s
                .runs
                .iter_mut()
                .filter(|r| r.attempt_id.as_deref() == attempt["id"].as_str())
            {
                run.status = if run.node == "implement" {
                    Status::Running
                } else {
                    Status::Cancelled
                };
                run.started = Some(1);
                run.finished = (run.node != "implement").then_some(2);
            }
            Ok(Value::Null)
        })
        .unwrap();
    e.tick(4).unwrap();
    e.tick_conversations(4).unwrap();
    let dispatch = port.calls.lock().unwrap()[0].clone();
    assert_eq!(dispatch.session.id, implementation.session_id);
    assert_eq!(
        dispatch
            .session
            .runtime_binding
            .as_ref()
            .unwrap()
            .tool_policy,
        ToolPolicy::TaskWrite
    );
    assert_eq!(
        dispatch.task.as_ref().unwrap().content["source"]["recipe_node"],
        "implement"
    );
    assert_eq!(
        dispatch.task.as_ref().unwrap().content["authorization"]["read_only_paths"],
        json!(["tests"])
    );
    let state = e.store.snapshot().unwrap();
    assert_eq!(state.configuration(&frozen.id).unwrap(), &frozen);
    assert_eq!(
        state.sessions.iter().filter(|s| s.owner_id == tid).count(),
        2
    );
    let session = state
        .sessions
        .iter()
        .find(|s| s.id == implementation.session_id)
        .unwrap();
    assert!(
        e.send_with_permission_binding(
            Command {
                command_id: id(),
                target: Some(session.id.clone()),
                expected_revision: Some(session.revision),
                payload: json!({"input":"Changed permission fixture","client_message_id":id()}),
            },
            binding()
        )
        .unwrap_err()
        .message
        .contains("显式新建")
    );
    e.tick_conversations(4).unwrap();
    e.store
        .transact(None, &mut |s| {
            let run = s
                .runs
                .iter_mut()
                .find(|r| r.id == implementation.id)
                .unwrap();
            run.status = Status::Failed;
            run.finished = Some(3);
            run.result =
                Some(json!({"failure_type":"adapter_contract","message":"Explicit retry fixture"}));
            Ok(Value::Null)
        })
        .unwrap();
    let retry = command(&e, "run.retry", Some(&implementation.id), json!({})).unwrap();
    let state = e.store.snapshot().unwrap();
    let retry = state
        .runs
        .iter()
        .find(|r| Some(r.id.as_str()) == retry["id"].as_str())
        .unwrap();
    assert_eq!(retry.session_id, implementation.session_id);
    assert_eq!(retry.retry_of.as_deref(), Some(implementation.id.as_str()));
    assert_eq!(
        state.sessions.iter().filter(|s| s.owner_id == tid).count(),
        2
    );
    let prepare = lock
        .task_plan
        .steps
        .iter()
        .find(|step| step.kind == "merge.prepare")
        .unwrap();
    let tester = open(&e, &tid, &prepare.key, None);
    assert_eq!(json!(tester), sessions["test_author"]);
    let view = e.query("query.session", &json!({"id":tester}), 4).unwrap();
    assert_eq!(view["current_runtime_binding"]["tool_policy"], "read_only");
    assert_eq!(
        view["session"]["runtime_binding"]["tool_policy"],
        "test_write"
    );
    assert_eq!(view["workflow_context"]["step"], prepare.key);
}

#[test]
fn task_recipe_and_same_named_feature_review_keep_distinct_sources_for_two_tasks() {
    let root = tempfile::tempdir().unwrap();
    let port = Arc::new(Recorder::default());
    let e = engine(root.path(), port.clone());
    let p = project(&e);
    // A Feature call named "task" intentionally shares compiled step keys with
    // the Task plan. Only the full source path distinguishes their bindings.
    let state = e.store.snapshot().unwrap();
    let mut child = state
        .configuration(DEFAULT_TASK_WORKFLOW_V3_ID)
        .unwrap()
        .data
        .clone();
    child["process_kind"] = json!("subworkflow");
    child["steps"]
        .as_array_mut()
        .unwrap()
        .retain(|s| ["attempt.execute", "delivery.review"].contains(&s["kind"].as_str().unwrap()));
    child["output_ports"] = json!({});
    for step in child["steps"].as_array_mut().unwrap() {
        if let Some(text) = step["task_instructions"].as_str() {
            step["task_instructions"] = json!(format!("FEATURE ONLY: {text}"));
        }
    }
    let draft = command(
        &e,
        "workflow.create",
        None,
        json!({"name":"Feature call with colliding compiled keys","document":child}),
    )
    .unwrap();
    let child = command(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let mut root_workflow = state
        .configuration(DEFAULT_FEATURE_WORKFLOW_V3_ID)
        .unwrap()
        .data
        .clone();
    let stage = root_workflow["stages"][0]["key"].clone();
    root_workflow["steps"].as_array_mut().unwrap().push(json!({"key":"task","name":"Feature-only call","stage":stage,"kind":"workflow.call","subworkflow_ref":child["id"]}));
    let draft = command(
        &e,
        "workflow.create",
        None,
        json!({"name":"Colliding Feature and Task source fixture","document":root_workflow}),
    )
    .unwrap();
    let root_workflow = command(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let defaults = e.configuration_defaults(Some(&p)).unwrap();
    command(&e,"project.configuration.update",Some(&p),json!({"workflow_ref":root_workflow["id"],"role_agent_bindings":defaults["role_agent_bindings"]})).unwrap();
    let f = feature(&e, &p);
    let mut all_task_sessions = Vec::new();
    for _ in 0..2 {
        let mut by_agent = HashMap::new();
        let mut frozen_turns = Vec::new();
        let tid = task_fixture(&e, &f);
        for (key, node) in [
            ("implementation", Some("test_author")),
            ("implementation", Some("implement")),
            ("implementation", Some("docs")),
            ("review", None),
            ("merge_review", None),
        ] {
            let kind = match key {
                "implementation" => "attempt.execute",
                "review" => "delivery.review",
                _ => "merge.review",
            };
            let compiled_key = e
                .store
                .snapshot()
                .unwrap()
                .feature_configuration(&f)
                .unwrap()
                .task_plan
                .steps
                .iter()
                .find(|s| s.kind == kind)
                .unwrap()
                .key
                .clone();
            let sid = open(&e, &tid, &compiled_key, node);
            assert_eq!(
                open(&e, &tid, &compiled_key, node),
                sid,
                "same Task, Agent and frozen node must reuse its session"
            );
            let actor = e
                .store
                .snapshot()
                .unwrap()
                .sessions
                .iter()
                .find(|session| session.id == sid)
                .unwrap()
                .agent
                .clone();
            if let Some(previous) = by_agent.insert(actor, sid.clone()) {
                assert_eq!(
                    previous, sid,
                    "same Task/Agent must reuse its native session across nodes and steps"
                );
            }
            let sent = send(&e, &sid, "Task input");
            frozen_turns.push(sent["id"].as_str().unwrap().to_owned());
            let task = e
                .query("query.execution_task", &json!({"turn_id":sent["id"]}), 4)
                .unwrap();
            assert_eq!(
                task["task"]["definition"]["workflow_revision_id"],
                DEFAULT_TASK_WORKFLOW_V3_ID
            );
            assert_eq!(
                task["task"]["definition"]["path"],
                format!("tasks/task/{key}")
            );
            assert_eq!(task["task"]["content"]["source"]["task_id"], tid);
            assert_eq!(
                task["task"]["content"]["authorization"]["read_only_paths"],
                if node == Some("docs") {
                    json!(["tests/frozen"])
                } else {
                    json!([])
                }
            );
            assert_eq!(
                task["task"]["content"]["source"]["recipe_node"],
                json!(node)
            );
            if node == Some("docs") {
                let view = e.query("query.session", &json!({"id":sid}), 4).unwrap();
                assert_eq!(
                    view["current_runtime_binding"]["read_only_paths"],
                    json!(["tests/frozen"])
                );
                assert_eq!(
                    view["session"]["runtime_binding"]["read_only_paths"],
                    json!([])
                );
            }
            e.tick_conversations(4).unwrap();
            e.tick_conversations(4).unwrap();
            let dispatch = port.calls.lock().unwrap().last().unwrap().clone();
            assert_eq!(
                json!(
                    dispatch
                        .session
                        .runtime_binding
                        .as_ref()
                        .unwrap()
                        .read_only_paths
                ),
                task["task"]["content"]["authorization"]["read_only_paths"]
            );
            assert_eq!(
                port.calls
                    .lock()
                    .unwrap()
                    .last()
                    .unwrap()
                    .task
                    .as_ref()
                    .unwrap()
                    .digest,
                task["task"]["digest"]
            );
        }
        assert_eq!(by_agent.len(), 3);
        let state = e.store.snapshot().unwrap();
        let original_frozen: Vec<_> = frozen_turns
            .iter()
            .map(|turn| {
                state
                    .governance
                    .iter()
                    .find(|record| record.kind == "execution_task" && record.definition_id == *turn)
                    .unwrap()
                    .clone()
            })
            .collect();
        all_task_sessions.extend(by_agent.values().cloned());
        let reviewer = state
            .sessions
            .iter()
            .find(|session| {
                session.owner_id == tid
                    && state
                        .definitions
                        .iter()
                        .find(|d| d.id == session.agent)
                        .unwrap()
                        .agent_profile
                        .as_ref()
                        .unwrap()
                        .role
                        == AgentRole::Reviewer
            })
            .unwrap();
        let developer = state
            .sessions
            .iter()
            .find(|session| {
                session.owner_id == tid
                    && state
                        .definitions
                        .iter()
                        .find(|d| d.id == session.agent)
                        .unwrap()
                        .agent_profile
                        .as_ref()
                        .unwrap()
                        .role
                        == AgentRole::Developer
            })
            .unwrap();
        let developer_context = e
            .query("query.session", &json!({"id":developer.id}), 4)
            .unwrap()["workflow_context"]
            .clone();
        let mut drift = binding();
        drift.model = "unexpected-model".into();
        assert!(command(&e,"session.open",Some(&tid),json!({"agent":developer.agent,"binding":drift,"workflow_step_instance_id":developer_context["step_instance_id"],"workflow_recipe_node":"implement"})).unwrap_err().message.contains("显式新建"));
        let busy = send(&e, &developer.id, "Pending Task discussion fixture");
        let implementation = state
            .feature_configuration(&f)
            .unwrap()
            .task_plan
            .steps
            .iter()
            .find(|step| step.kind == "attempt.execute")
            .unwrap()
            .key
            .clone();
        assert_eq!(
            open(&e, &tid, &implementation, Some("docs")),
            developer.id,
            "opening the current node while active does not replace the Session"
        );
        assert!(command(&e,"session.open",Some(&tid),json!({"agent":developer.agent,"binding":binding(),"workflow_step_instance_id":developer_context["step_instance_id"],"workflow_recipe_node":"implement"})).unwrap_err().message.contains("活动轮次"));
        let queued = e
            .store
            .snapshot()
            .unwrap()
            .turns
            .iter()
            .find(|turn| Some(turn.id.as_str()) == busy["id"].as_str())
            .unwrap()
            .clone();
        e.execute(
            "conversation.cancel",
            Command {
                command_id: id(),
                target: Some(queued.id),
                expected_revision: Some(queued.revision),
                payload: json!({}),
            },
        )
        .unwrap();
        let context = e
            .query("query.session", &json!({"id":reviewer.id}), 4)
            .unwrap()["workflow_context"]
            .clone();
        let newer = command(&e,"session.open",Some(&tid),json!({"agent":reviewer.agent,"binding":binding(),"workflow_step_instance_id":context["step_instance_id"],"force_new":true})).unwrap()["id"].as_str().unwrap().to_owned();
        assert_ne!(newer, reviewer.id);
        let review = state
            .feature_configuration(&f)
            .unwrap()
            .task_plan
            .steps
            .iter()
            .find(|step| step.kind == "delivery.review")
            .unwrap()
            .key
            .clone();
        assert_eq!(
            open(&e, &tid, &review, None),
            newer,
            "explicit new session is used by earlier execution nodes too"
        );
        let history = e
            .query(
                "query.sessions",
                &json!({"owner_id":tid,"workflow_session_id":reviewer.id}),
                4,
            )
            .unwrap();
        assert_eq!(history["total"], 2);
        assert_eq!(history["sessions"][0]["id"], newer);
        let after = e.store.snapshot().unwrap();
        for record in original_frozen {
            assert_eq!(after.configuration(&record.id).unwrap(), &record);
        }
        assert_eq!(
            after
                .sessions
                .iter()
                .filter(|session| session.owner_id == tid)
                .count(),
            4
        );
    }
    all_task_sessions.sort();
    all_task_sessions.dedup();
    assert_eq!(
        all_task_sessions.len(),
        6,
        "different Tasks must retain different sessions"
    );
    let starts = port.starts.lock().unwrap();
    for start in starts.iter() {
        let calls: Vec<_> = starts
            .iter()
            .filter(|other| other.session.id == start.session.id)
            .collect();
        assert!(
            calls
                .iter()
                .all(|other| other.task_base.as_ref().map(task_content_hash)
                    == start.task_base.as_ref().map(task_content_hash)),
            "the native Agent base stays identical across Task nodes"
        );
        assert!(calls[0].session.external_session.is_none());
        for resumed in calls.iter().skip(1) {
            assert_eq!(
                resumed.session.external_session.as_deref(),
                Some(format!("fixture-{}", start.session.id).as_str())
            );
        }
    }
    assert_eq!(port.calls.lock().unwrap().len(), 10);
}

#[test]
fn frozen_tasks_are_not_rewritable_and_v3_never_falls_back_to_legacy_prompts() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path(), Arc::default());
    let p = project(&e);
    let f = feature(&e, &p);
    let sid = open(&e, &f, "requirement_discussion", None);
    let sent = send(&e, &sid, "Freeze this task");
    assert!(
        e.store
            .transact(None, &mut |s| {
                let r = s
                    .governance
                    .iter_mut()
                    .find(|r| {
                        r.kind == "execution_task"
                            && Some(r.definition_id.as_str()) == sent["id"].as_str()
                    })
                    .unwrap();
                r.data["task"]["content"]["task"]["instructions"] = json!("tampered");
                r.content_hash = configuration_hash(&r.data);
                Ok(Value::Null)
            })
            .is_err()
    );
    let before = e.store.snapshot().unwrap();
    let binding = before
        .governance
        .iter()
        .find(|r| r.kind == "execution_task_binding" && r.definition_id == sid)
        .unwrap();
    assert!(
        e.store
            .transact(None, &mut |s| {
                s.governance.retain(|r| r.id != binding.id);
                Ok(Value::Null)
            })
            .is_err()
    );
    let mut task: FrozenAgentTask = serde_json::from_value(
        e.query("query.execution_task", &json!({"turn_id":sent["id"]}), 4)
            .unwrap()["task"]
            .clone(),
    )
    .unwrap();
    task.renderer_version = "future/v99".into();
    assert_eq!(task.validate().unwrap_err().code, "task_contract_mismatch");
    let mut forged = task.content.clone();
    forged["task"]["instructions"] = json!("Rehashed but not the frozen Workflow task");
    assert!(FrozenAgentTask::new(task.definition.clone(), task.base.clone(), forged).is_err());
    let mut oversized = task.base.clone();
    oversized.agent.body = "x".repeat(TASK_PAYLOAD_LIMIT);
    assert_eq!(
        FrozenAgentTask::new(task.definition.clone(), oversized, task.content.clone())
            .unwrap_err()
            .code,
        "task_input_too_large"
    );
}

#[test]
fn permission_successor_keeps_the_task_and_specs_and_accepts_the_original_input_once() {
    let root = tempfile::tempdir().unwrap();
    let port = Arc::new(Recorder::default());
    let e = engine(root.path(), port.clone());
    let p = project(&e);
    let f = feature(&e, &p);
    let sid = open(&e, &f, "requirement_discussion", None);
    let initial = send(&e, &sid, "Original task");
    e.store
        .transact(None, &mut |s| {
            let r = s
                .governance
                .iter_mut()
                .find(|r| r.kind == "workflow_instance" && r.definition_id == f)
                .unwrap();
            let mut runtime: WorkflowRuntime = r.decode()?;
            let step = runtime
                .steps
                .iter_mut()
                .find(|v| v.key == "requirement_discussion")
                .unwrap();
            step.status = "running".into();
            step.outputs.insert("session".into(), sid.clone());
            r.data = json!(runtime);
            r.content_hash = configuration_hash(&r.data);
            r.revision += 1;
            Ok(Value::Null)
        })
        .unwrap();
    e.tick_conversations(4).unwrap();
    e.tick_conversations(4).unwrap();
    let old = e
        .query("query.execution_task", &json!({"turn_id":initial["id"]}), 4)
        .unwrap();
    let s = e.store.snapshot().unwrap();
    let session = s.sessions.iter().find(|v| v.id == sid).unwrap();
    let mut next = session.runtime_binding.clone().unwrap();
    e.store
        .transact(None, &mut |state| {
            let lock = permission_fixture::prepare(
                state,
                HarnessProvider::Codex,
                PermissionPolicy::full_access(),
            );
            next.permissions = Some(
                devdesk_adapters::permission_profiles::compile_permission_profiles(
                    state,
                    &lock,
                    Some(ExecutionScope {
                        work_type: WorkType::Discussion,
                        writable_paths: vec![],
                        read_only_paths: vec![],
                    }),
                )?,
            );
            Ok(json!(null))
        })
        .unwrap();
    next.configuration_digest = "b".repeat(64);
    let c = Command {
        command_id: id(),
        target: Some(sid.clone()),
        expected_revision: Some(session.revision),
        payload: json!({"client_message_id":"successor-input","input":"Continue the same frozen task"}),
    };
    let sent = e
        .send_with_permission_binding(c.clone(), next.clone())
        .unwrap();
    assert_ne!(sent["effective_session_id"], sid);
    assert_eq!(e.send_with_permission_binding(c, next).unwrap(), sent);
    let runtime = e
        .store
        .snapshot()
        .unwrap()
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.definition_id == f)
        .unwrap()
        .decode::<WorkflowRuntime>()
        .unwrap();
    assert_eq!(
        runtime
            .steps
            .iter()
            .find(|s| s.key == "requirement_discussion")
            .unwrap()
            .outputs["session"],
        sent["effective_session_id"]
    );
    let task = e
        .query("query.execution_task", &json!({"turn_id":sent["id"]}), 4)
        .unwrap();
    assert_eq!(
        task["task"]["definition_digest"],
        old["task"]["definition_digest"]
    );
    assert_eq!(
        task["task"]["content"]["specifications"],
        old["task"]["content"]["specifications"]
    );
    assert_ne!(
        task["task"]["content"]["authorization"],
        old["task"]["content"]["authorization"]
    );
    assert_eq!(
        task["task"]["content"]["source"]["session_id"],
        sent["effective_session_id"]
    );
    e.tick_conversations(4).unwrap();
    e.tick_conversations(4).unwrap();
    assert_eq!(port.calls.lock().unwrap().len(), 2);
    assert_eq!(
        e.query("query.execution_task", &json!({"turn_id":initial["id"]}), 4)
            .unwrap(),
        old
    );
}

#[test]
fn repeated_subworkflow_calls_keep_distinct_origins_and_preview_is_stable() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path(), Arc::default());
    let child=command(&e,"workflow.create",None,json!({"name":"Reusable task","document":{"schema_version":3,"process_kind":"subworkflow","stages":[{"key":"work","name":"Work"}],"steps":[{
        "key":"discuss","name":"Discuss","stage":"work","kind":"agent.interact","contract_version":2,"task_instructions":"Explicit reusable task","role_ref":default_role_id("analyst"),"spec_refs":["builtin-spec-stage-prd-v1"],"interaction":{"output_kind":"requirement","max_turns":8}
    }]}})).unwrap();
    let child = command(&e, "workflow.publish", child["id"].as_str(), json!({})).unwrap();
    let parent = json!({"schema_version":3,"process_kind":"subworkflow","stages":[{"key":"work","name":"Work"}],"steps":[
        {"key":"first","name":"First","stage":"work","kind":"workflow.call","subworkflow_ref":child["id"]},
        {"key":"second","name":"Second","stage":"work","kind":"workflow.call","subworkflow_ref":child["id"],"dependencies":["first"]}]});
    let query = json!({"document":parent,"path":"first/discuss","agent_revision_id":default_configured_agent_id("analyst")});
    let first = e.query("query.workflow_task_preview", &query, 4).unwrap();
    assert_eq!(
        first,
        e.query("query.workflow_task_preview", &query, 4).unwrap()
    );
    assert!(first["base_context"]["agent"]["body"].is_string());
    assert!(
        !first["base_context"]["skills"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let other = e
        .query(
            "query.workflow_task_preview",
            &json!({"document":parent,"path":"second/discuss"}),
            4,
        )
        .unwrap();
    assert_eq!(first["definition"]["workflow_revision_id"], child["id"]);
    assert_eq!(first["task"], other["task"]);
    assert_ne!(first["definition_digest"], other["definition_digest"]);
    let p = project(&e);
    let f = feature(&e, &p);
    let sid = open(&e, &f, "requirement_discussion", None);
    assert!(command(&e,"conversation.send",Some(&sid),json!({"input":"Valid user input","client_message_id":id(),"task":{"instructions":"Fake override"}})).is_err());
    let sent = send(&e, &sid, "Normal input");
    let task = e
        .query("query.execution_task", &json!({"turn_id":sent["id"]}), 4)
        .unwrap();
    assert!(
        !task["task"]["rendered_input"]
            .as_str()
            .unwrap()
            .contains("Fake override")
    );
}

#[test]
#[ignore = "pinned native DSH SDK in Docker; model responses are offline fixtures"]
fn frozen_task_reaches_native_dsh_model_requests_and_resume() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let root = tempfile::tempdir().unwrap();
    let port = Arc::new(Recorder::default());
    let e = engine(root.path(), port.clone());
    let p = project(&e);
    let f = feature(&e, &p);
    let sid = open(&e, &f, "requirement_discussion", None);
    send(&e, &sid, "Offline native protocol fixture");
    e.tick_conversations(4).unwrap();
    let captured = port.calls.lock().unwrap()[0].clone();
    let task = captured.task.as_ref().unwrap();
    let persona =
        json!({"instructions":task.base.agent.body,"skills":task.base.skills}).to_string();
    let mut base_texts = vec![task.base.agent.body.clone()];
    base_texts.extend(task.base.skills.iter().map(|s| s.body.clone()));
    let cases= ["zai-coding-cn", "minimax-cn", "ollama"].map(|provider| {
        let m:ModelDefinition=serde_json::from_value(json!({"provider":provider,"model":"devdesk-mock-model","context_window":32768,"max_output_tokens":4096})).unwrap();
        let h:HarnessDefinition=serde_json::from_value(json!({"provider":"dsh","runtime_version":"0.2.2-rc.1","model_providers":[provider]})).unwrap();
        let c=resolve_model_connection(&m,&h).unwrap().unwrap();
        json!({"provider":provider,"patch":devdesk_adapters::model_providers::dsh_model_patch(&c,"devdesk-mock-model").unwrap(),"endpoint":c.worker_endpoint,"env":c.provider.api_key_env(),"persona":persona,"base_texts":base_texts,"task_payload":devdesk_adapters::harness::provider_turn_text(&captured).unwrap()})
    });
    let image = std::env::var("DEVDESK_DSH_MOCK_IMAGE").expect("set immutable image ID");
    assert!(image.starts_with("sha256:") && image.len() == 71);
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/protocols/dsh-model-providers.mjs")
        .canonicalize()
        .unwrap();
    let mut child = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-i",
            "--network=none",
            "--read-only",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--tmpfs",
            "/tmp:rw,nosuid,size=256m",
            "--mount",
        ])
        .arg(format!(
            "type=bind,source={},target=/probe.mjs,readonly",
            fixture.display()
        ))
        .args(["--entrypoint", "node", &image, "/probe.mjs"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&cases).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["reports"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["frozen_task_delivered"] == true)
    );
    println!("{report}");
}

#[test]
fn formal_run_queues_frozen_task_before_dispatch_and_retry_preserves_business_input() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path(), Arc::default());
    let p = project(&e);
    let f = feature(&e, &p);
    let sid = open(&e, &f, "requirement_execute", Some("requirement_validate"));
    let aid = id();
    let rid = id();
    e.store
        .transact(None, &mut |s| {
            let feature = s.feature(&f)?.clone();
            let session = s.sessions.iter().find(|v| v.id == sid).unwrap().clone();
            let context = s.contexts.iter().find(|v| v.owner_id == f).unwrap().clone();
            let mut binding = feature.binding.clone();
            binding.agent = session.agent.clone();
            binding.skills = s
                .definitions
                .iter()
                .find(|v| v.id == session.agent)
                .unwrap()
                .agent_profile
                .as_ref()
                .unwrap()
                .skill_revisions
                .clone();
            let location = CodeLocation {
                owner: f.clone(),
                branch: "fixture".into(),
                worktree: "fixture-readonly".into(),
            };
            s.attempts.push(Attempt {
                id: aid.clone(),
                feature_id: f.clone(),
                task_id: None,
                phase: "requirement_definition".into(),
                baseline: Some("a".repeat(40)),
                source_base: Some("a".repeat(40)),
                live_sessions: [("requirement_validate".into(), sid.clone())].into(),
                git_context: Some(GitContext {
                    project: p.clone(),
                    workspace: Workspace {
                        repository: "fixture".into(),
                        path: "fixture".into(),
                        mainline: "main".into(),
                    },
                    source: location.clone(),
                    target: location,
                    origin: Origin::External,
                }),
                status: Status::Running,
                input_version: feature.input_version,
                documents: vec![],
                outputs: vec![],
                scope: None,
                binding: binding.clone(),
                context_content: context.content,
                context_revision: context.revision,
                paused: false,
                revision: 1,
            });
            // These are explicit fixture source associations, equivalent to attempt.start.
            let mut source = s
                .governance
                .iter()
                .find(|r| r.kind == "execution_task_binding" && r.definition_id == sid)
                .unwrap()
                .clone();
            source.id = id();
            source.definition_id = aid.clone();
            s.governance.push(source);
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
                cancel_requested: false,
                started: Some(1),
                finished: None,
                result: None,
                result_artifact: None,
                origin: Origin::External,
            });
            Ok(Value::Null)
        })
        .unwrap();
    let pending = e
        .query("query.execution_task", &json!({"run_id":rid}), 4)
        .unwrap();
    assert_eq!(pending["status"], "not_prepared");
    e.tick(4).unwrap();
    let first = e
        .query("query.execution_task", &json!({"run_id":rid}), 4)
        .unwrap();
    assert_eq!(
        first["task"]["content"]["source"]["recipe_node"],
        "requirement_validate"
    );
    assert!(first["task"]["content"]["input"].is_object());
    assert!(
        first["task"]["content"]["task"]["node_instructions"]
            .as_str()
            .unwrap()
            .starts_with('#')
    );
    let retry = id();
    e.store
        .transact(None, &mut |s| {
            let old = s.runs.iter_mut().find(|r| r.id == rid).unwrap();
            old.status = Status::Failed;
            old.finished = Some(2);
            let mut next = old.clone();
            next.id = retry.clone();
            next.status = Status::Running;
            next.retry_of = Some(rid.clone());
            next.finished = None;
            s.runs.push(next);
            let old = s
                .turns
                .iter_mut()
                .find(|t| t.run_id.as_ref() == Some(&rid))
                .unwrap();
            old.status = TurnStatus::Failed;
            old.finished = Some(2);
            old.revision += 1;
            s.attempts
                .iter_mut()
                .find(|a| a.id == aid)
                .unwrap()
                .context_content = "Later context must not change a technical retry".into();
            Ok(Value::Null)
        })
        .unwrap();
    e.tick(4).unwrap();
    let second = e
        .query("query.execution_task", &json!({"run_id":retry}), 4)
        .unwrap();
    assert_eq!(
        second["task"]["definition_digest"],
        first["task"]["definition_digest"]
    );
    assert_eq!(
        second["task"]["content"]["input"]["frozen_context"],
        first["task"]["content"]["input"]["frozen_context"]
    );
    assert_eq!(second["task"]["content"]["retry_of"], rid);
    assert_ne!(second["task"]["digest"], first["task"]["digest"]);
}

#[test]
fn output_contract_describes_typed_evidence_and_rejects_retired_renderers() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path(), Arc::default());
    let p = project(&e);
    let f = feature(&e, &p);
    let sid = open(&e, &f, "requirement_discussion", None);
    let sent = send(&e, &sid, "Contract regression");
    let task: FrozenAgentTask = serde_json::from_value(
        e.query("query.execution_task", &json!({"turn_id":sent["id"]}), 4)
            .unwrap()["task"]
            .clone(),
    )
    .unwrap();
    let mut step = task.definition.decode_step().unwrap();
    step.kind = "attempt.execute".into();
    let output = agent_task_output_contract(&step, Some("requirement_validate"));
    let validator = jsonschema::validator_for(&output["output_schema"]).unwrap();
    let valid = json!({"approve":true,"summary":"verified","checks":[
        {"key":"traceability","passed":true,"evidence":"adopted PRD"},
        {"key":"scope","passed":true,"evidence":"one module"},
        {"key":"tests","passed":true,"evidence":"verifiable acceptance; implementation not required yet"},
        {"key":"failure_paths","passed":true,"evidence":"invalid input TypeError"}]});
    assert!(validator.is_valid(&valid));
    assert_eq!(output["output_schema"], output["schema"]);
    let mut old_valid = valid.clone();
    old_valid.as_object_mut().unwrap().remove("approve");
    assert!(!validator.is_valid(&old_valid));
    let mut wrong = valid.clone();
    wrong["checks"][0] = json!({"name":"traceability","status":"pass","evidence":["PRD"]});
    assert!(!validator.is_valid(&wrong));
    wrong = valid.clone();
    wrong["checks"][0]["key"] = json!("scope");
    assert!(!validator.is_valid(&wrong));
    let mut definition = task.definition.clone();
    definition.renderer_version = "agent-task/v1".into();
    let mut content = task.content.clone();
    content["renderer_version"] = json!("agent-task/v1");
    content["output_contract"]
        .as_object_mut()
        .unwrap()
        .remove("output_schema");
    assert!(FrozenAgentTask::new(definition, task.base, content).is_err());
}

#[test]
fn workflow_sessions_reuse_latest_per_step_agent_and_recipe_until_explicit_new() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path(), Arc::default());
    let p = project(&e);
    let f = feature(&e, &p);
    let first = open(&e, &f, "requirement_discussion", None);
    assert_eq!(open(&e, &f, "requirement_discussion", None), first);
    let source =
        e.query("query.session", &json!({"id":first}), 4).unwrap()["workflow_context"].clone();
    let actor = e
        .store
        .snapshot()
        .unwrap()
        .sessions
        .iter()
        .find(|s| s.id == first)
        .unwrap()
        .agent
        .clone();
    let payload = json!({"agent":actor,"binding":binding(),"workflow_step_instance_id":source["step_instance_id"],"force_new":true});
    let newest = command(&e, "session.open", Some(&f), payload.clone()).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(newest, first);
    assert_eq!(open(&e, &f, "requirement_discussion", None), newest);
    let other_step = open(&e, &f, "requirement_execute", Some("requirement_validate"));
    assert_ne!(other_step, newest);
    assert_eq!(
        open(&e, &f, "requirement_execute", Some("requirement_validate")),
        other_step
    );
    let other_node = open(&e, &f, "solution_execute", Some("test_plan_validate"));
    assert_ne!(other_node, newest);
    let other_feature = feature(&e, &p);
    let unrelated = open(&e, &other_feature, "requirement_discussion", None);
    let rows = e
        .query(
            "query.sessions",
            &json!({"owner_id":f,"workflow_session_id":first}),
            4,
        )
        .unwrap();
    assert_eq!(rows["sessions"][0]["id"], newest);
    assert_eq!(rows["sessions"][1]["id"], first);
    assert_eq!(rows["total"], 2);
    assert!(!rows.to_string().contains(&unrelated));
    let state = e.store.snapshot().unwrap();
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.kind == "execution_specification" && r.definition_id == newest)
            .count(),
        1
    );
    let _sent = send(&e, &newest, "Continue the same workflow step");
    assert!(
        command(&e, "session.open", Some(&f), payload)
            .unwrap_err()
            .message
            .contains("活动轮次")
    );
    assert_eq!(open(&e, &f, "requirement_discussion", None), newest);
}

#[test]
fn delivery_contracts_freeze_revisions_validate_sources_and_pass_real_materials() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path(), Arc::default());
    let p = project(&e);
    let delivery = command(&e,"delivery_type.create",None,json!({"name":"PRD fixture","data":{"format":"markdown","body":"## Goals\n\nDescribe observable acceptance."}})).unwrap();
    schemas::SCHEMAS.assert_valid("delivery-type.schema.json", &delivery["data"]);
    let mut doc = e
        .store
        .snapshot()
        .unwrap()
        .configuration(DEFAULT_FEATURE_WORKFLOW_V3_ID)
        .unwrap()
        .data
        .clone();
    for step in doc["steps"].as_array_mut().unwrap() {
        if step["key"] == "requirement_adopt" {
            step["delivery_outputs"] =
                json!({"prd":{"delivery_ref":delivery["id"],"source_port":"document"}});
        }
        if step["key"] == "design_discussion" {
            step["delivery_inputs"] = json!({"approved_prd":{"delivery_ref":delivery["id"],"source":{"step":"requirement_adopt","port":"prd"}}});
        }
    }
    let parsed: WorkflowDocument = serde_json::from_value(doc.clone()).unwrap();
    validate_workflow(&parsed).unwrap();
    let mut invalid = parsed.clone();
    invalid
        .steps
        .iter_mut()
        .find(|s| s.key == "design_discussion")
        .unwrap()
        .delivery_inputs
        .get_mut("approved_prd")
        .unwrap()
        .delivery_ref = "builtin-delivery-design-v1".into();
    assert!(
        validate_workflow(&invalid)
            .unwrap_err()
            .message
            .contains("不匹配")
    );
    let mut invalid = parsed.clone();
    let consumer = invalid
        .steps
        .iter_mut()
        .find(|s| s.key == "design_discussion")
        .unwrap();
    consumer.dependencies.clear();
    assert!(validate_workflow(&invalid).is_err());
    let mut invalid = parsed.clone();
    invalid
        .steps
        .iter_mut()
        .find(|s| s.key == "requirement_adopt")
        .unwrap()
        .delivery_outputs
        .get_mut("prd")
        .unwrap()
        .source_port = "made_up".into();
    assert!(validate_workflow(&invalid).is_err());
    let draft = command(
        &e,
        "workflow.create",
        None,
        json!({"name":"Delivery fixture","document":doc}),
    )
    .unwrap();
    let published = command(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let defaults = e.configuration_defaults(Some(&p)).unwrap();
    command(&e,"project.configuration.update",Some(&p),json!({"workflow_ref":published["id"],"role_agent_bindings":defaults["role_agent_bindings"]})).unwrap();
    let f = feature(&e, &p);
    e.evaluate_workflows().unwrap();
    for intent in e.pending_workflow_intents().unwrap() {
        if intent.data["method"] == "snapshot.save" {
            let result = e.execute(
                "snapshot.save",
                serde_json::from_value(intent.data["command"].clone()).unwrap(),
            );
            e.observe_workflow_intent(&intent.id, &result).unwrap();
            result.unwrap();
        }
    }
    e.evaluate_workflows().unwrap();
    let revised = command(&e,"delivery_type.revise",delivery["id"].as_str(),json!({"data":{"format":"markdown","body":"Later content must not change an existing Feature."}})).unwrap();
    assert_ne!(revised["id"], delivery["id"]);
    let preview = e
        .query(
            "query.workflow_task_preview",
            &json!({"id":published["id"],"path":"design_discussion"}),
            4,
        )
        .unwrap();
    assert_eq!(preview["delivery_types"][0]["data"], delivery["data"]);
    let frozen = e
        .store
        .snapshot()
        .unwrap()
        .feature_configuration(&f)
        .unwrap()
        .clone();
    assert_eq!(
        frozen
            .definition_hashes
            .get(delivery["id"].as_str().unwrap()),
        Some(&delivery["content_hash"].as_str().unwrap().to_owned())
    );
    let sid = open(&e, &f, "requirement_discussion", None);
    let sent = send(&e, &sid, "Define the PRD fixture");
    e.tick_conversations(4).unwrap();
    e.tick_conversations(4).unwrap();
    let proposal = command(&e,"proposal.create",None,json!({"turn_id":sent["id"],"target":f,"content":{"kind":"requirement","title":"Actual PRD","body":"A real adopted source document.","acceptance":[{"key":"A1","description":"Observable result"}]}})).unwrap();
    e.evaluate_workflows().unwrap();
    command(&e, "proposal.adopt", proposal["id"].as_str(), json!({})).unwrap();
    e.evaluate_workflows().unwrap();
    let state = e.store.snapshot().unwrap();
    let runtime: WorkflowRuntime = state
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.definition_id == f)
        .unwrap()
        .decode()
        .unwrap();
    let produced = &runtime
        .steps
        .iter()
        .find(|s| s.key == "requirement_adopt")
        .unwrap()
        .outputs;
    assert_eq!(produced["prd"], produced["document"]);
    assert!(state.documents.iter().any(|d| d.id == produced["prd"]));
    let sid = open(&e, &f, "design_discussion", None);
    let turn = send(&e, &sid, "Use the accepted PRD");
    let task = e
        .query("query.execution_task", &json!({"turn_id":turn["id"]}), 4)
        .unwrap();
    schemas::SCHEMAS.assert_valid("agent-task.schema.json", &task["task"]);
    assert_eq!(
        task["task"]["content"]["delivery_types"][0]["data"],
        delivery["data"]
    );
    assert!(
        task["task"]["content"]["inputs"]
            .to_string()
            .contains("A real adopted source document.")
    );
}
