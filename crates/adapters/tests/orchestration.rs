//! Explicit Harness control through production commands and SQLite.
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
use devdesk_adapters::integrations::NativeTools;
use devdesk_adapters::{FileArtifacts, SqliteStore};
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
use std::sync::Arc;

fn engine(path: &std::path::Path) -> Engine {
    let e = Engine::new(
        Arc::new(SqliteStore::open(path).unwrap()),
        Arc::new(FileArtifacts::new(path.into())),
        runtime::adapters(),
    )
    .with_tool_executor(Arc::new(NativeTools::new(path).unwrap()));
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    e
}
fn cmd(e: &Engine, method: &str, target: Option<&str>, mut p: Value) -> Result<Value> {
    let s = e.store.snapshot()?;
    let r = target.and_then(|id| {
        s.configuration(id)
            .ok()
            .map(|r| r.revision)
            .or_else(|| s.projects.iter().find(|r| r.id == id).map(|r| r.revision))
    });
    if let Some(row) = target
        .and_then(|id| s.configuration(id).ok())
        .filter(|r| r.kind == "workflow_instance")
    {
        p["plan_revision_id"] = row.data["active_plan_revision_id"].clone();
    }
    e.execute(
        method,
        Command {
            command_id: id(),
            target: target.map(str::to_owned),
            expected_revision: r,
            payload: p,
        },
    )
}
fn start(e: &Engine, doc: Value) -> String {
    let project = cmd(
        e,
        "project.create",
        None,
        json!({"title":"编排合同","workspace":"/tmp/fixture"}),
    )
    .unwrap();
    let draft = cmd(
        e,
        "workflow.create",
        None,
        json!({"name":"可控制流程","document":doc}),
    )
    .unwrap();
    let workflow = cmd(e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    cmd(
        e,
        "workflow.start",
        project["id"].as_str(),
            json!({"workflow_ref":workflow["id"],"role_agent_bindings":e.configuration_defaults(Some(project["id"].as_str().unwrap())).unwrap()["role_agent_bindings"]}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}
fn document() -> Value {
    json!({"schema_version":5,"stages":[{"key":"work","name":"工作"}],"steps":[{"key":"one","name":"第一步","stage":"work","executor":{"type":"control"}},{"key":"two","name":"第二步","stage":"work","dependencies":["one"],"executor":{"type":"control"}}]})
}
fn runtime(e: &Engine, id: &str) -> Value {
    e.query("workflow.inspect", &json!({"id":id}), 4).unwrap()
}
fn accept(e: &Engine, id: &str, key: &str) {
    let v = runtime(e, id);
    let s = v["runtime"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["step"] == key)
        .unwrap();
    cmd(e,"workflow.step.accept",Some(id),json!({"step":key,"execution_id":s["id"],"input_digest":s["input_digest"],"outputs":s["outputs"],"reason":"检查实际结果"})).unwrap();
}

#[test]
fn observations_never_dispatch_or_accept_without_an_explicit_command() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let id = start(&e, document());
    for _ in 0..3 {
        e.evaluate_workflows().unwrap();
    }
    assert_eq!(runtime(&e, &id)["runtime"]["steps"][0]["status"], "queued");
    assert!(
        cmd(
            &e,
            "workflow.step.dispatch",
            Some(&id),
            json!({"step":"two"})
        )
        .is_err()
    );
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&id),
        json!({"step":"one"}),
    )
    .unwrap();
    e.evaluate_workflows().unwrap();
    assert_eq!(
        runtime(&e, &id)["runtime"]["steps"][0]["status"],
        "awaiting_acceptance"
    );
    accept(&e, &id, "one");
    e.evaluate_workflows().unwrap();
    assert_eq!(runtime(&e, &id)["runtime"]["steps"][1]["status"], "queued");
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&id),
        json!({"step":"two"}),
    )
    .unwrap();
    accept(&e, &id, "two");
    e.evaluate_workflows().unwrap();
    assert_ne!(runtime(&e, &id)["runtime"]["status"], "completed");
    cmd(
        &e,
        "workflow.complete",
        Some(&id),
        json!({"reason":"全部交付已核对"}),
    )
    .unwrap();
    assert_eq!(runtime(&e, &id)["runtime"]["status"], "completed");
}

#[test]
fn runtime_change_preserves_an_accepted_contract_and_rejects_stale_preview() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let id = start(&e, document());
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&id),
        json!({"step":"one"}),
    )
    .unwrap();
    accept(&e, &id, "one");
    let before = runtime(&e, &id);
    let old = before["runtime"]["steps"][0].clone();
    let mut doc = before["runtime"]["plan"]["document"].clone();
    doc["steps"][1]["name"] = json!("调整后的工作");
    let changes = json!({"plan_revision_id":before["runtime"]["active_plan_revision_id"],"reason":"调整后续任务","operations":[{"type":"replace_plan","document":doc}]});
    let preview = e
        .query(
            "workflow.change.preview",
            &json!({"id":id,"changeset":changes}),
            4,
        )
        .unwrap();
    assert_eq!(before, runtime(&e, &id));
    cmd(
        &e,
        "workflow.change.apply",
        Some(&id),
        json!({"changeset":changes,"preview_hash":preview["preview_hash"]}),
    )
    .unwrap();
    let after = runtime(&e, &id);
    assert_eq!(after["runtime"]["steps"][0], old);
    assert_ne!(
        after["runtime"]["active_plan_revision_id"],
        before["runtime"]["active_plan_revision_id"]
    );
    assert!(
        cmd(
            &e,
            "workflow.change.apply",
            Some(&id),
            json!({"changeset":changes,"preview_hash":preview["preview_hash"]})
        )
        .is_err()
    );
    drop(e);
    let e = engine(d.path());
    assert_eq!(runtime(&e, &id)["runtime"]["steps"][0], old);
}

#[test]
fn inserting_a_node_and_rewiring_dependencies_preserves_the_accepted_predecessor() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let root = start(&e, document());
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&root),
        json!({"step":"one"}),
    )
    .unwrap();
    accept(&e, &root, "one");
    let before = runtime(&e, &root);
    let mut second = before["runtime"]["plan"]["document"]["steps"][1].clone();
    second["dependencies"] = json!(["inserted"]);
    let changes = json!({"plan_revision_id":before["runtime"]["active_plan_revision_id"],"reason":"新增核对并改变后续依赖","operations":[{"type":"upsert_step","step":{"key":"inserted","name":"新增核对","stage":"work","dependencies":["one"],"executor":{"type":"control"}}},{"type":"upsert_step","step":second}]});
    let preview = e
        .query(
            "workflow.change.preview",
            &json!({"id":root,"changeset":changes}),
            1,
        )
        .unwrap();
    cmd(
        &e,
        "workflow.change.apply",
        Some(&root),
        json!({"changeset":changes,"preview_hash":preview["preview_hash"]}),
    )
    .unwrap();
    let after = runtime(&e, &root);
    assert_eq!(after["runtime"]["steps"][0], before["runtime"]["steps"][0]);
    assert!(
        cmd(
            &e,
            "workflow.step.dispatch",
            Some(&root),
            json!({"step":"two"})
        )
        .is_err()
    );
    for key in ["inserted", "two"] {
        cmd(
            &e,
            "workflow.step.dispatch",
            Some(&root),
            json!({"step":key}),
        )
        .unwrap();
        accept(&e, &root, key);
    }
    cmd(
        &e,
        "workflow.complete",
        Some(&root),
        json!({"reason":"按修订后的顺序完成"}),
    )
    .unwrap();
    assert_eq!(runtime(&e, &root)["runtime"]["status"], "completed");
}

#[test]
fn bypass_keeps_a_distinct_decision_and_cannot_hide_a_missing_final_output() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let id = start(&e, document());
    let before = runtime(&e, &id);
    let changes = json!({"plan_revision_id":before["runtime"]["active_plan_revision_id"],"reason":"用户要求跳过此阶段","operations":[{"type":"bypass_stage","stage":"work"}]});
    let preview = e
        .query(
            "workflow.change.preview",
            &json!({"id":id,"changeset":changes}),
            4,
        )
        .unwrap();
    cmd(
        &e,
        "workflow.change.apply",
        Some(&id),
        json!({"changeset":changes,"preview_hash":preview["preview_hash"]}),
    )
    .unwrap();
    let after = runtime(&e, &id);
    assert!(
        after["runtime"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["status"] == "skipped")
    );
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.kind == "workflow_decision" && r.data["operation"] == "change")
    );
}

#[test]
fn a_plan_cannot_be_mutated_by_direct_storage_writes() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let id = start(&e, document());
    assert!(
        e.store
            .transact(None, &mut |state| {
                let r = state.governance.iter_mut().find(|r| r.id == id).unwrap();
                r.data["plan"]["document"]["steps"][0]["name"] = json!("篡改");
                r.content_hash = configuration_hash(&r.data);
                Ok(Value::Null)
            })
            .is_err()
    );
}

#[test]
fn published_steps_cannot_share_a_stable_node_identity() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let mut doc = document();
    doc["steps"][0]["node_id"] = json!("same-node");
    doc["steps"][1]["node_id"] = json!("same-node");
    let draft = cmd(
        &e,
        "workflow.create",
        None,
        json!({"name":"拒绝重复节点","document":doc}),
    )
    .unwrap();
    assert!(cmd(&e, "workflow.publish", draft["id"].as_str(), json!({})).is_err());
}

#[test]
fn bypass_requires_typed_replacement_and_preserves_the_imported_content() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let doc = json!({"schema_version":5,"stages":[{"key":"design","name":"设计"},{"key":"delivery","name":"交付"}],"steps":[
        {"key":"design","name":"编写设计","stage":"design","executor":{"type":"agent","role_ref":"builtin-role-architect"},"task_instructions":"分析设计","outputs":{"result":{"delivery_ref":"builtin-delivery-design-v1"}}},
        {"key":"consume","name":"采用设计","stage":"delivery","executor":{"type":"control"},"dependencies":["design"],"inputs":{"design":{"type":"source","source":{"step":"design","port":"result"}}}}]});
    let id = start(&e, doc);
    let initial = runtime(&e, &id);
    let plan = initial["runtime"]["active_plan_revision_id"].clone();
    let missing = json!({"plan_revision_id":plan,"reason":"已有设计","operations":[{"type":"bypass_stage","stage":"design"}]});
    assert!(
        e.query(
            "workflow.change.preview",
            &json!({"id":id,"changeset":missing}),
            4
        )
        .unwrap_err()
        .message
        .contains("替代输入")
    );
    let material=cmd(&e,"workflow.materials.import",Some(&id),json!({"delivery_ref":"builtin-delivery-design-v1","value":"用户在线下完成的接口和失败处理设计","source":"用户提供的设计"})).unwrap();
    let changes = json!({"plan_revision_id":plan,"reason":"采用用户设计并绕过重复编写","operations":[{"type":"bypass_stage","stage":"design","substitutions":{"design.result":{"type":"material","material_ref":material["id"]}}}]});
    let preview = e
        .query(
            "workflow.change.preview",
            &json!({"id":id,"changeset":changes}),
            4,
        )
        .unwrap();
    cmd(
        &e,
        "workflow.change.apply",
        Some(&id),
        json!({"changeset":changes,"preview_hash":preview["preview_hash"]}),
    )
    .unwrap();
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&id),
        json!({"step":"consume"}),
    )
    .unwrap();
    let after = runtime(&e, &id);
    assert_eq!(after["runtime"]["steps"][0]["status"], "skipped");
    assert_eq!(
        after["runtime"]["steps"][1]["inputs"]["design"],
        material["material"]["value"]
    );
    assert!(
        after["runtime"]["steps"][0]["outputs"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert!(after["runtime"]["steps"][0]["session_id"].is_null());
}

#[test]
fn bypass_cannot_supply_a_missing_final_output_or_change_a_different_root() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let doc = json!({"schema_version":5,"stages":[{"key":"design","name":"设计"}],"steps":[{"key":"design","name":"编写设计","stage":"design","executor":{"type":"agent","role_ref":"builtin-role-architect"},"task_instructions":"设计接口","outputs":{"result":{"delivery_ref":"builtin-delivery-design-v1"}}}],"outputs":{"result":{"step":"design","port":"result"}}});
    let id = start(&e, doc);
    let view = runtime(&e, &id);
    let changes = json!({"plan_revision_id":view["runtime"]["active_plan_revision_id"],"reason":"绕过","operations":[{"type":"bypass_stage","stage":"design"}]});
    assert!(
        e.query(
            "workflow.change.preview",
            &json!({"id":id,"changeset":changes}),
            4
        )
        .is_err()
    );
    let wrong_plan = json!({"plan_revision_id":"another-root-plan","reason":"跨根修改","operations":[{"type":"bypass_stage","stage":"design"}]});
    assert!(
        e.query(
            "workflow.change.preview",
            &json!({"id":id,"changeset":wrong_plan}),
            4
        )
        .is_err()
    );
    assert_eq!(runtime(&e, &id), view);
}

#[test]
fn bounded_segment_requires_explicit_authorization_and_stops_at_plan_change() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let id = start(&e, document());
    cmd(
        &e,
        "workflow.segment.dispatch",
        Some(&id),
        json!({"steps":["one","two"],"reason":"委托这两个无副作用步骤"}),
    )
    .unwrap();
    for _ in 0..5 {
        e.tick(1).unwrap();
    }
    let view = runtime(&e, &id);
    assert!(
        view["runtime"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["status"] == "completed")
    );
    assert_ne!(view["runtime"]["status"], "completed");
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.kind == "workflow_segment" && r.data["status"] == "completed")
    );
}

#[derive(Default)]
struct NativeManager {
    segment: bool,
    pending:
        std::sync::Mutex<std::collections::HashMap<String, Vec<ports::ConversationObservation>>>,
    turns: std::sync::Mutex<std::collections::HashMap<String, String>>,
    replies: std::sync::Mutex<Vec<ToolReceipt>>,
}
impl ports::Harness for NativeManager {
    fn identity(&self) -> String {
        "fixture:workflow-controller".into()
    }
    fn open_session(&self, r: &ports::SessionStart) -> Result<ports::SessionReceipt> {
        assert!(r.configured_task.is_none());
        assert!(
            r.code_view.is_none(),
            "a controller must not mount the owner's code workspace"
        );
        assert_eq!(
            r.session.runtime_binding.as_ref().unwrap().work_type,
            WorkType::Orchestration
        );
        Ok(ports::SessionReceipt {
            external_session: format!("native-{}", r.session.id),
            container_id: "fixture-container".into(),
            image_digest: r.execution.image_digest.clone(),
            operation_id: r.execution.operation_id.clone(),
        })
    }
    fn dispatch_turn(&self, r: &ports::TurnDispatch) -> Result<()> {
        assert!(r.turn.code_view.is_none());
        use ports::ConversationObservation as O;
        let input: Value = serde_json::from_str(&r.turn.input).unwrap();
        assert_eq!(input["renderer_version"], "orchestration-task/v1");
        let instance = &input["instances"][0];
        let root = instance["id"].clone();
        let plan = instance["plan"].clone();
        let execution = instance["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["status"] != "completed");
        let (method, args) = if self.segment {
            (
                "workflow_segment_dispatch",
                json!({"instance_id":root,"plan_revision_id":plan,"steps":["one","two"],"reason":"明确委托两个自动节点"}),
            )
        } else if let Some(e) = execution {
            if e["status"] == "queued" {
                (
                    "workflow_step_dispatch",
                    json!({"instance_id":root,"plan_revision_id":plan,"step":e["step"]}),
                )
            } else {
                (
                    "workflow_step_accept",
                    json!({"instance_id":root,"plan_revision_id":plan,"step":e["step"],"execution_id":e["id"],"input_digest":e["input_digest"],"outputs":e["outputs"]}),
                )
            }
        } else {
            (
                "workflow_complete",
                json!({"instance_id":root,"plan_revision_id":plan}),
            )
        };
        self.turns
            .lock()
            .unwrap()
            .insert(r.session.id.clone(), r.turn.id.clone());
        self.pending.lock().unwrap().insert(
            r.session.id.clone(),
            vec![
                O::Admitted {
                    turn: r.turn.id.clone(),
                    message: Some("manager-message".into()),
                    external_turn: Some(format!("native-turn-{}", r.turn.id)),
                },
                O::Consumed {
                    turn: r.turn.id.clone(),
                    message: Some("manager-message".into()),
                },
                O::ToolCall {
                    turn: r.turn.id.clone(),
                    request: json!(17),
                    thread: r.session.external_session.clone().unwrap(),
                    external_turn: format!("native-turn-{}", r.turn.id),
                    call: format!("call-{}", r.turn.id),
                    tool: format!("devdesk_{method}"),
                    arguments: args,
                    worktree: String::new(),
                },
            ],
        );
        Ok(())
    }
    fn poll_session(&self, s: &Session) -> Result<Vec<ports::ConversationObservation>> {
        Ok(self
            .pending
            .lock()
            .unwrap()
            .remove(&s.id)
            .unwrap_or_default())
    }
    fn answer_tool_call(&self, s: &Session, _: &Value, r: &ToolReceipt) -> Result<()> {
        assert!(
            r.structured_content.is_none() || r.content.is_empty(),
            "native controller results must be sent once"
        );
        self.replies.lock().unwrap().push(r.clone());
        let turn = self.turns.lock().unwrap()[&s.id].clone();
        self.pending.lock().unwrap().insert(
            s.id.clone(),
            vec![ports::ConversationObservation::Terminal {
                turn,
                outcome: ports::ProviderTerminal::Completed,
                reason: "native manager tool round".into(),
                quiescent: true,
            }],
        );
        Ok(())
    }
    fn stop_session(&self, _: &Session, e: &RuntimeExecution) -> Result<ports::WorkerStop> {
        Ok(ports::WorkerStop {
            operation_id: e.operation_id.clone(),
            writer_revoked: true,
            mode: "fixture".into(),
            requires_successor: false,
        })
    }
    fn cleanup_worker(&self, _: &Session, _: &RuntimeExecution) -> Result<()> {
        Ok(())
    }
}

fn drive_manager(e: &Engine) {
    drive_manager_with_send(e, true)
}
fn drive_manager_with_send(e: &Engine, allow_send: bool) {
    drive_manager_cycle(e, allow_send, true)
}
fn drive_manager_cycle(e: &Engine, allow_send: bool, full_tick: bool) {
    e.drive_controllers().unwrap();
    for intent in e.pending_workflow_intents().unwrap() {
        let method = intent.data["method"].as_str().unwrap();
        if method == "conversation.send" && !allow_send {
            continue;
        }
        let mut c: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
        if method == "session.open" {
            let configured = e.configured_agent_request(&c).unwrap().unwrap();
            let s = e.store.snapshot().unwrap();
            let record = s
                .configuration(c.payload["agent"].as_str().unwrap())
                .unwrap();
            let a: AgentComposition = record.decode().unwrap();
            let mut binding:RuntimeBinding=serde_json::from_value(json!({"installation":{"id":"fixture-runtime","key":{"provider":"codex","version":"fixture","os":"linux","arch":"aarch64"},"artifact":{"kind":"docker_image","image_id":format!("sha256:{}","a".repeat(64))},"source":"explicit controller fixture","license":"test"},"adapter_version":"fixture/v1","model":"fixture-model","tool_policy":"read_only","work_type":"orchestration","configuration_digest":"a".repeat(64)})).unwrap();
            binding.permissions = Some(
                devdesk_adapters::permission_profiles::compile_permission_profiles(
                    &s,
                    &configured.permissions,
                    Some(ExecutionScope {
                        work_type: WorkType::Orchestration,
                        writable_paths: vec![],
                        read_only_paths: vec![],
                    }),
                )
                .unwrap(),
            );
            binding.agent_configuration = Some(AgentExecutionConfiguration {
                integration_lock: Some(configured.integration_lock),
                agent_instance_id: "controller-fixture-identity".into(),
                agent_revision_id: record.id.clone(),
                composition_hash: record.content_hash.clone(),
                model_revision_id: a.model_ref,
                harness_revision_id: a.harness_ref,
                model_provider: "openai".into(),
                model_parameters: Default::default(),
                tool_revisions: a.tools,
            });
            c.payload["binding"] = json!(binding);
        }
        if method == "conversation.send" && intent.data["controller"] == true {
            e.store
                .transact(None, &mut |s| {
                    let session = s
                        .sessions
                        .iter_mut()
                        .find(|s| Some(s.id.as_str()) == c.target.as_deref())
                        .unwrap();
                    session.revision += 1;
                    Ok(Value::Null)
                })
                .unwrap();
        }
        let result = e.execute(method, c);
        e.observe_workflow_intent(&intent.id, &result).unwrap();
        result.unwrap();
    }
    if full_tick {
        e.tick(1).unwrap();
    } else {
        e.tick_conversations(1).unwrap();
    }
}

#[test]
fn a_controller_does_not_inherit_a_live_owners_code_view() {
    let directory = tempfile::tempdir().unwrap();
    let mut e = engine(directory.path());
    e.harness = Arc::new(NativeManager::default());
    let root = start(&e, document());
    let project = runtime(&e, &root)["runtime"]["project_id"]
        .as_str()
        .unwrap()
        .to_owned();
    e.store
        .transact(None, &mut |state| {
            let path = directory.path().to_string_lossy().to_string();
            let base = "a".repeat(40);
            let p = state.projects.iter_mut().find(|p| p.id == project).unwrap();
            p.execution_mode = "live".into();
            p.workspace = path.clone();
            p.workspace_definition = Some(Workspace {
                repository: "fixture/local".into(),
                path: path.clone(),
                mainline: "main".into(),
            });
            let operation = id();
            let worktree = RepositoryWorktree {
                id: id(),
                project_id: project.clone(),
                owner_id: project.clone(),
                kind: WorktreeKind::Mainline,
                branch: "main".into(),
                path,
                base: base.clone(),
                operation_id: operation.clone(),
            };
            state.repository_operations.push(RepositoryOperation {
                id: operation,
                project_id: project.clone(),
                action: RepositoryAction::CreateWorktree {
                    owner: project.clone(),
                    worktree_kind: WorktreeKind::Mainline,
                    base: base.clone(),
                },
                status: RepositoryOperationStatus::Completed,
                receipt: Some(RepositoryReceipt::Worktree {
                    worktree: worktree.clone(),
                }),
                reason: "Explicit Git-context fixture".into(),
                revision: 1,
            });
            state.repository_worktrees.push(worktree);
            state.heads.push(GitHead {
                id: format!("main:{project}"),
                project_id: project.clone(),
                revision: base,
            });
            Ok(Value::Null)
        })
        .unwrap();
    let controller = cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager","agent_ref":"builtin-agent-sol-high-project-manager-v3","policy":WorkflowControlPolicy::default()})).unwrap();
    for _ in 0..50 {
        drive_manager(&e);
        if runtime(&e, &root)["runtime"]["status"] == "completed" {
            break;
        }
    }
    let state = e.store.snapshot().unwrap();
    let sid = state
        .configuration(controller["id"].as_str().unwrap())
        .unwrap()
        .data["session_id"]
        .as_str()
        .unwrap();
    let turns: Vec<_> = state
        .turns
        .iter()
        .filter(|turn| turn.session_id == sid)
        .collect();
    assert!(!turns.is_empty() && turns.iter().all(|turn| turn.code_view.is_none()));
    assert_eq!(runtime(&e, &root)["runtime"]["status"], "completed");
}

#[test]
fn a_native_manager_controls_without_a_running_step_and_persists_its_real_call_chain() {
    let d = tempfile::tempdir().unwrap();
    let mut e = engine(d.path());
    runtime::bind_default_permissions(&e);
    let harness = Arc::new(NativeManager::default());
    e.harness = harness.clone();
    let id = start(&e, document());
    cmd(&e,"orchestration.open",Some(&id),json!({"role_ref":"builtin-role-project-manager","agent_ref":"builtin-agent-sol-high-project-manager-v3","policy":WorkflowControlPolicy::default()})).unwrap();
    for _ in 0..60 {
        drive_manager(&e);
        if runtime(&e, &id)["runtime"]["status"] == "completed" {
            break;
        }
    }
    drive_manager(&e);
    let s = e.store.snapshot().unwrap();
    assert_eq!(runtime(&e, &id)["runtime"]["status"], "completed");
    assert_eq!(harness.replies.lock().unwrap().len(), 5);
    assert!(harness.replies.lock().unwrap().iter().all(|r| r.success));
    assert!(!s.governance.iter().any(|r| r.kind == "configured_session"));
    assert!(
        s.orchestration_wakeups
            .iter()
            .any(|w| w.status == "consumed")
    );
    assert_eq!(
        s.governance
            .iter()
            .filter(|r| r.kind == "controller_tool_reply")
            .count(),
        5
    );
    assert_eq!(
        s.governance
            .iter()
            .filter(|r| r.kind == "workflow_decision" && r.data["actor"]["kind"] == "controller")
            .count(),
        5
    );
}

#[test]
fn native_control_survives_restart_after_action_and_before_reply_without_replay() {
    let directory = tempfile::tempdir().unwrap();
    let harness = Arc::new(NativeManager::default());
    let mut e = engine(directory.path());
    e.harness = harness.clone();
    let root = start(&e, document());
    cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager","agent_ref":"builtin-agent-sol-high-project-manager-v3","policy":WorkflowControlPolicy::default()})).unwrap();
    for _ in 0..20 {
        drive_manager_cycle(&e, true, false);
        if e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.kind == "controller_tool_call" && r.data["status"] == "prepared")
        {
            break;
        }
    }
    let call = e
        .store
        .snapshot()
        .unwrap()
        .governance
        .into_iter()
        .find(|r| r.kind == "controller_tool_call")
        .unwrap();
    assert_eq!(call.data["status"], "prepared");
    assert!(
        e.store
            .transact(None, &mut |s| {
                let r = s.governance.iter_mut().find(|r| r.id == call.id).unwrap();
                r.data["epoch"] = json!(99);
                Ok(Value::Null)
            })
            .is_err()
    );
    e.drive_controllers().unwrap();
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .configuration(&call.id)
            .unwrap()
            .data["status"],
        "succeeded"
    );
    assert!(harness.replies.lock().unwrap().is_empty());
    drop(e);
    let mut e = engine(directory.path());
    e.harness = harness.clone();
    for _ in 0..3 {
        e.drive_controllers().unwrap();
    }
    let state = e.store.snapshot().unwrap();
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][0]["status"],
        "awaiting_acceptance"
    );
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][1]["status"],
        "queued"
    );
    assert!(harness.replies.lock().unwrap().len() <= 1);
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.kind == "controller_tool_reply" && r.definition_id == call.id)
            .count(),
        1
    );
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.kind == "workflow_decision" && r.data["operation"] == "dispatch")
            .count(),
        1
    );
}

#[test]
fn human_takeover_revokes_an_already_authorized_native_segment() {
    let directory = tempfile::tempdir().unwrap();
    let mut e = engine(directory.path());
    e.harness = Arc::new(NativeManager {
        segment: true,
        ..Default::default()
    });
    let root = start(&e, document());
    let opened=cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager","agent_ref":"builtin-agent-sol-high-project-manager-v3","policy":WorkflowControlPolicy::default()})).unwrap();
    for _ in 0..20 {
        drive_manager_cycle(&e, true, false);
        if e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.kind == "controller_tool_call" && r.data["status"] == "prepared")
        {
            break;
        }
    }
    e.drive_controllers().unwrap();
    let calls = e
        .store
        .snapshot()
        .unwrap()
        .governance
        .into_iter()
        .filter(|r| r.kind == "controller_tool_call")
        .collect::<Vec<_>>();
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.kind == "workflow_segment" && r.data["status"] == "active"),
        "{calls:?}"
    );
    cmd(
        &e,
        "orchestration.takeover",
        opened["id"].as_str(),
        json!({"epoch":1}),
    )
    .unwrap();
    e.drive_workflow_segments().unwrap();
    let state = e.store.snapshot().unwrap();
    assert!(
        state
            .governance
            .iter()
            .any(|r| r.kind == "workflow_segment" && r.data["status"] == "stopped")
    );
    assert!(
        runtime(&e, &root)["runtime"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["status"] == "queued")
    );
}

#[test]
fn revising_one_child_preserves_its_sibling_and_parent_output_contract() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let draft = cmd(
        &e,
        "workflow.create",
        None,
        json!({"name":"兄弟子流程","document":document()}),
    )
    .unwrap();
    let child = cmd(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let root = start(
        &e,
        json!({"schema_version":5,"stages":[{"key":"calls","name":"两次调用"}],"steps":[{"key":"left","name":"左调用","stage":"calls","executor":{"type":"workflow","workflow_ref":child["id"]}},{"key":"right","name":"右调用","stage":"calls","executor":{"type":"workflow","workflow_ref":child["id"]}}]}),
    );
    for key in ["left", "right"] {
        cmd(
            &e,
            "workflow.step.dispatch",
            Some(&root),
            json!({"step":key}),
        )
        .unwrap();
    }
    let s = e.store.snapshot().unwrap();
    let parent: ConfiguredWorkflowInstance = s.configuration(&root).unwrap().decode().unwrap();
    let find = |key: &str| {
        s.governance
            .iter()
            .find(|r| {
                r.kind == "workflow_instance"
                    && r.data["parent_execution_id"]
                        == parent.steps.iter().find(|e| e.step == key).unwrap().id
            })
            .unwrap()
            .id
            .clone()
    };
    let left = find("left");
    let right = find("right");
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&left),
        json!({"step":"one"}),
    )
    .unwrap();
    accept(&e, &left, "one");
    for key in ["one", "two"] {
        cmd(
            &e,
            "workflow.step.dispatch",
            Some(&right),
            json!({"step":key}),
        )
        .unwrap();
        accept(&e, &right, key);
    }
    cmd(
        &e,
        "workflow.complete",
        Some(&right),
        json!({"reason":"右调用完成"}),
    )
    .unwrap();
    e.evaluate_workflows().unwrap();
    accept(&e, &root, "right");
    let before = runtime(&e, &left);
    let sibling = runtime(&e, &right);
    let preserved = before["runtime"]["steps"][0].clone();
    let mut step = before["runtime"]["plan"]["document"]["steps"][1].clone();
    step["name"] = json!("左调用新的交接要求");
    let changes = json!({"plan_revision_id":before["runtime"]["active_plan_revision_id"],"reason":"只改变左调用","operations":[{"type":"upsert_step","step":step}]});
    let preview = e
        .query(
            "workflow.change.preview",
            &json!({"id":left,"changeset":changes}),
            4,
        )
        .unwrap();
    cmd(
        &e,
        "workflow.change.apply",
        Some(&left),
        json!({"changeset":changes,"preview_hash":preview["preview_hash"]}),
    )
    .unwrap();
    assert_eq!(runtime(&e, &right)["runtime"], sibling["runtime"]);
    assert_eq!(runtime(&e, &left)["runtime"]["steps"][0], preserved);
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.kind == "workflow_child_override" && r.data["child_instance_id"] == left)
    );
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&left),
        json!({"step":"two"}),
    )
    .unwrap();
    accept(&e, &left, "two");
    cmd(
        &e,
        "workflow.complete",
        Some(&left),
        json!({"reason":"左调用新计划完成"}),
    )
    .unwrap();
    e.evaluate_workflows().unwrap();
    accept(&e, &root, "left");
    cmd(
        &e,
        "workflow.complete",
        Some(&root),
        json!({"reason":"两次确切调用都完成"}),
    )
    .unwrap();
    assert_eq!(runtime(&e, &root)["runtime"]["status"], "completed");
}

#[test]
fn delegated_segment_stops_on_a_rejected_condition_and_keeps_the_report() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let mut doc = document();
    doc["steps"][0]["completion"] = json!({"op":"equals","source":{"step":"$input","port":"assessment","pointer":"/approve"},"value":true});
    doc["inputs"] =
        json!({"assessment":{"delivery_ref":"builtin-delivery-object-v1","required":false}});
    let root = start(&e, doc);
    cmd(
        &e,
        "workflow.segment.dispatch",
        Some(&root),
        json!({"steps":["one","two"],"reason":"只委托当前两个节点"}),
    )
    .unwrap();
    for _ in 0..5 {
        e.tick(1).unwrap();
    }
    let view = runtime(&e, &root);
    assert_eq!(view["runtime"]["steps"][0]["status"], "awaiting_acceptance");
    assert_eq!(view["runtime"]["steps"][1]["status"], "queued");
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.kind == "workflow_segment"
                && r.data["status"] == "stopped"
                && r.data["reason"].as_str().unwrap_or("").contains("完成条件"))
    );
}

#[test]
fn a_parent_does_not_spend_execution_budget_while_children_wait_for_controller_commands() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let draft = cmd(
        &e,
        "workflow.create",
        None,
        json!({"name":"子任务预算","document":document()}),
    )
    .unwrap();
    let child = cmd(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let root = start(
        &e,
        json!({"schema_version":5,"stages":[{"key":"work","name":"父流程"}],"steps":[{"key":"call","name":"显式子调用","stage":"work","executor":{"type":"workflow","workflow_ref":child["id"]}}]}),
    );
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&root),
        json!({"step":"call"}),
    )
    .unwrap();
    e.evaluate_workflows().unwrap();
    assert!(runtime(&e, &root)["runtime"]["steps"][0]["budget_wait_started"].is_number());
    let state = e.store.snapshot().unwrap();
    let invocation = state
        .governance
        .iter()
        .find(|r| {
            r.kind == "workflow_instance" && r.id != root && r.data["root_instance_id"] == root
        })
        .unwrap();
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&invocation.id),
        json!({"step":"one"}),
    )
    .unwrap();
    e.evaluate_workflows().unwrap();
    assert_eq!(
        runtime(&e, &invocation.id)["runtime"]["steps"][0]["status"],
        "awaiting_acceptance"
    );
    assert!(runtime(&e, &root)["runtime"]["steps"][0]["budget_wait_started"].is_number());
}

#[test]
fn extending_an_invocation_timeout_keeps_its_children_and_original_execution_contract() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let draft = cmd(
        &e,
        "workflow.create",
        None,
        json!({"name":"预算后继","document":document()}),
    )
    .unwrap();
    let child = cmd(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let root = start(
        &e,
        json!({"schema_version":5,"stages":[{"key":"work","name":"父流程"}],"steps":[{"key":"call","name":"持续子调用","stage":"work","executor":{"type":"workflow","workflow_ref":child["id"]},"budget":{"max_turns":4,"timeout_seconds":60,"retry_limit":1}}]}),
    );
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&root),
        json!({"step":"call"}),
    )
    .unwrap();
    let before = runtime(&e, &root);
    let execution = before["runtime"]["steps"][0].clone();
    let state = e.store.snapshot().unwrap();
    let invocation = state
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.data["parent_execution_id"] == execution["id"])
        .unwrap();
    let original_child = runtime(&e, &invocation.id);
    let mut step = before["runtime"]["plan"]["document"]["steps"][0].clone();
    step["budget"]["timeout_seconds"] = json!(7200);
    let change = json!({"plan_revision_id":before["runtime"]["active_plan_revision_id"],"reason":"授权延长整个子调用，业务合同和输入不变","operations":[{"type":"upsert_step","step":step}]});
    let preview = e
        .query(
            "workflow.change.preview",
            &json!({"id":root,"changeset":change}),
            1,
        )
        .unwrap();
    assert_eq!(preview["budget_updates"], json!(["call"]));
    assert_eq!(preview["affected"], json!([]));
    cmd(
        &e,
        "workflow.change.apply",
        Some(&root),
        json!({"changeset":change,"preview_hash":preview["preview_hash"]}),
    )
    .unwrap();
    let after = runtime(&e, &root);
    assert_eq!(after["runtime"]["steps"][0], execution);
    assert_eq!(
        runtime(&e, &invocation.id)["runtime"],
        original_child["runtime"]
    );
    assert_eq!(
        after["runtime"]["plan"]["document"]["steps"][0]["budget"]["timeout_seconds"],
        7200
    );
    let frozen = e
        .query(
            "workflow.execution.inspect",
            &json!({"id":root,"execution_id":execution["id"],"part":"contract"}),
            1,
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(frozen["value_chunk"].as_str().unwrap()).unwrap()["budget"]["timeout_seconds"],
        60
    );
}

#[test]
fn a_waiver_keeps_the_rejected_value_and_the_human_signature_separate() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let mut doc = document();
    doc["steps"][0]["inputs"] =
        json!({"assessment":{"type":"constant","value":{"approve":false,"reason":"未解决的风险"}}});
    doc["steps"][0]["completion"] =
        json!({"op":"equals","source":{"step":"one","port":"unused"},"value":true});
    // The input is read via $input so the rejected fact has no invented delivery producer.
    doc["inputs"] =
        json!({"assessment":{"delivery_ref":"builtin-delivery-object-v1","required":false}});
    doc["steps"][0]["completion"] =
        json!({"op":"exists","source":{"step":"$input","port":"assessment"}});
    let root = start(&e, doc);
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&root),
        json!({"step":"one"}),
    )
    .unwrap();
    let before = runtime(&e, &root);
    let execution = before["runtime"]["steps"][0].clone();
    assert!(cmd(&e,"workflow.step.accept",Some(&root),json!({"step":"one","execution_id":execution["id"],"input_digest":execution["input_digest"],"outputs":execution["outputs"]})).is_err());
    let changes = json!({"plan_revision_id":before["runtime"]["active_plan_revision_id"],"reason":"用户接受尚未解决的风险","operations":[{"type":"waive","node_id":execution["node_id"],"requirement":"completion"}]});
    let preview = e
        .query(
            "workflow.change.preview",
            &json!({"id":root,"changeset":changes}),
            4,
        )
        .unwrap();
    cmd(
        &e,
        "workflow.change.apply",
        Some(&root),
        json!({"changeset":changes,"preview_hash":preview["preview_hash"]}),
    )
    .unwrap();
    accept(&e, &root, "one");
    let after = runtime(&e, &root);
    assert_eq!(
        after["runtime"]["steps"][0]["inputs"]["assessment"]["approve"],
        false
    );
    assert_eq!(after["runtime"]["steps"][0]["id"], execution["id"]);
    let state = e.store.snapshot().unwrap();
    assert!(
        state
            .governance
            .iter()
            .any(|r| r.kind == "workflow_waiver" && r.data["requirement"] == "completion")
    );
    assert!(
        !state
            .governance
            .iter()
            .any(|r| r.kind == "workflow_confirmation" || r.kind == "workflow_human_decision")
    );
}

#[test]
fn output_reads_are_root_scoped_and_large_unicode_materials_are_chunked() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let root = start(&e, document());
    let value = "材料🙂".repeat(2000);
    let material = cmd(
        &e,
        "workflow.materials.import",
        Some(&root),
        json!({"delivery_ref":"builtin-delivery-design-v1","value":value,"source":"用户长文档"}),
    )
    .unwrap();
    let mut offset = 0;
    let mut combined = String::new();
    loop {
        let page = e
            .query(
                "workflow.outputs.read",
                &json!({"id":root,"ids":[material["id"]],"offset":offset,"max_bytes":4096}),
                4,
            )
            .unwrap();
        let output = &page["outputs"][0];
        combined.push_str(output["value_chunk"].as_str().unwrap());
        if let Some(next) = output["next_offset"].as_u64() {
            offset = next;
        } else {
            break;
        }
    }
    assert_eq!(
        serde_json::from_str::<Value>(&combined).unwrap(),
        json!(value)
    );
    e.store
        .transact(None, &mut |s| {
            let mut foreign = s.configuration(material["id"].as_str().unwrap())?.clone();
            foreign.id = id();
            foreign.data["root_instance_id"] = json!("another-root");
            foreign.content_hash = configuration_hash(&foreign.data);
            s.governance.push(foreign);
            Ok(Value::Null)
        })
        .unwrap();
    let foreign = e
        .store
        .snapshot()
        .unwrap()
        .governance
        .iter()
        .find(|r| r.kind == "typed_material" && r.data["root_instance_id"] == "another-root")
        .unwrap()
        .id
        .clone();
    assert!(
        e.query(
            "workflow.outputs.read",
            &json!({"id":root,"ids":[foreign]}),
            4
        )
        .is_err()
    );
}

#[test]
fn takeover_fences_a_late_native_command_without_advancing_the_next_node() {
    let directory = tempfile::tempdir().unwrap();
    let mut e = engine(directory.path());
    let harness = Arc::new(NativeManager::default());
    e.harness = harness;
    let root = start(&e, document());
    let controller=cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager","agent_ref":"builtin-agent-sol-high-project-manager-v3","policy":WorkflowControlPolicy::default()})).unwrap()["id"].as_str().unwrap().to_owned();
    for _ in 0..8 {
        drive_manager(&e);
        if e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.kind == "controller_tool_call" && r.data["status"] == "succeeded")
        {
            break;
        }
    }
    let before = runtime(&e, &root);
    let state = e.store.snapshot().unwrap();
    let mut late = state
        .governance
        .iter()
        .find(|r| r.kind == "controller_tool_call" && r.data["status"] == "succeeded")
        .unwrap()
        .clone();
    cmd(&e, "orchestration.takeover", Some(&controller), json!({})).unwrap();
    late.id = id();
    late.data["status"] = json!("prepared");
    late.data["arguments"]["step"] = json!("two");
    late.content_hash = configuration_hash(&late.data);
    let late_id = late.id.clone();
    e.store
        .transact(None, &mut |s| {
            s.governance.push(late.clone());
            Ok(Value::Null)
        })
        .unwrap();
    e.drive_controllers().unwrap();
    let state = e.store.snapshot().unwrap();
    assert_eq!(
        state.configuration(&late_id).unwrap().data["status"],
        "failed"
    );
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"],
        before["runtime"]["steps"]
    );
    assert_eq!(
        state.configuration(&controller).unwrap().data["status"],
        "paused"
    );
}

#[test]
fn changing_a_standard_covered_feature_requires_an_explicit_coverage_waiver() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let spec=cmd(&e,"spec.create",None,json!({"name":"必须保留的规范","data":{"schema_version":3,"body":"检查材料来源","checks":[]}})).unwrap();
    let mut doc = document();
    doc["steps"][0]["tags"] = json!(["protected"]);
    doc["steps"][0]["spec_refs"] = json!([spec["id"]]);
    let draft = cmd(
        &e,
        "workflow.create",
        None,
        json!({"name":"有基线的流程","document":doc}),
    )
    .unwrap();
    let workflow = cmd(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let standard=cmd(&e,"standard.create",None,json!({"name":"覆盖基线","data":{"schema_version":3,"body":"保留 protected 步骤的规范","baseline_rules":[{"selector":{"tags":["protected"]},"required_spec_refs":[spec["id"]],"min_matches":1}]}})).unwrap();
    let project = cmd(&e, "project.create", None, json!({"title":"基线验收"})).unwrap();
    cmd(&e,"project.configuration.update",project["id"].as_str(),json!({"workflow_ref":workflow["id"],"role_agent_bindings":{},"standard_ref":standard["id"]})).unwrap();
    let feature = cmd(
        &e,
        "feature.create",
        None,
        json!({"project_id":project["id"],"title":"不能暗中删除规范"}),
    )
    .unwrap();
    let state = e.store.snapshot().unwrap();
    let root = state
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.data["owner_id"] == feature["id"])
        .unwrap()
        .id
        .clone();
    let view = runtime(&e, &root);
    let mut step = view["runtime"]["plan"]["document"]["steps"][0].clone();
    step["spec_refs"] = json!([]);
    let mut changes = json!({"plan_revision_id":view["runtime"]["active_plan_revision_id"],"reason":"改变规范覆盖范围","operations":[{"type":"upsert_step","step":step}]});
    assert!(
        e.query(
            "workflow.change.preview",
            &json!({"id":root,"changeset":changes}),
            4
        )
        .unwrap_err()
        .message
        .contains("coverage")
    );
    changes["operations"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"waive","node_id":"$workflow","requirement":"coverage"}));
    let preview = e
        .query(
            "workflow.change.preview",
            &json!({"id":root,"changeset":changes}),
            4,
        )
        .unwrap();
    cmd(
        &e,
        "workflow.change.apply",
        Some(&root),
        json!({"changeset":changes,"preview_hash":preview["preview_hash"]}),
    )
    .unwrap();
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.kind == "workflow_waiver" && r.data["requirement"] == "coverage")
    );
}

#[test]
fn a_large_plan_is_inspected_in_explicit_bounded_pages() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let mut doc = document();
    doc["steps"]=json!((0..75).map(|i|json!({"key":format!("node_{i}"),"name":format!("材料节点 {i}"),"stage":"work","executor":{"type":"control"}})).collect::<Vec<_>>());
    let root = start(&e, doc);
    let mut ids = std::collections::BTreeSet::new();
    let mut offset = 0;
    loop {
        let page = e
            .query(
                "workflow.inspect",
                &json!({"id":root,"offset":offset,"limit":16}),
                1,
            )
            .unwrap();
        assert!(page["runtime"]["steps"].as_array().unwrap().len() <= 16);
        assert_eq!(page["page"]["total"], 75);
        for step in page["steps"].as_array().unwrap() {
            ids.insert(step["node_id"].as_str().unwrap().to_owned());
        }
        if let Some(next) = page["page"]["next_offset"].as_u64() {
            offset = next;
        } else {
            break;
        }
    }
    assert_eq!(ids.len(), 75);
    assert!(e.store.snapshot().unwrap().turns.is_empty());
}

#[test]
fn changes_before_send_admission_are_coalesced_into_one_pending_wakeup() {
    let directory = tempfile::tempdir().unwrap();
    let mut e = engine(directory.path());
    e.harness = Arc::new(NativeManager::default());
    let root = start(&e, document());
    cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager","agent_ref":"builtin-agent-sol-high-project-manager-v3","policy":WorkflowControlPolicy::default()})).unwrap();
    for _ in 0..3 {
        drive_manager_with_send(&e, false);
    }
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&root),
        json!({"step":"one"}),
    )
    .unwrap();
    for _ in 0..3 {
        e.drive_controllers().unwrap();
    }
    let state = e.store.snapshot().unwrap();
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.kind == "workflow_intent"
                && r.data["method"] == "conversation.send"
                && r.data["status"] == "prepared")
            .count(),
        1
    );
    assert_eq!(state.orchestration_wakeups.len(), 1);
    assert!(state.turns.is_empty());
}

#[test]
fn human_input_follows_the_controller_epoch_across_background_revision_updates() {
    let directory = tempfile::tempdir().unwrap();
    let mut e = engine(directory.path());
    e.harness = Arc::new(NativeManager::default());
    let root = start(&e, document());
    let opened=cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager","agent_ref":"builtin-agent-sol-high-project-manager-v3","policy":WorkflowControlPolicy::default()})).unwrap();
    let controller = opened["id"].as_str().unwrap();
    let original = e
        .store
        .snapshot()
        .unwrap()
        .configuration(controller)
        .unwrap()
        .revision;
    for _ in 0..3 {
        drive_manager_with_send(&e, false);
    }
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .configuration(controller)
            .unwrap()
            .revision
            > original
    );
    e.execute(
        "orchestration.input",
        Command {
            command_id: id(),
            target: Some(controller.into()),
            expected_revision: Some(original),
            payload: json!({"epoch":1,"input":"后台推进不应丢失用户指令"}),
        },
    )
    .unwrap();
    cmd(
        &e,
        "orchestration.takeover",
        Some(controller),
        json!({"epoch":1}),
    )
    .unwrap();
    assert!(
        cmd(
            &e,
            "orchestration.input",
            Some(controller),
            json!({"epoch":1,"input":"过期输入不得越过人工接管"})
        )
        .is_err()
    );
}

#[test]
fn signing_an_unchanged_waiting_gate_wakes_the_idle_manager() {
    let directory = tempfile::tempdir().unwrap();
    let mut e = engine(directory.path());
    e.harness = Arc::new(NativeManager::default());
    let mut doc = document();
    doc["steps"][0]["confirmation"] = json!("human");
    for n in 0..16 {
        doc["steps"].as_array_mut().unwrap().insert(n, json!({"key":format!("history{n}"),"name":"已完成记录","stage":"work","executor":{"type":"control"}}));
    }
    let root = start(&e, doc);
    for n in 0..16 {
        let key = format!("history{n}");
        cmd(
            &e,
            "workflow.step.dispatch",
            Some(&root),
            json!({"step":key}),
        )
        .unwrap();
        accept(&e, &root, &key);
    }
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&root),
        json!({"step":"one"}),
    )
    .unwrap();
    cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager","agent_ref":"builtin-agent-sol-high-project-manager-v3","policy":WorkflowControlPolicy::default()})).unwrap();
    for _ in 0..20 {
        drive_manager(&e);
    }
    let before = e.store.snapshot().unwrap();
    let count = before.orchestration_wakeups.len();
    assert_eq!(count, 1);
    assert!(
        before
            .turns
            .iter()
            .all(|t| t.status == TurnStatus::Completed)
    );
    let envelope = |state: &EngineeringState| -> Value {
        serde_json::from_str(
            state
                .governance
                .iter()
                .rev()
                .find(|r| r.kind == "workflow_intent" && r.data["method"] == "conversation.send")
                .unwrap()
                .data["command"]["payload"]["input"]
                .as_str()
                .unwrap(),
        )
        .unwrap()
    };
    let initial = envelope(&before);
    let projected = initial["instances"][0]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["step"] == "one")
        .unwrap();
    assert_eq!(initial["instances"][0]["step_total"], 18);
    assert_eq!(
        initial["instances"][0]["steps"].as_array().unwrap().len(),
        16
    );
    assert_eq!(projected["human_satisfied"], false);
    assert_eq!(projected["confirmation_status"], "pending");
    let view = runtime(&e, &root);
    let execution = view["runtime"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["step"] == "one")
        .unwrap();
    cmd(&e,"workflow.step.confirm",Some(&root),json!({"step":"one","input_digest":execution["input_digest"],"outputs":execution["outputs"]})).unwrap();
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][16]["status"],
        "waiting_confirmation"
    );
    drive_manager_with_send(&e, false);
    assert_eq!(
        e.store.snapshot().unwrap().orchestration_wakeups.len(),
        count + 1
    );
    let state = e.store.snapshot().unwrap();
    let updated = envelope(&state);
    let projected = updated["instances"][0]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["step"] == "one")
        .unwrap();
    let confirmation = state
        .governance
        .iter()
        .find(|r| r.kind == "workflow_confirmation" && r.definition_id == execution["id"])
        .unwrap();
    assert_eq!(projected["human_satisfied"], true);
    assert_eq!(projected["confirmation_status"], "confirmed");
    assert_eq!(projected["confirmation_ref"], confirmation.id);
    assert_eq!(projected["confirmation_waiver_ref"], Value::Null);
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][16]["status"],
        "waiting_confirmation"
    );
    for _ in 0..30 {
        drive_manager(&e);
        if runtime(&e, &root)["runtime"]["status"] == "completed" {
            break;
        }
    }
    assert_eq!(runtime(&e, &root)["runtime"]["status"], "completed");
}

#[test]
fn control_version_fences_commands_while_budget_observations_change_row_revisions() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let root = start(&e, document());
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&root),
        json!({"step":"one"}),
    )
    .unwrap();
    let view = runtime(&e, &root);
    let revision = view["record"]["revision"].as_u64().unwrap();
    e.store
        .transact(None, &mut |s| {
            let row = s.governance.iter_mut().find(|r| r.id == root).unwrap();
            row.data["steps"][0]["budget_waited_ms"] = json!(1);
            row.revision += 1;
            row.content_hash = configuration_hash(&row.data);
            Ok(Value::Null)
        })
        .unwrap();
    let payload = json!({"control_version":view["runtime"]["control_version"],"plan_revision_id":view["runtime"]["active_plan_revision_id"]});
    e.execute(
        "workflow.pause",
        Command {
            command_id: id(),
            target: Some(root.clone()),
            expected_revision: Some(revision),
            payload: payload.clone(),
        },
    )
    .unwrap();
    assert!(
        e.execute(
            "workflow.resume",
            Command {
                command_id: id(),
                target: Some(root.clone()),
                expected_revision: Some(revision),
                payload
            }
        )
        .is_err()
    );
    assert_eq!(runtime(&e, &root)["runtime"]["paused"], true);
}

#[test]
fn a_pending_human_decision_signs_only_the_exact_current_business_gate() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let mut doc = document();
    doc["steps"][0]["confirmation"] = json!("human");
    let root = start(&e, doc);
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&root),
        json!({"step":"one"}),
    )
    .unwrap();
    let before = runtime(&e, &root);
    let execution = &before["runtime"]["steps"][0];
    let args = json!({"instance_id":root,"plan_revision_id":before["runtime"]["active_plan_revision_id"],"step":"one","execution_id":execution["id"],"input_digest":execution["input_digest"],"outputs":execution["outputs"]});
    let pending=cmd(&e,"workflow.decision.request",Some(&root),json!({"operation":"workflow.step.accept","arguments":args,"reason":"请人类核对当前确切版本"})).unwrap();
    assert!(
        !e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.kind == "workflow_confirmation")
    );
    cmd(
        &e,
        "workflow.decision.resolve",
        pending["id"].as_str(),
        json!({"approved":true,"reason":"人类核对后批准本版本"}),
    )
    .unwrap();
    let state = e.store.snapshot().unwrap();
    let signed = state
        .governance
        .iter()
        .find(|r| r.kind == "workflow_confirmation")
        .unwrap();
    assert_eq!(signed.data["actor"]["kind"], "human");
    assert_eq!(signed.definition_id, execution["id"]);
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][0]["status"],
        "waiting_confirmation"
    );
    accept(&e, &root, "one");
    let decision = state
        .governance
        .iter()
        .find(|r| r.kind == "workflow_human_decision")
        .unwrap();
    let chunk = e
        .query(
            "workflow.inspect",
            &json!({"id":root,"decision_ref":decision.id}),
            1,
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(chunk["value_chunk"].as_str().unwrap()).unwrap()["arguments"],
        args
    );
    assert!(
        cmd(
            &e,
            "workflow.decision.resolve",
            pending["id"].as_str(),
            json!({"approved":true,"reason":"不可重复签署"})
        )
        .is_err()
    );
}

#[test]
fn an_instance_can_revise_its_plan_after_the_template_directory_is_reset() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let root = start(
        &e,
        json!({"schema_version":5,"stages":[{"key":"work","name":"原模板阶段"}],"steps":[{"key":"study","name":"原调查任务","stage":"work","executor":{"type":"agent","role_ref":"builtin-role-analyst"},"task_instructions":"调查原材料","outputs":{"result":{"delivery_ref":"builtin-delivery-object-v1"}}}]}),
    );
    let before = runtime(&e, &root);
    e.store
        .transact(None, &mut |s| {
            restore_template_catalog(s, "scope-reset-regression")?;
            Ok(Value::Null)
        })
        .unwrap();
    let mut step = before["runtime"]["plan"]["document"]["steps"][0].clone();
    step["task_instructions"] = json!("使用原冻结角色调查新的目标");
    let changes = json!({"plan_revision_id":before["runtime"]["active_plan_revision_id"],"reason":"重置模板后调整当前实例","operations":[{"type":"upsert_step","step":step}]});
    let preview = e
        .query(
            "workflow.change.preview",
            &json!({"id":root,"changeset":changes}),
            1,
        )
        .unwrap();
    cmd(
        &e,
        "workflow.change.apply",
        Some(&root),
        json!({"changeset":changes,"preview_hash":preview["preview_hash"]}),
    )
    .unwrap();
    assert_eq!(
        runtime(&e, &root)["runtime"]["plan"]["roles"],
        before["runtime"]["plan"]["roles"]
    );
}
