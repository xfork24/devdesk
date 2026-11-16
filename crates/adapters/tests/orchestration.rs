#[path = "../../../tests/support/configuration.rs"]
mod configuration_fixture;
// Explicit Harness control through production commands and SQLite.
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
    if method == "permission.profile.create" {
        return configuration_fixture::create_profile(e, p);
    }
    if method == "orchestration.open"
        && let Some(policy) = p.as_object_mut().and_then(|v| v.remove("policy"))
    {
        let state = e.store.snapshot()?;
        let source = selected_harness_profile(&state).unwrap();
        let mut data = source.data.clone();
        let mut authority = policy.clone();
        let mut budget = serde_json::to_value(AgentSchedulingBudget::default()).unwrap();
        for key in [
            "max_turns_per_wakeup",
            "max_tool_calls_per_turn",
            "timeout_seconds",
        ] {
            if let Some(value) = authority.as_object_mut().unwrap().remove(key) {
                budget[key] = value;
            }
        }
        data["policy"]["platform"] = authority;
        if let Some(target) = target.filter(|id| {
            state
                .configuration(id)
                .is_ok_and(|r| r.r#type == "workflow_instance")
        }) {
            configuration_fixture::scheduling(e, target, &json!({"agent":budget}))?;
        } else {
            p["budget"] = budget;
        }
        let context = configuration_fixture::create_profile(
            e,
            json!({"name":"Fixture controller Profile","data":data}),
        )?;
        p["harness_permission_ref"] = context["id"].clone();
    }
    if method == "workflow.driver.select" && p["policy_ref"].is_object() {
        let patch = p.as_object_mut().unwrap().remove("policy_ref").unwrap();
        configuration_fixture::scheduling(e, target.unwrap(), &patch)?;
    }
    if method == "workflow.start" && p["orchestration"]["policy_ref"].is_object() {
        let patch = p["orchestration"]
            .as_object_mut()
            .unwrap()
            .remove("policy_ref")
            .unwrap();
        p["workflow_ref"] = json!(configuration_fixture::workflow_with_scheduling(
            e,
            p["workflow_ref"].as_str().unwrap(),
            &patch
        )?);
    }
    let s = e.store.snapshot()?;
    if method == "workflow.driver.select" {
        p["expected_driver_epoch"] = json!(
            s.governance
                .iter()
                .find(|row| row.r#type == "workflow_driver"
                    && Some(row.definition_id.as_str()) == target)
                .and_then(|row| row.data["epoch"].as_u64())
                .unwrap_or(0)
        );
    }
    let r = target.and_then(|id| {
        s.configuration(id)
            .ok()
            .map(|r| r.revision)
            .or_else(|| s.projects.iter().find(|r| r.id == id).map(|r| r.revision))
    });
    if let Some(row) = target
        .and_then(|id| s.configuration(id).ok())
        .filter(|r| r.r#type == "workflow_instance")
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
    json!({"schema_version":6,"stages":[{"key":"work","name":"工作"}],"steps":[{"key":"one","name":"第一步","stage":"work","executor":{"type":"control"}},{"key":"two","name":"第二步","stage":"work","dependencies":["one"],"executor":{"type":"control"}}]})
}

#[test]
fn concurrent_driver_changes_require_the_original_control_epoch() {
    let directory = tempfile::tempdir().unwrap();
    let e = Arc::new(engine(directory.path()));
    let root = start(&e, document());
    let before = e.store.snapshot().unwrap();
    let revision = before.configuration(&root).unwrap().revision;
    let epoch = before
        .governance
        .iter()
        .find(|row| row.r#type == "workflow_driver" && row.definition_id == root)
        .and_then(|row| row.data["epoch"].as_u64())
        .unwrap_or(0);
    let unguarded = Command {
        command_id: "missing-driver-epoch".into(),
        target: Some(root.clone()),
        expected_revision: Some(revision),
        payload: json!({"mode":"manual"}),
    };
    assert_eq!(
        e.execute("workflow.driver.select", unguarded)
            .unwrap_err()
            .code,
        "precondition_required"
    );
    let barrier = Arc::new(std::sync::Barrier::new(100));
    let threads = (0..100)
        .map(|n| {
            let e = e.clone();
            let barrier = barrier.clone();
            let command = Command {
                command_id: format!("driver-race-{n}"),
                target: Some(root.clone()),
                expected_revision: Some(revision),
                payload: json!({"mode":"manual","expected_driver_epoch":epoch}),
            };
            std::thread::spawn(move || {
                barrier.wait();
                let result = e.execute("workflow.driver.select", command.clone());
                (command, result)
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        results.iter().filter(|(_, result)| result.is_ok()).count(),
        1
    );
    assert!(
        results
            .iter()
            .filter_map(|(_, result)| result.as_ref().err())
            .all(|error| error.code == "revision_conflict")
    );
    let (winner, receipt) = results.iter().find(|(_, result)| result.is_ok()).unwrap();
    assert_eq!(
        e.execute("workflow.driver.select", winner.clone()).unwrap(),
        *receipt.as_ref().unwrap()
    );
    let after = e.store.snapshot().unwrap();
    assert_eq!(after.configuration(&root).unwrap().revision, revision);
    assert_eq!(
        after
            .governance
            .iter()
            .find(|row| row.r#type == "workflow_driver" && row.definition_id == root)
            .unwrap()
            .data["epoch"],
        epoch + 1
    );
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
fn observing_unstarted_and_waiting_steps_does_not_clone_rollback_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let root = start(&e, document());
    let trace = devdesk_adapters::performance::Trace::start();
    e.evaluate_workflows().unwrap();
    assert!(trace.finish()["spans"]["runtime.step_checkpoint"].is_null());
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&root),
        json!({"step":"one"}),
    )
    .unwrap();
    let trace = devdesk_adapters::performance::Trace::start();
    e.evaluate_workflows().unwrap();
    assert!(trace.finish()["spans"]["runtime.step_checkpoint"].is_null());
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][0]["status"],
        "awaiting_acceptance"
    );
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][1]["status"],
        "queued"
    );
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
            .any(|r| r.r#type == "workflow_decision" && r.data["operation"] == "change")
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
                r.content_hash = content_digest(&r.refs, &r.data);
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
    let material_type = cmd(&e,"delivery_type.create",None,json!({"name":"fixture markdown","spec_refs":["fixture-spec:1"],"data":{"format":"markdown"}})).unwrap();
    let doc = json!({"schema_version":6,"stages":[{"key":"design","name":"设计"},{"key":"delivery","name":"交付"}],"steps":[
        {"key":"design","name":"编写设计","stage":"design","executor":{"type":"agent","role_ref":"builtin-role-architect:1"},"task_instructions":"分析设计","outputs":{"result":{"delivery_ref":material_type["id"]}}},
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
    let material=cmd(&e,"workflow.materials.import",Some(&id),json!({"delivery_ref":material_type["id"],"value":"用户在线下完成的接口和失败处理设计","source":"用户提供的设计"})).unwrap();
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
    let doc = json!({"schema_version":6,"stages":[{"key":"design","name":"设计"}],"steps":[{"key":"design","name":"编写设计","stage":"design","executor":{"type":"agent","role_ref":"builtin-role-architect:1"},"task_instructions":"设计接口","outputs":{"result":{"delivery_ref":"builtin-delivery-design:3"}}}],"outputs":{"result":{"step":"design","port":"result"}}});
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
            .any(|r| r.r#type == "workflow_segment" && r.data["status"] == "completed")
    );
}

#[derive(Default)]
struct NativeManager {
    segment: bool,
    human: bool,
    deliveries: std::sync::Mutex<std::collections::HashMap<String, Value>>,
    observed: std::sync::Mutex<std::collections::HashMap<String, Value>>,
    reject_replies: std::sync::atomic::AtomicBool,
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
        assert_eq!(input["renderer_version"], "orchestration-task/v2");
        self.deliveries
            .lock()
            .unwrap()
            .insert(r.session.id.clone(), input.clone());
        assert!(
            input["instructions"].is_null(),
            "runtime events cannot contain orchestration instructions"
        );
        let mut observed = self.observed.lock().unwrap();
        if input["bootstrap"] == true {
            observed.insert(r.session.id.clone(), input["instances"][0].clone());
        } else {
            assert!(
                input["instances"].is_null(),
                "subsequent turns receive events, not repeated graph snapshots"
            );
        }
        let instance = observed.get_mut(&r.session.id).unwrap();
        for event in input["events"].as_array().into_iter().flatten() {
            if event["source"]["change"]["kind"] == "human.confirmed" {
                for step in instance["steps"].as_array_mut().unwrap() {
                    if step["id"] == event["source"]["change"]["execution_id"] {
                        step["human_satisfied"] = json!(true);
                    }
                }
            }
            for change in event["source"]["change"]["changes"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if change["kind"] == "active_plan_revision_id" {
                    instance["plan"] = change["value"].clone();
                }
                if change["kind"] == "execution" {
                    let execution = &change["execution"];
                    if let Some(step) = instance["steps"]
                        .as_array_mut()
                        .unwrap()
                        .iter_mut()
                        .find(|s| s["step"] == execution["step"])
                    {
                        *step = execution.clone();
                        step["id"] = execution["execution_id"].clone();
                    }
                }
            }
        }
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
            if self.human && e["status"] == "waiting_confirmation" && e["human_satisfied"] != true {
                (
                    "workflow_decision_request",
                    json!({"instance_id":root,"plan_revision_id":plan,"operation":"workflow.step.confirm","reason":"Fixture Human must decide the exact Control result","arguments":{"instance_id":root,"plan_revision_id":plan,"step":e["step"],"execution_id":e["id"],"input_digest":e["input_digest"],"outputs":e["outputs"]},"interaction":{"title":"确认夹具控制结果","background":"当前明确配置了人工门禁","requested_action":"确认当前结果或拒绝","completed_work":"已实际派发 Control 并读取其结果","review_summary":"此夹具不要求独立 Agent 评审","materials":[],"evidence":[{"label":"真实 Control 执行","purpose":"核对确切执行状态","instance_id":root,"execution_id":e["id"]}]}}),
                )
            } else if e["status"] == "queued" {
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
        if self
            .reject_replies
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return Err(err(
                "provider_failure",
                "fixture interrupted native reply transport",
            ));
        }
        assert!(
            r.structured_content.is_none() || r.content.is_empty(),
            "native controller results must be sent once"
        );
        self.replies.lock().unwrap().push(r.clone());
        let turn = self.turns.lock().unwrap()[&s.id].clone();
        if self.human
            && r.structured_content
                .as_ref()
                .is_some_and(|v| v["status"] == "pending")
        {
            let delivery = self.deliveries.lock().unwrap()[&s.id].clone();
            let instance = self.observed.lock().unwrap()[&s.id].clone();
            self.pending.lock().unwrap().insert(s.id.clone(),vec![ports::ConversationObservation::ToolCall {
                turn:turn.clone(),request:json!(18),thread:s.external_session.clone().unwrap(),
                external_turn:format!("native-turn-{turn}"),call:format!("wait-{turn}"),
                tool:"devdesk_workflow_controller_wait".into(),worktree:String::new(),
                arguments:json!({"instance_id":instance["id"],"plan_revision_id":instance["plan"],"delivery_id":delivery["delivery_id"],"summary":"已准备材料并请求人类决定，等待实际签署","waiting_for":[instance["id"]]})
            }]);
            return Ok(());
        }
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
            let a = s.resolve_agent(&record.id).unwrap();
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
                context_protocol: None,
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
            let permissions = binding.permissions.as_ref().unwrap();
            binding.writable_paths = permissions.scope.writable_paths.clone();
            binding.read_only_paths = permissions.scope.read_only_paths.clone();
            binding.tool_policy = if binding.writable_paths.is_empty() {
                ToolPolicy::ReadOnly
            } else if binding.writable_paths == ["."] {
                ToolPolicy::WorkspaceWrite
            } else {
                ToolPolicy::ScopedWrite
            };
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

struct RejectedManager;
impl ports::Harness for RejectedManager {
    fn open_session(&self, _: &ports::SessionStart) -> Result<ports::SessionReceipt> {
        Err(err("provider_failure", "worker 未启动，输入未发送").technical_not_applied())
    }
    fn stop_session(&self, _: &Session, execution: &RuntimeExecution) -> Result<ports::WorkerStop> {
        Ok(ports::WorkerStop {
            operation_id: execution.operation_id.clone(),
            writer_revoked: true,
            mode: "fixture_absent_container".into(),
            requires_successor: true,
        })
    }
}

#[test]
fn manager_starts_without_extra_input_and_surfaces_rejected_startup_until_resumed() {
    let directory = tempfile::tempdir().unwrap();
    let mut e = engine(directory.path());
    e.harness = Arc::new(RejectedManager);
    let root = start(&e, document());
    cmd(&e, "orchestration.open", Some(&root), json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10"})).unwrap();
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"agent"}),
    )
    .unwrap();
    for _ in 0..8 {
        drive_manager_cycle(&e, true, false);
    }
    let state = e.store.snapshot().unwrap();
    let controller = state
        .governance
        .iter()
        .find(|r| r.r#type == "workflow_controller")
        .unwrap();
    assert_eq!(controller.data["status"], "failed");
    assert!(
        controller.data["reason"]
            .as_str()
            .unwrap()
            .contains("worker 未启动")
    );
    assert_eq!(
        state.turns.len(),
        1,
        "must not silently replay the rejected input"
    );
    assert!(!state.turns[0].consumed);
    assert_eq!(state.turns[0].status, TurnStatus::Failed);
    assert!(state.executions.iter().all(|e| e.writer_revoked));
    let input: Value = serde_json::from_str(&state.turns[0].input).unwrap();
    assert_eq!(input["user_messages"], json!([]));
    assert_eq!(
        input["instances"][0]["steps"][0]["confirmation_policy"],
        "automatic"
    );
    assert!(input["instructions"].is_null());
    assert_eq!(input["bootstrap"], true);
    let previous_session = controller.data["session_id"].clone();
    cmd(
        &e,
        "orchestration.resume",
        Some(&controller.id),
        json!({"epoch":controller.data["epoch"]}),
    )
    .unwrap();
    for _ in 0..8 {
        drive_manager_cycle(&e, true, false);
    }
    let resumed = e.store.snapshot().unwrap();
    assert_eq!(resumed.turns.len(), 2);
    assert_ne!(
        resumed.configuration(&controller.id).unwrap().data["session_id"],
        previous_session
    );
    assert_eq!(resumed.turns[0].id, state.turns[0].id);
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
    let controller = cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10","policy":WorkflowControlPolicy::default()})).unwrap();
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
    cmd(&e,"orchestration.open",Some(&id),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10","policy":WorkflowControlPolicy::default()})).unwrap();
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
    assert!(
        !s.governance
            .iter()
            .any(|r| r.r#type == "configured_session")
    );
    assert!(
        s.orchestration_wakeups
            .iter()
            .any(|w| w.status == "consumed")
    );
    assert_eq!(
        s.governance
            .iter()
            .filter(|r| r.r#type == "controller_tool_reply")
            .count(),
        5
    );
    assert_eq!(
        s.governance
            .iter()
            .filter(|r| r.r#type == "workflow_decision" && r.data["actor"]["kind"] == "controller")
            .count(),
        5
    );
}

#[test]
fn committed_native_action_recovers_its_receipt_after_host_interruption_and_takeover() {
    struct InterruptedAction {
        inner: Arc<dyn EngineeringStore>,
        armed: std::sync::atomic::AtomicBool,
    }
    impl EngineeringStore for InterruptedAction {
        fn root(&self) -> String {
            self.inner.root()
        }
        fn snapshot(&self) -> Result<EngineeringState> {
            self.inner.snapshot()
        }
        fn replay(&self, id: &str, fp: &str) -> Result<Option<Value>> {
            self.inner.replay(id, fp)
        }
        fn transact(
            &self,
            key: Option<(&str, &str)>,
            action: &mut dyn FnMut(&mut EngineeringState) -> Result<Value>,
        ) -> Result<Value> {
            let value = self.inner.transact(key, action)?;
            if key.is_some_and(|(id, _)| id.ends_with(":action"))
                && self.armed.swap(false, std::sync::atomic::Ordering::Relaxed)
            {
                panic!("fixture Host exits after durable action, before native result");
            }
            Ok(value)
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let mut e = engine(directory.path());
    e.harness = Arc::new(NativeManager::default());
    let root = start(&e, document());
    cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10","policy":WorkflowControlPolicy::default()})).unwrap();
    for _ in 0..20 {
        drive_manager_cycle(&e, true, false);
        if e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|row| row.r#type == "controller_tool_call" && row.data["status"] == "prepared")
        {
            break;
        }
    }
    let inner = e.store.clone();
    e.store = Arc::new(InterruptedAction {
        inner,
        armed: std::sync::atomic::AtomicBool::new(true),
    });
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| e.drive_controllers())).is_err()
    );
    let state = e.store.snapshot().unwrap();
    assert_eq!(
        state
            .governance
            .iter()
            .find(|row| row.r#type == "controller_tool_call")
            .unwrap()
            .data["status"],
        "prepared"
    );
    let turn = state
        .turns
        .iter()
        .find(|turn| turn.status.active())
        .unwrap();
    e.execute(
        "conversation.cancel",
        Command {
            command_id: id(),
            target: Some(turn.id.clone()),
            expected_revision: Some(turn.revision),
            payload: json!({}),
        },
    )
    .unwrap();
    e.tick_conversations(1).unwrap();
    e.drive_controllers().unwrap();
    let state = e.store.snapshot().unwrap();
    assert_eq!(
        state
            .governance
            .iter()
            .find(|row| row.r#type == "controller_tool_call")
            .unwrap()
            .data["status"],
        "succeeded"
    );
    assert_eq!(
        state
            .governance
            .iter()
            .find(|row| row.r#type == "controller_tool_reply")
            .unwrap()
            .data["status"],
        "worker_unavailable"
    );
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|row| row.r#type == "workflow_decision" && row.data["operation"] == "dispatch")
            .count(),
        1
    );
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][0]["status"],
        "awaiting_acceptance"
    );
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][1]["status"],
        "queued"
    );
}

#[test]
fn cancelling_manager_turn_revokes_prepared_calls_before_any_business_dispatch() {
    let directory = tempfile::tempdir().unwrap();
    let mut e = engine(directory.path());
    e.harness = Arc::new(NativeManager::default());
    let root = start(&e, document());
    cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10","policy":WorkflowControlPolicy::default()})).unwrap();
    for _ in 0..20 {
        drive_manager_cycle(&e, true, false);
        if e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|row| row.r#type == "controller_tool_call" && row.data["status"] == "prepared")
        {
            break;
        }
    }
    let state = e.store.snapshot().unwrap();
    let turn = state
        .turns
        .iter()
        .find(|turn| turn.status.active())
        .unwrap();
    e.execute(
        "conversation.cancel",
        Command {
            command_id: id(),
            target: Some(turn.id.clone()),
            expected_revision: Some(turn.revision),
            payload: json!({}),
        },
    )
    .unwrap();
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .find(|row| row.r#type == "workflow_controller")
            .unwrap()
            .data["status"],
        "paused"
    );
    e.tick_conversations(1).unwrap();
    e.drive_controllers().unwrap();
    let state = e.store.snapshot().unwrap();
    assert_eq!(
        state.turns.len(),
        1,
        "cancel cannot silently create another manager Turn"
    );
    assert!(
        state
            .executions
            .iter()
            .all(|execution| execution.writer_revoked)
    );
    assert!(
        !state
            .governance
            .iter()
            .any(|row| row.r#type == "workflow_decision"
                && row.data["actor"]["kind"] == "controller")
    );
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][0]["status"],
        "queued"
    );
}

#[test]
fn native_result_is_delivered_in_the_same_drive_and_never_redispatched() {
    let directory = tempfile::tempdir().unwrap();
    let harness = Arc::new(NativeManager::default());
    let mut e = engine(directory.path());
    e.harness = harness.clone();
    let root = start(&e, document());
    cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10","policy":WorkflowControlPolicy::default()})).unwrap();
    for _ in 0..20 {
        drive_manager_cycle(&e, true, false);
        if e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.r#type == "controller_tool_call" && r.data["status"] == "prepared")
        {
            break;
        }
    }
    assert!(harness.replies.lock().unwrap().is_empty());
    e.drive_controllers().unwrap();
    assert_eq!(
        harness.replies.lock().unwrap().len(),
        1,
        "reply must not wait for a later drive/tick"
    );
    e.drive_controllers().unwrap();
    assert_eq!(harness.replies.lock().unwrap().len(), 1);
    let state = e.store.snapshot().unwrap();
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_decision" && r.data["operation"] == "dispatch")
            .count(),
        1
    );
}

#[test]
fn native_control_survives_restart_after_action_and_before_reply_without_replay() {
    let directory = tempfile::tempdir().unwrap();
    let harness = Arc::new(NativeManager::default());
    harness
        .reject_replies
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let mut e = engine(directory.path());
    e.harness = harness.clone();
    let root = start(&e, document());
    cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10","policy":WorkflowControlPolicy::default()})).unwrap();
    for _ in 0..20 {
        drive_manager_cycle(&e, true, false);
        if e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.r#type == "controller_tool_call" && r.data["status"] == "prepared")
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
        .find(|r| r.r#type == "controller_tool_call")
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
    harness
        .reject_replies
        .store(false, std::sync::atomic::Ordering::Relaxed);
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
            .filter(|r| r.r#type == "controller_tool_reply" && r.definition_id == call.id)
            .count(),
        1
    );
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_decision" && r.data["operation"] == "dispatch")
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
    let opened=cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10","policy":WorkflowControlPolicy::default()})).unwrap();
    for _ in 0..20 {
        drive_manager_cycle(&e, true, false);
        if e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.r#type == "controller_tool_call" && r.data["status"] == "prepared")
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
        .filter(|r| r.r#type == "controller_tool_call")
        .collect::<Vec<_>>();
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_segment" && r.data["status"] == "active"),
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
            .any(|r| r.r#type == "workflow_segment" && r.data["status"] == "stopped")
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
fn revising_child_ignores_unused_parent_roles_and_resource_locks() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let draft = cmd(
        &e,
        "workflow.create",
        None,
        json!({"name":"子流程","document":document()}),
    )
    .unwrap();
    let child = cmd(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let root = start(
        &e,
        json!({"schema_version":6,"stages":[{"key":"work","name":"工作"}],"steps":[
            {"key":"call","name":"调用","stage":"work","executor":{"type":"workflow","workflow_ref":child["id"]}},
            {"key":"parent_only","name":"父流程独有职责","stage":"work","task_instructions":"读取父流程需求","executor":{"type":"agent","role_ref":"builtin-role-analyst:1"}}
        ]}),
    );
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&root),
        json!({"step":"call"}),
    )
    .unwrap();
    let state = e.store.snapshot().unwrap();
    let child = state
        .governance
        .iter()
        .find(|r| r.r#type == "workflow_instance" && r.id != root)
        .unwrap()
        .id
        .clone();
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&child),
        json!({"step":"one"}),
    )
    .unwrap();
    accept(&e, &child, "one");
    let before = runtime(&e, &child);
    assert!(
        before["runtime"]["plan"]["roles"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    let mut step = before["runtime"]["plan"]["document"]["steps"][1].clone();
    step["name"] = json!("新的尾部交付说明");
    let changes = json!({"plan_revision_id":before["runtime"]["active_plan_revision_id"],"reason":"仅修订尾部","operations":[{"type":"upsert_step","step":step}]});
    let preview = e
        .query(
            "workflow.change.preview",
            &json!({"id":child,"changeset":changes}),
            4,
        )
        .unwrap();
    assert_eq!(preview["affected"], json!(["two"]));
    cmd(
        &e,
        "workflow.change.apply",
        Some(&child),
        json!({"changeset":changes,"preview_hash":preview["preview_hash"]}),
    )
    .unwrap();
    let after = runtime(&e, &child);
    assert_eq!(after["runtime"]["steps"][0], before["runtime"]["steps"][0]);
    assert!(
        after["runtime"]["plan"]["roles"]
            .as_object()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn reverting_unstarted_step_revision_restores_exact_receipts_and_rejects_started_or_foreign_changes()
 {
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
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&root),
        json!({"step":"two"}),
    )
    .unwrap();
    let before = runtime(&e, &root);
    let operations = before["runtime"]["plan"]["document"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            let mut step = s.clone();
            step["name"] = json!("修订合同");
            json!({"type":"upsert_step","step":step})
        })
        .collect::<Vec<_>>();
    let changes = json!({"plan_revision_id":before["runtime"]["active_plan_revision_id"],"reason":"修订全部合同","operations":operations});
    let preview = e
        .query(
            "workflow.change.preview",
            &json!({"id":root,"changeset":changes}),
            4,
        )
        .unwrap();
    let applied = cmd(
        &e,
        "workflow.change.apply",
        Some(&root),
        json!({"changeset":changes,"preview_hash":preview["preview_hash"]}),
    )
    .unwrap();
    let changed = runtime(&e, &root);
    assert!(
        changed["runtime"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["status"] == "queued")
    );
    let revert = json!({"plan_revision_id":changed["runtime"]["active_plan_revision_id"],"reason":"撤回尚未启动的修订","operations":[{"type":"revert_change","change_id":applied["id"]}]});
    let foreign = start(&e, document());
    let mut foreign_revert = revert.clone();
    foreign_revert["plan_revision_id"] =
        runtime(&e, &foreign)["runtime"]["active_plan_revision_id"].clone();
    assert!(
        e.query(
            "workflow.change.preview",
            &json!({"id":foreign,"changeset":foreign_revert}),
            4
        )
        .is_err()
    );
    let preview = e
        .query(
            "workflow.change.preview",
            &json!({"id":root,"changeset":revert}),
            4,
        )
        .unwrap();
    cmd(
        &e,
        "workflow.change.apply",
        Some(&root),
        json!({"changeset":revert,"preview_hash":preview["preview_hash"]}),
    )
    .unwrap();
    let restored = runtime(&e, &root);
    assert_eq!(restored["runtime"]["steps"], before["runtime"]["steps"]);
    assert_eq!(restored["runtime"]["plan"], before["runtime"]["plan"]);
    assert_ne!(
        restored["runtime"]["active_plan_revision_id"],
        before["runtime"]["active_plan_revision_id"]
    );
    assert!(
        e.query(
            "workflow.change.preview",
            &json!({"id":root,"changeset":revert}),
            4
        )
        .is_err()
    );
    accept(&e, &root, "two");
    let mut changes = changes;
    changes["plan_revision_id"] = runtime(&e, &root)["runtime"]["active_plan_revision_id"].clone();
    let preview = e
        .query(
            "workflow.change.preview",
            &json!({"id":root,"changeset":changes}),
            4,
        )
        .unwrap();
    let applied = cmd(
        &e,
        "workflow.change.apply",
        Some(&root),
        json!({"changeset":changes,"preview_hash":preview["preview_hash"]}),
    )
    .unwrap();
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&root),
        json!({"step":"one"}),
    )
    .unwrap();
    let revert = json!({"plan_revision_id":runtime(&e,&root)["runtime"]["active_plan_revision_id"],"reason":"禁止丢弃已启动的替代执行","operations":[{"type":"revert_change","change_id":applied["id"]}]});
    assert!(
        e.query(
            "workflow.change.preview",
            &json!({"id":root,"changeset":revert}),
            4
        )
        .is_err()
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
        json!({"schema_version":6,"stages":[{"key":"calls","name":"两次调用"}],"steps":[{"key":"left","name":"左调用","stage":"calls","executor":{"type":"workflow","workflow_ref":child["id"]}},{"key":"right","name":"右调用","stage":"calls","executor":{"type":"workflow","workflow_ref":child["id"]}}]}),
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
                r.r#type == "workflow_instance"
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
            .any(|r| r.r#type == "workflow_child_override" && r.data["child_instance_id"] == left)
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
        json!({"assessment":{"delivery_ref":"fixture-delivery-object:1","required":false}});
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
            .any(|r| r.r#type == "workflow_segment"
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
        json!({"schema_version":6,"stages":[{"key":"work","name":"父流程"}],"steps":[{"key":"call","name":"显式子调用","stage":"work","executor":{"type":"workflow","workflow_ref":child["id"]}}]}),
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
            r.r#type == "workflow_instance" && r.id != root && r.data["root_instance_id"] == root
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
        json!({"schema_version":6,"stages":[{"key":"work","name":"父流程"}],"steps":[{"key":"call","name":"持续子调用","stage":"work","executor":{"type":"workflow","workflow_ref":child["id"]},"budget":{"max_turns":4,"timeout_seconds":60,"retry_limit":1}}]}),
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
        .find(|r| {
            r.r#type == "workflow_instance" && r.data["parent_execution_id"] == execution["id"]
        })
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
        json!({"assessment":{"delivery_ref":"fixture-delivery-object:1","required":false}});
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
            .any(|r| r.r#type == "workflow_waiver" && r.data["requirement"] == "completion")
    );
    assert!(
        !state
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_confirmation" || r.r#type == "workflow_human_decision")
    );
}

#[test]
fn output_reads_are_root_scoped_and_large_unicode_materials_are_chunked() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let material_type = cmd(&e,"delivery_type.create",None,json!({"name":"fixture markdown","spec_refs":["fixture-spec:1"],"data":{"format":"markdown"}})).unwrap();
    let root = start(&e, document());
    let value = "材料🙂".repeat(2000);
    let material = cmd(
        &e,
        "workflow.materials.import",
        Some(&root),
        json!({"delivery_ref":material_type["id"],"value":value,"source":"用户长文档"}),
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
            foreign.content_hash = content_digest(&foreign.refs, &foreign.data);
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
        .find(|r| r.r#type == "typed_material" && r.data["root_instance_id"] == "another-root")
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
    let controller=cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10","policy":WorkflowControlPolicy::default()})).unwrap()["id"].as_str().unwrap().to_owned();
    for _ in 0..8 {
        drive_manager(&e);
        if e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.r#type == "controller_tool_call" && r.data["status"] == "succeeded")
        {
            break;
        }
    }
    let before = runtime(&e, &root);
    let state = e.store.snapshot().unwrap();
    let mut late = state
        .governance
        .iter()
        .find(|r| r.r#type == "controller_tool_call" && r.data["status"] == "succeeded")
        .unwrap()
        .clone();
    cmd(&e, "orchestration.takeover", Some(&controller), json!({})).unwrap();
    late.id = id();
    late.data["status"] = json!("prepared");
    late.data["arguments"]["step"] = json!("two");
    late.content_hash = content_digest(&late.refs, &late.data);
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
fn a_large_plan_is_inspected_in_explicit_bounded_pages() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let mut doc = document();
    doc["steps"]=json!((0..75).map(|i|json!({"key":format!("node_{i}"),"name":format!("材料节点 {i}"),"stage":"work","executor":{"type":"control"}})).collect::<Vec<_>>());
    let root = start(&e, doc);
    let before = configuration_hash(&e.store.snapshot().unwrap());
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
        let keys: Vec<_> = page["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["step"].clone())
            .collect();
        assert_eq!(
            keys,
            page["runtime"]["plan"]["document"]["steps"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["key"].clone())
                .collect::<Vec<_>>()
        );
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
    assert_eq!(before, configuration_hash(&e.store.snapshot().unwrap()));
    assert!(e.store.snapshot().unwrap().turns.is_empty());
}

#[test]
fn changes_before_send_admission_are_coalesced_into_one_pending_wakeup() {
    let directory = tempfile::tempdir().unwrap();
    let mut e = engine(directory.path());
    e.harness = Arc::new(NativeManager::default());
    let root = start(&e, document());
    cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10","policy":WorkflowControlPolicy::default()})).unwrap();
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
            .filter(|r| r.r#type == "workflow_intent"
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
    let opened=cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10","policy":WorkflowControlPolicy::default()})).unwrap();
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
    cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10","policy":WorkflowControlPolicy::default()})).unwrap();
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
                .find(|r| r.r#type == "workflow_intent" && r.data["method"] == "conversation.send")
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
    assert!(updated["instances"].is_null());
    assert!(updated["instructions"].is_null());
    let confirmation = state
        .governance
        .iter()
        .find(|r| r.r#type == "workflow_confirmation" && r.definition_id == execution["id"])
        .unwrap();
    assert!(
        updated["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["source"]["change"]["confirmation_ref"] == confirmation.id)
    );
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
            row.content_hash = content_digest(&row.refs, &row.data);
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
            .any(|r| r.r#type == "workflow_confirmation")
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
        .find(|r| r.r#type == "workflow_confirmation")
        .unwrap();
    assert_eq!(signed.data["actor"]["kind"], "human");
    assert_eq!(signed.definition_id, execution["id"]);
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][0]["status"],
        "completed"
    );
    let decision = state
        .governance
        .iter()
        .find(|r| r.r#type == "workflow_human_decision")
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

fn pending_business_gate(e: &Engine, root: &str) -> Value {
    let current = runtime(e, root);
    let execution = &current["runtime"]["steps"][0];
    cmd(e, "workflow.decision.request", Some(root), json!({
        "operation":"workflow.step.accept", "reason":"请核对当前PRD",
        "arguments":{"instance_id":root,"plan_revision_id":current["runtime"]["active_plan_revision_id"],"step":"one","execution_id":execution["id"],"input_digest":execution["input_digest"],"outputs":execution["outputs"]},
        "interaction":{"title":"确认PRD","background":"当前需求已整理","requested_action":"确认当前版本","completed_work":"已产生当前结果","review_summary":"当前检查通过","materials":[],"evidence":[{"label":"当前执行","purpose":"核对执行结果","instance_id":root,"execution_id":execution["id"]}]}
    })).unwrap()
}

#[test]
fn manager_chat_signs_only_one_observed_gate_and_never_conditional_or_stale_approval() {
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
    let opened = cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10","policy":WorkflowControlPolicy::default()})).unwrap();
    let controller = opened["id"].as_str().unwrap();
    let pending = pending_business_gate(&e, &root);
    let context = json!([{"id":pending["id"],"revision":1}]);
    for input in [
        "不要确认PRD",
        "确认PRD前先修改标题",
        "确认PRD，如果测试通过",
        "如果通过就批准",
        "确认是否符合要求",
        "确认PRD，但先不要继续",
    ] {
        cmd(
            &e,
            "orchestration.input",
            Some(controller),
            json!({"epoch":1,"input":input,"approval_context":context}),
        )
        .unwrap();
        assert_eq!(
            runtime(&e, &root)["runtime"]["steps"][0]["status"],
            "waiting_confirmation",
            "{input}"
        );
    }
    assert!(cmd(&e,"orchestration.input",Some(controller),json!({"epoch":1,"input":"确认PRD","approval_context":[{"id":pending["id"],"revision":99}]})).is_err());
    cmd(
        &e,
        "orchestration.input",
        Some(controller),
        json!({"epoch":1,"input":"确认PRD"}),
    )
    .unwrap();
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][0]["status"],
        "waiting_confirmation"
    );
    let approved = cmd(
        &e,
        "orchestration.input",
        Some(controller),
        json!({"epoch":1,"input":"确认PRD，继续执行后续步骤","approval_context":context}),
    )
    .unwrap();
    assert_eq!(approved["approval"]["status"], "approved");
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][0]["status"],
        "completed"
    );
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_confirmation" && r.data["actor"]["kind"] == "human")
    );
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&root),
        json!({"step":"two"}),
    )
    .unwrap();
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][1]["status"],
        "awaiting_acceptance"
    );
}

#[test]
fn workflow_confirmation_settles_the_same_request_and_accepts_the_exact_result() {
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
    let inbox = e.workflow_driver_query(&json!({"id":root})).unwrap();
    assert_eq!(inbox["approvals"].as_array().unwrap().len(), 1);
    assert_eq!(inbox["approvals"][0]["operation"], "workflow.step.confirm");
    let pending = pending_business_gate(&e, &root);
    let without_manager = e
        .query("query.orchestration", &json!({"root_instance_id":root}), 1)
        .unwrap();
    assert!(without_manager["record"].is_null());
    assert_eq!(without_manager["decisions"][0]["id"], pending["id"]);
    let mut args = inbox["approvals"][0]["arguments"].clone();
    args["reason"] = json!("已核对材料");
    cmd(&e, "workflow.step.confirm", Some(&root), args).unwrap();
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][0]["status"],
        "completed"
    );
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .configuration(pending["id"].as_str().unwrap())
            .unwrap()
            .data["status"],
        "approved"
    );
    assert!(
        e.workflow_driver_query(&json!({"id":root})).unwrap()["approvals"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn manager_sees_and_signs_a_deep_child_gate_before_any_model_decision_request() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let mut leaf_doc = document();
    leaf_doc["steps"][0]["name"] = json!("子流程人工验收");
    leaf_doc["steps"][0]["confirmation"] = json!("human");
    let draft = cmd(
        &e,
        "workflow.create",
        None,
        json!({"name":"叶子流程","document":leaf_doc}),
    )
    .unwrap();
    let leaf = cmd(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let middle_doc = json!({"schema_version":6,"stages":[{"key":"work","name":"Task执行"}],"steps":[{"key":"call","name":"运行任务子流程","stage":"work","executor":{"type":"workflow","workflow_ref":leaf["id"]}}]});
    let draft = cmd(
        &e,
        "workflow.create",
        None,
        json!({"name":"中间流程","document":middle_doc}),
    )
    .unwrap();
    let middle = cmd(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let root = start(
        &e,
        json!({"schema_version":6,"stages":[{"key":"work","name":"Feature"}],"steps":[{"key":"call","name":"执行Task","stage":"work","executor":{"type":"workflow","workflow_ref":middle["id"]}}]}),
    );
    let opened = cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10","policy":WorkflowControlPolicy::default()})).unwrap();
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&root),
        json!({"step":"call"}),
    )
    .unwrap();
    let mid = e
        .query("query.workflow_instance", &json!({"id":root}), 1)
        .unwrap()["children"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&mid),
        json!({"step":"call"}),
    )
    .unwrap();
    let child = e
        .query("query.workflow_instance", &json!({"id":mid}), 1)
        .unwrap()["children"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&child),
        json!({"step":"one"}),
    )
    .unwrap();
    let inbox = e.workflow_driver_query(&json!({"id":root})).unwrap();
    assert_eq!(inbox["approvals"].as_array().unwrap().len(), 1);
    assert_eq!(inbox["approvals"][0]["instance_id"], child);
    assert_eq!(inbox["approvals"][0]["human_required"], true);
    assert_eq!(
        runtime(&e, &root)["human_gates"]["items"][0]["instance_id"],
        child
    );
    assert!(
        !e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_pending_decision")
    );
    for _ in 0..4 {
        drive_manager_with_send(&e, false);
    }
    let state = e.store.snapshot().unwrap();
    let send = state
        .governance
        .iter()
        .rev()
        .find(|r| r.r#type == "workflow_intent" && r.data["method"] == "conversation.send")
        .unwrap();
    let input: Value =
        serde_json::from_str(send.data["command"]["payload"]["input"].as_str().unwrap()).unwrap();
    assert_eq!(input["human_gates"]["items"][0]["instance_id"], child);
    let controller = state.configuration(opened["id"].as_str().unwrap()).unwrap();
    let session = controller.data["session_id"].as_str().unwrap();
    let detail = e.query("query.session", &json!({"id":session}), 1).unwrap();
    let context = &detail["approval_context"][0];
    assert_eq!(context["approval_source"], "workflow_step");
    assert_eq!(context["definition_id"], child);
    let result = cmd(&e,"orchestration.input",Some(&controller.id),json!({"epoch":controller.data["epoch"],"input":"确认，继续执行后续步骤","approval_context":[{"id":context["id"],"revision":context["revision"]}]})).unwrap();
    assert_eq!(result["approval"]["status"], "approved");
    assert_eq!(
        runtime(&e, &child)["runtime"]["steps"][0]["status"],
        "completed"
    );
    assert_eq!(
        e.workflow_driver_query(&json!({"id":root})).unwrap()["approvals"],
        json!([])
    );
    let policy = rust_policy(&e, 32);
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"rust","policy_ref":policy}),
    )
    .unwrap();
    for _ in 0..40 {
        e.drive_rust_workflows().unwrap();
        e.evaluate_workflows().unwrap();
        if runtime(&e, &root)["runtime"]["status"] == "completed" {
            break;
        }
    }
    assert_eq!(runtime(&e, &root)["runtime"]["status"], "completed");
}

#[test]
fn manager_chat_does_not_approve_multiple_pending_requests() {
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
    let opened = cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10","policy":WorkflowControlPolicy::default()})).unwrap();
    let one = pending_business_gate(&e, &root);
    let two = pending_business_gate(&e, &root);
    let result = cmd(&e,"orchestration.input",opened["id"].as_str(),json!({"epoch":1,"input":"确认PRD，继续执行后续步骤","approval_context":[{"id":one["id"],"revision":1},{"id":two["id"],"revision":1}]})).unwrap();
    assert!(result["approval"].is_null());
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][0]["status"],
        "waiting_confirmation"
    );
}

#[test]
fn an_instance_can_revise_its_plan_after_the_template_directory_is_reset() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let root = start(
        &e,
        json!({"schema_version":6,"stages":[{"key":"work","name":"原模板阶段"}],"steps":[{"key":"study","name":"原调查任务","stage":"work","executor":{"type":"agent","role_ref":"builtin-role-analyst:1"},"task_instructions":"调查原材料","outputs":{"result":{"delivery_ref":"fixture-delivery-object:1"}}}]}),
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

#[test]
fn source_control_policy_is_canonical_after_reload_without_changing_authorization() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let root = start(&e, document());
    let opened=cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10"})).unwrap();
    let controller = opened["id"].as_str().unwrap().to_owned();
    let before = e
        .store
        .snapshot()
        .unwrap()
        .configuration(&controller)
        .unwrap()
        .data
        .clone();
    cmd(
        &e,
        "orchestration.input",
        Some(&controller),
        json!({"epoch":1,"input":"按既有授权核对当前工作"}),
    )
    .unwrap();
    drop(e);
    let e = engine(dir.path());
    cmd(
        &e,
        "orchestration.input",
        Some(&controller),
        json!({"epoch":1,"input":"继续核对既有授权"}),
    )
    .unwrap();
    let after = e
        .store
        .snapshot()
        .unwrap()
        .configuration(&controller)
        .unwrap()
        .data
        .clone();
    assert_eq!(before["policy"], after["policy"]);
    assert_eq!(before["policy_digest"], after["policy_digest"]);
}

#[test]
fn human_adopts_root_manager_sources_only_after_the_old_writer_is_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = engine(dir.path());
    e.harness = Arc::new(NativeManager::default());
    let root = start(&e, document());
    let state = e.store.snapshot().unwrap();
    let original = state
        .configuration("builtin-agent-sol-high-project-manager:10")
        .unwrap();
    let mut successor = original.source();
    successor.identity = format!("selected-manager-{}", id());
    successor.revision = 1;
    successor.name = "Selected successor".into();
    let next_agent = source_reference(&successor.identity, successor.revision).unwrap();
    let change = ConfigurationChangeSet {
        operations: vec![ConfigurationOperation::Create { source: successor }],
    };
    let preview = e.preview_configuration_change(&change).unwrap();
    cmd(
        &e,
        "configuration.change.apply",
        None,
        json!({"changeset":change,"preview_hash":preview["preview_hash"]}),
    )
    .unwrap();
    let context = selected_harness_profile(&state).unwrap();
    let data = context.data.clone();
    configuration_fixture::scheduling(&e,&root,&json!({"agent":{"max_turns_per_wakeup":8,"max_tool_calls_per_turn":64,"timeout_seconds":1800}})).unwrap();
    let mut payload = json!({"name":"Selected longer budget","scope":"global","data":data});
    for (k, v) in &context.refs {
        payload[k] = v.clone();
    }
    let next_context = cmd(&e, "permission.profile.create", None, payload).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let controller = cmd(
        &e,
        "orchestration.open",
        Some(&root),
        json!({"role_ref":"builtin-role-project-manager:1","agent_ref":original.id}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"agent"}),
    )
    .unwrap();
    for _ in 0..20 {
        drive_manager_cycle(&e, true, false);
        if e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.r#type == "controller_tool_call" && r.data["status"] == "prepared")
        {
            break;
        }
    }
    let before = e.store.snapshot().unwrap();
    let old = before.configuration(&controller).unwrap().data.clone();
    let old_sid = old["session_id"].as_str().unwrap().to_owned();
    assert!(
        before
            .executions
            .iter()
            .any(|x| x.session_id == old_sid && !x.writer_revoked)
    );
    assert!(
        cmd(
            &e,
            "orchestration.resume",
            Some(&controller),
            json!({"agent_ref":next_agent,"harness_permission_ref":next_context})
        )
        .is_err()
    );
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .configuration(&controller)
            .unwrap()
            .data,
        old
    );
    cmd(&e, "orchestration.takeover", Some(&controller), json!({})).unwrap();
    for _ in 0..5 {
        e.tick_conversations(4).unwrap();
    }
    let stopped = e.store.snapshot().unwrap();
    assert!(
        stopped
            .executions
            .iter()
            .filter(|x| x.session_id == old_sid)
            .all(|x| x.writer_revoked)
    );
    let reply = cmd(
        &e,
        "orchestration.resume",
        Some(&controller),
        json!({"agent_ref":next_agent,"harness_permission_ref":next_context}),
    )
    .unwrap();
    assert_eq!(reply["controller"]["agent_revision_id"], next_agent);
    assert_eq!(reply["controller"]["harness_permission_ref"], next_context);
    assert_eq!(reply["controller"]["policy"]["timeout_seconds"], 1800);
    assert_eq!(reply["controller"]["predecessor_session_id"], old_sid);
    let successor = reply["id"].as_str().unwrap().to_owned();
    assert_ne!(successor, controller);
    e.drive_controllers().unwrap();
    let after = e.store.snapshot().unwrap();
    assert_eq!(
        after.configuration(&controller).unwrap().data["status"],
        "closed"
    );
    assert_eq!(
        after.configuration(&controller).unwrap().data["policy"],
        old["policy"]
    );
    assert_eq!(
        after
            .configuration(&format!("workflow-driver:{root}"))
            .unwrap()
            .data["controller_id"],
        successor
    );
    assert!(
        runtime(&e, &root)["runtime"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["status"] == "queued")
    );
    assert_eq!(
        after
            .sessions
            .iter()
            .find(|x| x.id == old_sid)
            .unwrap()
            .agent,
        original.id
    );
    assert!(
        after
            .governance
            .iter()
            .any(|r| r.r#type == "controller_selection" && r.definition_id == successor)
    );
    assert!(
        after
            .governance
            .iter()
            .any(|r| r.r#type == "controller_tool_call" && r.data["epoch"] == old["epoch"])
    );
    assert!(cmd(&e, "orchestration.resume", Some(&controller), json!({})).is_err());
    for _ in 0..20 {
        drive_manager_cycle(&e, true, true);
        if runtime(&e, &root)["runtime"]["steps"][0]["status"] != "queued" {
            break;
        }
    }
    let effective = e.store.snapshot().unwrap();
    assert!(
        effective
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_decision"
                && r.data["operation"] == "dispatch"
                && r.data["actor"]["controller_id"] == successor)
    );
    drop(e);
    let e = engine(dir.path());
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .configuration(&successor)
            .unwrap()
            .data["policy"]["timeout_seconds"],
        1800
    );
}

fn rust_policy(_e: &Engine, max_actions: usize) -> Value {
    json!({"id":{"max_concurrency":2,"max_actions_per_tick":max_actions,"lease_seconds":60,"max_retries":0,"retry_error_codes":[]}})["id"].clone()
}

#[test]
fn three_business_checkpoints_block_successors_and_rework_requires_a_new_confirmation() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let keys = ["prd_review", "design_review", "tasks_review", "implement"];
    let steps: Vec<_> = keys
        .iter()
        .enumerate()
        .map(|(index, key)| {
            json!({"key":key,"name":key,"stage":"work","executor":{"type":"control"},
            "dependencies":if index == 0 {vec![]} else {vec![keys[index-1]]},
            "confirmation":if index < 3 {"human"} else {"automatic"}})
        })
        .collect();
    let root = start(
        &e,
        json!({"schema_version":6,"stages":[{"key":"work","name":"关键业务阶段"}],"steps":steps}),
    );
    let policy = rust_policy(&e, 16);
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"rust","policy_ref":policy}),
    )
    .unwrap();
    let mut first_confirmation = Value::Null;
    for (index, key) in keys[..3].iter().enumerate() {
        for _ in 0..5 {
            e.drive_rust_workflows().unwrap();
            e.evaluate_workflows().unwrap();
        }
        let view = runtime(&e, &root);
        let execution = &view["runtime"]["steps"][index];
        assert_eq!(execution["status"], "waiting_confirmation");
        assert_eq!(view["runtime"]["steps"][index + 1]["status"], "queued");
        let args = json!({"step":key,"execution_id":execution["id"],"input_digest":execution["input_digest"],"outputs":execution["outputs"]});
        assert!(cmd(&e, "workflow.step.accept", Some(&root), args.clone()).is_err());
        cmd(&e, "workflow.step.confirm", Some(&root), args.clone()).unwrap();
        if index == 0 {
            first_confirmation = args;
        }
    }
    // Before implementation is dispatched, rework the PRD and invalidate all later decisions.
    cmd(
        &e,
        "workflow.step.rework",
        Some(&root),
        json!({"step":keys[0]}),
    )
    .unwrap();
    for _ in 0..5 {
        e.drive_rust_workflows().unwrap();
        e.evaluate_workflows().unwrap();
    }
    let view = runtime(&e, &root);
    assert_eq!(
        view["runtime"]["steps"][0]["status"],
        "waiting_confirmation"
    );
    assert_ne!(
        view["runtime"]["steps"][0]["id"],
        first_confirmation["execution_id"]
    );
    assert_eq!(view["steps"][0]["human_satisfied"], false);
    assert!(
        view["runtime"]["steps"].as_array().unwrap()[1..]
            .iter()
            .all(|step| step["status"] == "queued")
    );
    assert!(
        cmd(
            &e,
            "workflow.step.confirm",
            Some(&root),
            first_confirmation.clone()
        )
        .is_err()
    );
    assert!(cmd(&e, "workflow.step.accept", Some(&root), first_confirmation).is_err());
    // The guard survives reopening the actual SQLite dataset.
    drop(e);
    let e = engine(directory.path());
    e.drive_rust_workflows().unwrap();
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][0]["status"],
        "waiting_confirmation"
    );
}
#[test]
fn rust_driver_completes_control_graph_and_idle_ticks_have_no_business_writes() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let root = start(&e, document());
    let policy = rust_policy(&e, 16);
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"rust","policy_ref":policy}),
    )
    .unwrap();
    for _ in 0..8 {
        e.drive_rust_workflows().unwrap();
        e.evaluate_workflows().unwrap();
    }
    let s = e.store.snapshot().unwrap();
    assert_eq!(s.configuration(&root).unwrap().data["status"], "completed");
    assert!(
        s.governance
            .iter()
            .any(|r| r.r#type == "workflow_decision" && r.data["actor"]["kind"] == "driver")
    );
    let before = serde_json::to_value(&s).unwrap();
    e.drive_rust_workflows().unwrap();
    let after = serde_json::to_value(e.store.snapshot().unwrap()).unwrap();
    assert_eq!(before, after);
    drop(e);
    let e = engine(d.path());
    e.drive_rust_workflows().unwrap();
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .configuration(&root)
            .unwrap()
            .data["status"],
        "completed"
    );
}
#[test]
fn rust_driver_waits_for_human_confirmation_and_switch_preserves_execution() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let mut doc = document();
    doc["steps"][0]["confirmation"] = json!("human");
    let root = start(&e, doc);
    let policy = rust_policy(&e, 1);
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"rust","policy_ref":policy}),
    )
    .unwrap();
    e.drive_rust_workflows().unwrap();
    e.evaluate_workflows().unwrap();
    for _ in 0..3 {
        e.drive_rust_workflows().unwrap();
        e.evaluate_workflows().unwrap();
    }
    let before = runtime(&e, &root);
    let execution = &before["runtime"]["steps"][0];
    assert!(
        ["waiting_confirmation", "awaiting_acceptance"]
            .contains(&execution["status"].as_str().unwrap())
    );
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"manual"}),
    )
    .unwrap();
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"],
        before["runtime"]["steps"]
    );
    e.drive_rust_workflows().unwrap();
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"],
        before["runtime"]["steps"]
    );
    cmd(&e,"workflow.step.confirm",Some(&root),json!({"step":"one","execution_id":execution["id"],"input_digest":execution["input_digest"],"outputs":execution["outputs"]})).unwrap();
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"rust","policy_ref":policy}),
    )
    .unwrap();
    for _ in 0..8 {
        e.drive_rust_workflows().unwrap();
        e.evaluate_workflows().unwrap();
    }
    assert_eq!(runtime(&e, &root)["runtime"]["status"], "completed");
}
#[test]
fn rust_driver_drives_child_instances_and_does_not_dispatch_while_paused() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let draft = cmd(
        &e,
        "workflow.create",
        None,
        json!({"name":"child","document":document()}),
    )
    .unwrap();
    let child = cmd(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let root = start(
        &e,
        json!({"schema_version":6,"stages":[{"key":"work","name":"parent"}],"steps":[{"key":"call","name":"child invocation","stage":"work","executor":{"type":"workflow","workflow_ref":child["id"]}}]}),
    );
    let policy = rust_policy(&e, 16);
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"rust","policy_ref":policy}),
    )
    .unwrap();
    cmd(&e, "workflow.pause", Some(&root), json!({})).unwrap();
    e.drive_rust_workflows().unwrap();
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][0]["status"],
        "queued"
    );
    cmd(&e, "workflow.resume", Some(&root), json!({})).unwrap();
    for _ in 0..20 {
        e.drive_rust_workflows().unwrap();
        e.evaluate_workflows().unwrap();
    }
    assert_eq!(runtime(&e, &root)["runtime"]["status"], "completed");
    let s = e.store.snapshot().unwrap();
    let children: Vec<_> = s
        .governance
        .iter()
        .filter(|r| {
            r.r#type == "workflow_instance" && r.id != root && r.data["root_instance_id"] == root
        })
        .collect();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].data["status"], "completed");
    assert_eq!(
        s.governance
            .iter()
            .filter(|r| r.r#type == "workflow_driver")
            .count(),
        1
    );
}
#[test]
fn switching_agent_to_rust_rejects_already_prepared_native_calls() {
    let d = tempfile::tempdir().unwrap();
    let mut e = engine(d.path());
    let harness = Arc::new(NativeManager::default());
    e.harness = harness;
    let root = start(&e, document());
    cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10","policy":WorkflowControlPolicy::default()})).unwrap();
    for _ in 0..20 {
        drive_manager_cycle(&e, true, false);
        if e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.r#type == "controller_tool_call" && r.data["status"] == "prepared")
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
        .find(|r| r.r#type == "controller_tool_call" && r.data["status"] == "prepared")
        .unwrap();
    let policy = rust_policy(&e, 16);
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"rust","policy_ref":policy}),
    )
    .unwrap();
    e.drive_controllers().unwrap();
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .configuration(&call.id)
            .unwrap()
            .data["status"],
        "failed"
    );
    for _ in 0..8 {
        e.drive_rust_workflows().unwrap();
        e.evaluate_workflows().unwrap();
    }
    assert_eq!(runtime(&e, &root)["runtime"]["status"], "completed");
    let s = e.store.snapshot().unwrap();
    assert_eq!(
        s.governance
            .iter()
            .filter(|r| r.r#type == "workflow_dispatch")
            .count(),
        2
    );
}

#[test]
fn foreach_capacity_is_shared_by_inspection_admission_and_rust_selection() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let item = cmd(
        &e,
        "delivery_type.create",
        None,
        json!({"name":"item","spec_refs":["fixture-spec:1"],"data":{"format":"text"}}),
    )
    .unwrap();
    let items = cmd(&e, "delivery_type.create", None, json!({"name":"items","spec_refs":["fixture-spec:1"],"data":{"format":"json","content_schema":{"type":"array","items":{"type":"string"}}}})).unwrap();
    let mut child_doc = document();
    child_doc["inputs"] = json!({"item":{"delivery_ref":item["id"]}});
    let draft = cmd(
        &e,
        "workflow.create",
        None,
        json!({"name":"child","document":child_doc}),
    )
    .unwrap();
    let child = cmd(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let parent = json!({"schema_version":6,"stages":[{"key":"work","name":"work"}],"inputs":{"items":{"delivery_ref":items["id"]}},"steps":[{"key":"members","name":"members","stage":"work","executor":{"type":"workflow","workflow_ref":child["id"],"foreach":{"source":{"step":"$input","port":"items"},"item_input":"item","max_concurrency":1,"min_items":2}}}]});
    let project = cmd(
        &e,
        "project.create",
        None,
        json!({"title":"capacity","workspace":"/tmp/fixture"}),
    )
    .unwrap();
    let draft = cmd(
        &e,
        "workflow.create",
        None,
        json!({"name":"parent","document":parent}),
    )
    .unwrap();
    let workflow = cmd(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let root = cmd(&e,"workflow.start",project["id"].as_str(),json!({"workflow_ref":workflow["id"],"role_agent_bindings":{},"inputs":{"items":["A","B"]}})).unwrap()["id"].as_str().unwrap().to_owned();
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&root),
        json!({"step":"members"}),
    )
    .unwrap();
    e.evaluate_workflows().unwrap();
    let state = e.store.snapshot().unwrap();
    let mut children: Vec<_> = state
        .governance
        .iter()
        .filter(|r| {
            r.r#type == "workflow_instance" && r.id != root && r.data["root_instance_id"] == root
        })
        .map(|r| r.id.clone())
        .collect();
    children.sort();
    assert_eq!(children.len(), 2);
    let blocked = &children[0];
    let running = &children[1];
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(running),
        json!({"step":"one"}),
    )
    .unwrap();
    e.evaluate_workflows().unwrap();
    let observed = runtime(&e, blocked);
    assert_eq!(observed["steps"][0]["dispatchable"], false);
    assert_eq!(
        observed["steps"][0]["actions"]["dispatch"]["code"],
        "foreach_capacity"
    );
    assert!(
        !observed["allowed_actions"]
            .as_array()
            .unwrap()
            .contains(&json!("dispatch"))
    );
    assert_eq!(
        cmd(
            &e,
            "workflow.step.dispatch",
            Some(blocked),
            json!({"step":"one"})
        )
        .unwrap_err()
        .code,
        "blocked"
    );
    let policy = rust_policy(&e, 16);
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"rust","policy_ref":policy}),
    )
    .unwrap();
    for _ in 0..24 {
        e.drive_rust_workflows().unwrap();
        e.evaluate_workflows().unwrap();
    }
    assert_eq!(runtime(&e, &root)["runtime"]["status"], "completed");
    for child in &children {
        assert_eq!(runtime(&e, child)["runtime"]["status"], "completed");
    }
    let state = e.store.snapshot().unwrap();
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_dispatch")
            .count(),
        5
    );
    drop(e);
    let e = engine(dir.path());
    e.drive_rust_workflows().unwrap();
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_dispatch")
            .count(),
        5
    );
}

#[test]
fn an_independent_result_progresses_while_another_step_waits_for_a_human() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let mut doc = document();
    doc["steps"][0]["confirmation"] = json!("human");
    doc["steps"][1]["dependencies"] = json!([]);
    let root = start(&e, doc);
    let policy = rust_policy(&e, 16);
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"rust","policy_ref":policy}),
    )
    .unwrap();
    for _ in 0..8 {
        e.drive_rust_workflows().unwrap();
        e.evaluate_workflows().unwrap();
    }
    let view = runtime(&e, &root);
    assert_ne!(view["runtime"]["steps"][0]["status"], "completed");
    assert_eq!(view["runtime"]["steps"][1]["status"], "completed");
    assert_eq!(
        view["steps"][0]["actions"]["accept"]["code"],
        "human_confirmation"
    );
    let state = e.store.snapshot().unwrap();
    assert!(
        !state
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_confirmation")
    );
    let before = state
        .governance
        .iter()
        .filter(|r| r.r#type == "workflow_dispatch" || r.r#type == "workflow_decision")
        .count();
    for _ in 0..3 {
        e.drive_rust_workflows().unwrap();
        e.evaluate_workflows().unwrap();
    }
    let state = e.store.snapshot().unwrap();
    assert_eq!(
        before,
        state
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_dispatch" || r.r#type == "workflow_decision")
            .count()
    );
}

fn recovery_root(e: &Engine, effect: &str) -> (String, String) {
    let mut doc = document();
    doc["stages"][0]["default_role_ref"] = json!("builtin-role-analyst:1");
    doc["steps"][0]["executor"] = json!({"type":"agent"});
    doc["steps"][0]["task_instructions"] = json!("Return the declared result for the frozen input");
    doc["scheduling"] = json!({"max_concurrency":2,"max_actions_per_tick":16,"lease_seconds":60,"max_retries":1,"retry_error_codes":["fixture_transient"]});
    let root = start(e, doc);
    cmd(
        e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"rust"}),
    )
    .unwrap();
    e.drive_rust_workflows().unwrap();
    let pending = e.pending_workflow_intents().unwrap();
    let intent = pending
        .iter()
        .find(|r| r.data["method"] == "session.open")
        .unwrap();
    let failure: Error=serde_json::from_value(json!({"code":"fixture_transient","message":"Explicit port failure; no input applied","execution_failure":{"kind":"technical","effect":effect}})).unwrap();
    e.observe_workflow_intent(&intent.id, &Err(failure))
        .unwrap();
    e.evaluate_workflows().unwrap();
    (root, intent.id.clone())
}
#[test]
fn rust_retries_only_known_technical_failures_with_a_durable_source_and_budget() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let (root, source) = recovery_root(&e, "not_applied");
    let before = runtime(&e, &root)["runtime"]["steps"][0].clone();
    assert_eq!(before["failure"]["code"], "fixture_transient");
    assert_eq!(before["failure"]["source_ref"], source);
    e.drive_rust_workflows().unwrap();
    e.evaluate_workflows().unwrap();
    let after = runtime(&e, &root)["runtime"]["steps"][0].clone();
    assert_ne!(before["id"], after["id"]);
    assert_eq!(before["inputs"], after["inputs"]);
    assert_eq!(before["input_digest"], after["input_digest"]);
    assert_eq!(after["sequence"], 1);
    let next = e
        .pending_workflow_intents()
        .unwrap()
        .into_iter()
        .find(|r| r.data["method"] == "session.open")
        .unwrap();
    let failure:Error=serde_json::from_value(json!({"code":"fixture_transient","message":"again","execution_failure":{"kind":"technical","effect":"not_applied"}})).unwrap();
    e.observe_workflow_intent(&next.id, &Err(failure)).unwrap();
    e.evaluate_workflows().unwrap();
    e.drive_rust_workflows().unwrap();
    let failed = runtime(&e, &root)["runtime"]["steps"][0].clone();
    assert_eq!(failed["id"], after["id"]);
    drop(e);
    let e = engine(dir.path());
    e.drive_rust_workflows().unwrap();
    assert_eq!(runtime(&e, &root)["runtime"]["steps"][0]["id"], after["id"]);
    let state = e.store.snapshot().unwrap();
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_execution_history" && r.data["step"] == "one")
            .count(),
        1
    );
    assert_eq!(
        state.configuration(&source).unwrap().data["error"]["code"],
        "fixture_transient"
    );
}
#[test]
fn an_unknown_effect_is_not_retried_even_when_its_code_is_in_policy() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let (root, _) = recovery_root(&e, "unknown");
    let before = runtime(&e, &root)["runtime"]["steps"][0].clone();
    assert_eq!(before["failure"]["effect"], "unknown");
    for _ in 0..4 {
        e.drive_rust_workflows().unwrap();
        e.evaluate_workflows().unwrap();
    }
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][0]["id"],
        before["id"]
    );
    assert!(e.pending_workflow_intents().unwrap().is_empty());
}

#[test]
fn tool_preparation_recovers_through_the_same_known_failure_policy() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let root = start(
        &e,
        json!({"schema_version":6,"stages":[{"key":"work","name":"work"}],"steps":[{"key":"capture","name":"capture","stage":"work","executor":{"type":"tool","tool_ref":"builtin-tool-materials-capture:1","arguments":{"material":{"type":"constant","value":{"data":"frozen input"}}}}}]}),
    );
    let policy = json!({"id":{"max_concurrency":2,"max_actions_per_tick":16,"lease_seconds":60,"max_retries":1,"retry_error_codes":["fixture_preparation_failed"]}});
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"rust","policy_ref":policy["id"]}),
    )
    .unwrap();
    e.drive_rust_workflows().unwrap();
    let failed = e
        .pending_workflow_intents()
        .unwrap()
        .into_iter()
        .find(|r| r.data["method"] == "workflow.tool.execute")
        .unwrap();
    let error = err(
        "fixture_preparation_failed",
        "explicit preparation port rejected before enqueue",
    )
    .technical_not_applied();
    e.observe_workflow_intent(&failed.id, &Err(error.clone()))
        .unwrap();
    e.evaluate_workflows().unwrap();
    let before = runtime(&e, &root)["runtime"]["steps"][0].clone();
    e.drive_rust_workflows().unwrap();
    let next = e
        .pending_workflow_intents()
        .unwrap()
        .into_iter()
        .find(|r| r.data["method"] == "workflow.tool.execute")
        .unwrap();
    assert_ne!(failed.definition_id, next.definition_id);
    let command: Command = serde_json::from_value(next.data["command"].clone()).unwrap();
    let request = e.configured_platform_request(&command).unwrap().unwrap();
    let result = e.enqueue_platform_tool(&command, &request);
    e.observe_workflow_intent(&next.id, &result).unwrap();
    e.drive_platform_tools().unwrap();
    for _ in 0..4 {
        e.evaluate_workflows().unwrap();
        e.drive_rust_workflows().unwrap();
    }
    assert_eq!(runtime(&e, &root)["runtime"]["status"], "completed");
    let state = e.store.snapshot().unwrap();
    assert_eq!(
        state.configuration(&failed.id).unwrap().data["error"],
        json!(error)
    );
    let history = state
        .governance
        .iter()
        .find(|r| r.r#type == "workflow_execution_history" && r.definition_id == before["id"])
        .unwrap();
    assert_eq!(history.data["failure"]["source_ref"], failed.id);
    assert!(
        e.observe_workflow_intent(&failed.id, &Ok(json!({"id":"late-fabricated-result"})))
            .is_err()
    );
}

#[test]
fn an_unknown_worker_reserves_its_root_capacity_until_reconciliation() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let mut doc = document();
    doc["stages"][0]["default_role_ref"] = json!("builtin-role-analyst:1");
    for step in doc["steps"].as_array_mut().unwrap() {
        step["executor"] = json!({"type":"agent"});
        step["task_instructions"] = json!("Respect the frozen task");
        step["dependencies"] = json!([]);
    }
    let root = start(&e, doc);
    let policy = json!({"id":{"max_concurrency":1,"max_actions_per_tick":16,"lease_seconds":60,"max_retries":1,"retry_error_codes":["fixture_unknown"]}});
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"rust","policy_ref":policy["id"]}),
    )
    .unwrap();
    e.drive_rust_workflows().unwrap();
    let first = e.pending_workflow_intents().unwrap();
    assert_eq!(first.len(), 1);
    e.observe_workflow_intent(
        &first[0].id,
        &Err(err("fixture_unknown", "outcome unavailable").technical_unknown()),
    )
    .unwrap();
    e.evaluate_workflows().unwrap();
    for _ in 0..3 {
        e.drive_rust_workflows().unwrap();
        e.evaluate_workflows().unwrap();
    }
    let view = runtime(&e, &root);
    assert_eq!(
        view["runtime"]["steps"][0]["status"],
        "needs_reconciliation"
    );
    assert_eq!(view["runtime"]["steps"][1]["status"], "queued");
    assert!(e.pending_workflow_intents().unwrap().is_empty());
}

#[test]
fn task_dependencies_block_dispatch_without_starving_independent_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let anchor = start(&e, document());
    let project = runtime(&e, &anchor)["runtime"]["project_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let feature = id();
    let tasks = [id(), id(), id()];
    // Business records are fixtures; dispatch, inspection and completion use production commands.
    e.store
        .transact(None, &mut |state| {
            state.features.push(Feature {
                id: feature.clone(),
                project_id: project.clone(),
                title: "three tasks".into(),
                status: Status::Draft,
                revision: 1,
                input_version: 1,
                predecessor: None,
                source_materials: vec![],
            });
            for (index, task) in tasks.iter().enumerate() {
                state.tasks.push(Task {
                    id: task.clone(),
                    feature_id: feature.clone(),
                    title: format!("Task {index}"),
                    status: Status::Draft,
                    revision: 1,
                    inherited_snapshot: None,
                    dependencies: if index == 1 {
                        vec![tasks[0].clone()]
                    } else {
                        vec![]
                    },
                    worktree: String::new(),
                    scope: TaskScope::default(),
                });
            }
            Ok(Value::Null)
        })
        .unwrap();
    let text_delivery = cmd(
        &e,
        "delivery_type.create",
        None,
        json!({"name":"task id","spec_refs":["fixture-spec:1"],"data":{"format":"text"}}),
    )
    .unwrap();
    let draft = cmd(&e,"workflow.create",None,json!({"name":"task completion","document":{"schema_version":300,"stages":[{"key":"work","name":"work"}],"steps":[{"key":"finish","name":"finish","stage":"work","executor":{"type":"tool","tool_ref":"builtin-tool-tasks-complete:1","arguments":{"task_id":{"type":"source","source":{"step":"$input","port":"task"}}}}}],"inputs":{"task":{"delivery_ref":text_delivery["id"]}}}})).unwrap();
    let workflow = cmd(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let mut roots = vec![];
    for task in &tasks {
        let root=e.execute("workflow.start",Command {command_id:id(),target:Some(task.clone()),expected_revision:Some(1),payload:json!({"workflow_ref":workflow["id"],"role_agent_bindings":{},"inputs":{"task":task}})}).unwrap()["id"].as_str().unwrap().to_owned();
        roots.push(root);
    }
    let blocked = runtime(&e, &roots[1]);
    assert_eq!(
        blocked["steps"][0]["actions"]["dispatch"]["code"],
        "task_dependencies"
    );
    assert!(
        cmd(
            &e,
            "workflow.step.dispatch",
            Some(&roots[1]),
            json!({"step":"finish"})
        )
        .is_err()
    );
    let policy = rust_policy(&e, 16);
    // B is deliberately selected first. A remains manual while independent C completes.
    for index in [1, 2] {
        cmd(
            &e,
            "workflow.driver.select",
            Some(&roots[index]),
            json!({"mode":"rust","policy_ref":policy}),
        )
        .unwrap();
    }
    for _ in 0..8 {
        e.drive_rust_workflows().unwrap();
        for intent in e.pending_workflow_intents().unwrap() {
            let command: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
            let request = e.configured_platform_request(&command).unwrap().unwrap();
            let result = e.enqueue_platform_tool(&command, &request);
            e.observe_workflow_intent(&intent.id, &result).unwrap();
        }
        e.drive_platform_tools().unwrap();
        e.evaluate_workflows().unwrap();
    }
    assert_eq!(runtime(&e, &roots[2])["runtime"]["status"], "completed");
    assert_eq!(
        runtime(&e, &roots[1])["runtime"]["steps"][0]["status"],
        "queued"
    );
    cmd(
        &e,
        "workflow.driver.select",
        Some(&roots[0]),
        json!({"mode":"rust","policy_ref":policy}),
    )
    .unwrap();
    for _ in 0..16 {
        e.drive_rust_workflows().unwrap();
        for intent in e.pending_workflow_intents().unwrap() {
            let command: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
            let request = e.configured_platform_request(&command).unwrap().unwrap();
            let result = e.enqueue_platform_tool(&command, &request);
            e.observe_workflow_intent(&intent.id, &result).unwrap();
        }
        e.drive_platform_tools().unwrap();
        e.evaluate_workflows().unwrap();
    }
    for root in &roots {
        assert_eq!(runtime(&e, root)["runtime"]["status"], "completed");
    }
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .tasks
            .iter()
            .all(|t| t.status == Status::Completed)
    );
}

#[test]
fn one_root_schedules_three_bound_tasks_without_a_dependent_member_taking_a_slot() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let anchor = start(&e, document());
    let project = runtime(&e, &anchor)["runtime"]["project_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let feature = id();
    let tasks = [id(), id(), id()];
    e.store
        .transact(None, &mut |state| {
            state.features.push(Feature {
                id: feature.clone(),
                project_id: project.clone(),
                title: "bound tasks".into(),
                status: Status::Draft,
                revision: 1,
                input_version: 1,
                predecessor: None,
                source_materials: vec![],
            });
            for (index, task) in tasks.iter().enumerate() {
                state.tasks.push(Task {
                    id: task.clone(),
                    feature_id: feature.clone(),
                    title: format!("Task {index}"),
                    status: Status::Draft,
                    revision: 1,
                    inherited_snapshot: None,
                    dependencies: if index == 1 {
                        vec![tasks[0].clone()]
                    } else {
                        vec![]
                    },
                    worktree: String::new(),
                    scope: TaskScope::default(),
                });
            }
            Ok(Value::Null)
        })
        .unwrap();
    let item=cmd(&e,"delivery_type.create",None,json!({"name":"bound item","spec_refs":["fixture-spec:1"],"data":{"format":"json","content_schema":{"type":"object"}}})).unwrap();
    let items=cmd(&e,"delivery_type.create",None,json!({"name":"bound items","spec_refs":["fixture-spec:1"],"data":{"format":"json","content_schema":{"type":"array","items":{"type":"object"}}}})).unwrap();
    let draft=cmd(&e,"workflow.create",None,json!({"name":"bound child","document":{"schema_version":900,"stages":[{"key":"work","name":"work"}],"inputs":{"item":{"delivery_ref":item["id"]}},"steps":[
        {"key":"gate","name":"A waits for a Human","stage":"work","executor":{"type":"control"},"condition":{"op":"equals","source":{"step":"$input","port":"item","pointer":"/key"},"value":"A"},"confirmation":"human"},
        {"key":"finish","name":"complete Task","stage":"work","dependencies":["gate"],"executor":{"type":"tool","tool_ref":"builtin-tool-tasks-complete:1","arguments":{"task_id":{"type":"source","source":{"step":"$input","port":"item","pointer":"/id"}}}}}
    ]}})).unwrap();
    let child = cmd(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let draft=cmd(&e,"workflow.create",None,json!({"name":"three bound Tasks","document":{"schema_version":901,"stages":[{"key":"work","name":"work"}],"inputs":{"items":{"delivery_ref":items["id"]}},"steps":[{"key":"members","name":"members","stage":"work","executor":{"type":"workflow","workflow_ref":child["id"],"work_binding":{"input":"item","pointer":"/work_ref"},"foreach":{"source":{"step":"$input","port":"items"},"item_input":"item","max_concurrency":2,"min_items":3}}}]}})).unwrap();
    let parent = cmd(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let members:Vec<_>=[1,0,2].into_iter().map(|index|json!({"key":(["A","B","C"][index]),"id":tasks[index],"work_ref":{"kind":"task","id":tasks[index]}})).collect();
    let root=e.execute("workflow.start",Command {command_id:id(),target:Some(feature.clone()),expected_revision:Some(1),payload:json!({"workflow_ref":parent["id"],"role_agent_bindings":{},"inputs":{"items":members}})}).unwrap()["id"].as_str().unwrap().to_owned();
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&root),
        json!({"step":"members"}),
    )
    .unwrap();
    e.evaluate_workflows().unwrap();
    let state = e.store.snapshot().unwrap();
    let children: Vec<_> = tasks
        .iter()
        .map(|task| {
            state
                .governance
                .iter()
                .find(|row| {
                    row.r#type == "workflow_instance"
                        && row.data["root_instance_id"] == root
                        && row.data["owner_id"] == *task
                })
                .unwrap()
                .id
                .clone()
        })
        .collect();
    cmd(
        &e,
        "workflow.step.dispatch",
        Some(&children[0]),
        json!({"step":"gate"}),
    )
    .unwrap();
    e.evaluate_workflows().unwrap();
    let policy = json!({"id":{"max_concurrency":1,"max_actions_per_tick":16,"lease_seconds":60,"max_retries":0,"retry_error_codes":[]}});
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"rust","policy_ref":policy["id"]}),
    )
    .unwrap();
    let tick = || {
        e.drive_rust_workflows().unwrap();
        for intent in e.pending_workflow_intents().unwrap() {
            let command: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
            let request = e.configured_platform_request(&command).unwrap().unwrap();
            let result = e.enqueue_platform_tool(&command, &request);
            e.observe_workflow_intent(&intent.id, &result).unwrap();
        }
        e.drive_platform_tools().unwrap();
        e.evaluate_workflows().unwrap();
    };
    for _ in 0..12 {
        tick();
    }
    assert_eq!(
        runtime(&e, &children[0])["runtime"]["steps"][0]["status"],
        "waiting_confirmation"
    );
    assert_eq!(
        runtime(&e, &children[1])["steps"][0]["actions"]["dispatch"]["code"],
        "task_dependencies"
    );
    assert_eq!(runtime(&e, &children[2])["runtime"]["status"], "completed");
    assert!(
        runtime(&e, &children[1])["runtime"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|step| step["status"] == "queued")
    );
    let view = runtime(&e, &children[0]);
    let inbox = e.workflow_driver_query(&json!({"id":root})).unwrap();
    assert!(
        inbox["approvals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["instance_id"] == children[0] && a["owner_id"] == tasks[0])
    );
    assert!(
        runtime(&e, &root)["human_gates"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["instance_id"] == children[0])
    );
    let gate = &view["runtime"]["steps"][0];
    cmd(&e,"workflow.step.confirm",Some(&children[0]),json!({"step":"gate","input_digest":gate["input_digest"],"outputs":gate["outputs"],"reason":"Fixture Human signs the exact waiting gate; no completion override"})).unwrap();
    for _ in 0..20 {
        tick();
    }
    assert_eq!(runtime(&e, &root)["runtime"]["status"], "completed");
    for child in &children {
        assert_eq!(runtime(&e, child)["runtime"]["status"], "completed");
    }
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .tasks
            .iter()
            .all(|task| task.status == Status::Completed)
    );
}

#[test]
fn scheduling_adoption_updates_driver_and_budget_from_the_new_workflow_revision() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let root = start(&e, document());
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"rust"}),
    )
    .unwrap();
    configuration_fixture::scheduling(&e,&root,&json!({"max_concurrency":1,"max_actions_per_tick":2,"agent":{"max_turns_per_wakeup":3,"max_tool_calls_per_turn":12,"timeout_seconds":90}})).unwrap();
    let view = e.workflow_driver_query(&json!({"id":root})).unwrap();
    assert_eq!(view["driver"]["data"]["policy"]["max_actions_per_tick"], 2);
    assert_eq!(
        view["driver"]["data"]["policy"]["agent"]["max_turns_per_wakeup"],
        3
    );
}

#[test]
fn native_manager_prepares_human_request_waits_and_resumes_from_the_decision_alone() {
    let directory = tempfile::tempdir().unwrap();
    let mut e = engine(directory.path());
    e.harness = Arc::new(NativeManager {
        human: true,
        ..Default::default()
    });
    let mut doc = document();
    doc["steps"][0]["confirmation"] = json!("human");
    let root = start(&e, doc);
    cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-sol-high-project-manager:10"})).unwrap();
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"agent"}),
    )
    .unwrap();
    for _ in 0..30 {
        drive_manager(&e);
        if e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_controller" && r.data["run_state"] == "waiting")
        {
            break;
        }
    }
    let state = e.store.snapshot().unwrap();
    let request = state
        .governance
        .iter()
        .find(|r| r.r#type == "workflow_pending_decision")
        .unwrap();
    assert_eq!(request.data["actor"]["kind"], "controller");
    assert_eq!(
        request.data["request"]["interaction"]["title"],
        "确认夹具控制结果"
    );
    let waiting = runtime(&e, &root);
    assert_eq!(
        waiting["runtime"]["steps"][0]["status"],
        "waiting_confirmation"
    );
    assert_eq!(waiting["runtime"]["steps"][1]["status"], "queued");
    assert!(
        !state
            .governance
            .iter()
            .any(|r| r.r#type == "controller_input")
    );
    cmd(
        &e,
        "workflow.decision.resolve",
        Some(&request.id),
        json!({"approved":true,"reason":"Explicit fixture Human signs this exact result"}),
    )
    .unwrap();
    for _ in 0..40 {
        drive_manager(&e);
        if runtime(&e, &root)["runtime"]["status"] == "completed" {
            break;
        }
    }
    assert_eq!(runtime(&e, &root)["runtime"]["status"], "completed");
    let final_state = e.store.snapshot().unwrap();
    assert!(
        !final_state
            .governance
            .iter()
            .any(|r| r.r#type == "controller_input")
    );
    assert!(
        final_state
            .governance
            .iter()
            .any(|r| r.r#type == "controller_tool_call"
                && r.data["method"] == "workflow.controller.wait"
                && r.data["status"] == "succeeded")
    );
    for row in final_state
        .governance
        .iter()
        .filter(|r| r.r#type == "workflow_decision")
    {
        if row.data["operation"] == "start"
            || (row.data["operation"] == "accept"
                && row.data["execution_id"] == waiting["runtime"]["steps"][0]["id"])
        {
            assert_eq!(row.data["actor"]["kind"], "human");
            continue;
        }
        assert_eq!(row.data["actor"]["kind"], "controller");
    }
}
