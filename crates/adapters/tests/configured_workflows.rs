#[path = "../../../tests/support/control.rs"]
mod control_fixture;
// Configuration-driven execution through actual SQLite and Artifact storage.
#[path = "../../../tests/support/defaults.rs"]
mod default_fixture;
#[path = "../../../tests/support/schemas.rs"]
mod schemas;
use default_fixture::*;
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
use devdesk_adapters::integrations::NativeTools;
use devdesk_adapters::{FileArtifacts, SqliteStore};
use devdesk_core::ports::ToolExecutor;
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

fn engine(path: &std::path::Path) -> Engine {
    let engine = Engine::new(
        Arc::new(SqliteStore::open(path).unwrap()),
        Arc::new(FileArtifacts::new(path.into())),
        runtime::adapters(),
    )
    .with_tool_executor(Arc::new(NativeTools::new(path).unwrap()));
    engine.bootstrap().unwrap();
    runtime::bind_default_permissions(&engine);
    engine
}
fn command(engine: &Engine, method: &str, target: Option<&str>, payload: Value) -> Result<Value> {
    let state = engine.store.snapshot()?;
    let revision = target.and_then(|target| {
        state
            .configuration(target)
            .ok()
            .map(|r| r.revision)
            .or_else(|| {
                state
                    .projects
                    .iter()
                    .find(|p| p.id == target)
                    .map(|p| p.revision)
            })
            .or_else(|| {
                state
                    .features
                    .iter()
                    .find(|f| f.id == target)
                    .map(|f| f.revision)
            })
            .or_else(|| {
                state
                    .tasks
                    .iter()
                    .find(|t| t.id == target)
                    .map(|t| t.revision)
            })
    });
    engine.execute(
        method,
        Command {
            command_id: id(),
            target: target.map(str::to_owned),
            expected_revision: revision,
            payload,
        },
    )
}
fn publish(engine: &Engine, document: Value) -> Value {
    let draft = command(
        engine,
        "workflow.create",
        None,
        json!({"name":"自定义业务","document":document}),
    )
    .unwrap();
    command(engine, "workflow.publish", draft["id"].as_str(), json!({})).unwrap()
}

#[test]
fn custom_role_and_typed_business_step_publish_preview_and_reload() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let role=command(&engine,"role.create",None,json!({"name":"安全研究员","data":{"instructions":"调查输入材料的风险","visibility":{"kind":"global"}}})).unwrap();
    assert!(role["data"].get("execution_role").is_none());
    let output=command(&engine,"delivery_type.create",None,json!({"name":"风险清单","data":{"body":"本次发现的风险与依据","format":"json","content_schema":{
        "type":"object","required":["risks"],"properties":{"risks":{"type":"array","items":{"type":"string"}}},"additionalProperties":false}}})).unwrap();
    let document = json!({"schema_version":5,"stages":[{"key":"inquiry","name":"调查","default_role_ref":role["definition_id"]}],"steps":[{
        "key":"study","name":"识别风险","stage":"inquiry","executor":{"type":"agent"},"task_instructions":"按调研 Skill 分析并提交风险。",
        "outputs":{"risks":{"delivery_ref":output["id"]}}}]});
    let workflow = publish(&engine, document.clone());
    let preview = engine
        .query(
            "query.workflow_task_preview",
            &json!({"id":workflow["id"],"step":"study"}),
            4,
        )
        .unwrap();
    assert_eq!(
        preview["task"]["task_instructions"],
        "按调研 Skill 分析并提交风险。"
    );
    assert_eq!(
        preview["task"]["outputs"]["risks"]["definition"]["id"],
        output["id"]
    );
    let required = engine
        .query(
            "query.workflow_requirements",
            &json!({"id":workflow["id"]}),
            4,
        )
        .unwrap();
    assert_eq!(required["roles"][0]["role_ref"], role["definition_id"]);
    assert!(required["document"]["steps"][0].get("kind").is_none());
    let state = engine.store.snapshot().unwrap();
    assert_eq!(
        state
            .configuration(workflow["id"].as_str().unwrap())
            .unwrap()
            .data["steps"],
        document["steps"]
    );
    drop(engine);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    let reopened = loop {
        match SqliteStore::open(dir.path()) {
            Ok(store) => break store,
            Err(error)
                if error.code == "dataset_locked" && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            Err(error) => panic!("{error}"),
        }
    };
    let state = reopened.snapshot().unwrap();
    assert_eq!(
        state
            .configuration(role["id"].as_str().unwrap())
            .unwrap()
            .name,
        "安全研究员"
    );
}

#[test]
fn control_workflow_runs_and_confirmation_is_bound_to_exact_content() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let workflow = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"decision","name":"业务决定"}],"steps":[
        {"key":"check","name":"确认范围","stage":"decision","executor":{"type":"control"},"confirmation":"human"},
        {"key":"finish","name":"结束","stage":"decision","executor":{"type":"control"},"dependencies":["check"]}]}),
    );
    let project = command(
        &engine,
        "project.create",
        None,
        json!({"title":"配置运行","workspace":"."}),
    )
    .unwrap();
    let instance = command(
        &engine,
        "workflow.start",
        project["id"].as_str(),
        json!({"workflow_ref":workflow["id"],"role_agent_bindings":{}}),
    )
    .unwrap();
    schemas::SCHEMAS.assert_valid(
        "rpc.schema.json#/$defs/ConfiguredWorkflowInstance",
        &instance["data"],
    );
    let instance_id = instance["id"].as_str().unwrap();
    control_fixture::evaluate(&engine).unwrap();
    let row = engine
        .store
        .snapshot()
        .unwrap()
        .configuration(instance_id)
        .unwrap()
        .clone();
    assert_eq!(row.data["steps"][0]["status"], "waiting_confirmation");
    assert_eq!(row.data["steps"][1]["status"], "queued");
    assert!(
        command(
            &engine,
            "workflow.step.confirm",
            Some(instance_id),
            json!({"step":"check","input_digest":"forged","outputs":{}})
        )
        .is_err()
    );
    command(
        &engine,
        "workflow.step.confirm",
        Some(instance_id),
        json!({"step":"check","input_digest":row.data["steps"][0]["input_digest"],"outputs":{}}),
    )
    .unwrap();
    control_fixture::evaluate(&engine).unwrap();
    assert_eq!(
        engine
            .store
            .snapshot()
            .unwrap()
            .configuration(instance_id)
            .unwrap()
            .data["status"],
        "completed"
    );
}

#[test]
fn child_closure_rejects_wrong_type_and_recursive_calls_before_publish() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let kind = command(
        &engine,
        "delivery_type.create",
        None,
        json!({"name":"材料","data":{"body":"输入材料","format":"text"}}),
    )
    .unwrap();
    let child = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"work","name":"工作"}],"inputs":{"source":{"delivery_ref":kind["id"]}},
        "steps":[{"key":"gate","name":"检查材料","stage":"work","executor":{"type":"control"}}]}),
    );
    let parent = json!({"schema_version":5,"stages":[{"key":"work","name":"工作"}],"steps":[{"key":"nested","name":"调用","stage":"work",
        "executor":{"type":"workflow","workflow_ref":child["id"]}}]});
    let draft = command(
        &engine,
        "workflow.create",
        None,
        json!({"name":"缺少子流程输入","document":parent}),
    )
    .unwrap();
    assert!(command(&engine, "workflow.publish", draft["id"].as_str(), json!({})).is_err());
}

#[derive(Default)]
struct RecordingAgent {
    hold: std::sync::atomic::AtomicBool,
    responses: Mutex<Vec<Value>>,
    calls: Mutex<Vec<ports::TurnDispatch>>,
    pending: Mutex<HashMap<Id, Vec<ports::ConversationObservation>>>,
}
impl ports::Harness for RecordingAgent {
    fn identity(&self) -> String {
        "fixture:configured-agent".into()
    }
    fn open_session(&self, request: &ports::SessionStart) -> Result<ports::SessionReceipt> {
        let task = request
            .configured_task
            .as_ref()
            .expect("native start requires the actual configured task");
        task.validate()?;
        assert!(request.documents.is_empty());
        Ok(ports::SessionReceipt {
            external_session: format!("fixture-{}", request.session.id),
            container_id: "fixture-container".into(),
            image_digest: request.execution.image_digest.clone(),
            operation_id: request.execution.operation_id.clone(),
        })
    }
    fn dispatch_turn(&self, request: &ports::TurnDispatch) -> Result<()> {
        use ports::ConversationObservation as Event;
        assert_eq!(
            devdesk_adapters::harness::provider_turn_text(request)?,
            request.turn.input
        );
        self.calls.lock().unwrap().push(request.clone());
        self.pending.lock().unwrap().insert(request.session.id.clone(),vec![
            Event::Admitted{turn:request.turn.id.clone(),message:Some("fixture-message".into()),external_turn:Some(format!("fixture-{}",request.turn.id))},
            Event::Consumed{turn:request.turn.id.clone(),message:Some("fixture-message".into())},
            Event::Message{turn:request.turn.id.clone(),key:format!("fixture-final-{}",request.turn.id),content:{let mut responses=self.responses.lock().unwrap();if responses.is_empty(){json!({"status":"completed","summary":"已分析输入材料","outputs":{"risks":{"risks":["权限隔离"]}}})}else{responses.remove(0)}}.to_string(),
                kind:ConversationEventKind::Assistant,mode:MessageMode::Full,item:format!("fixture-item-{}",request.turn.id)},
            Event::Terminal{turn:request.turn.id.clone(),outcome:ports::ProviderTerminal::Completed,reason:"explicit port fixture".into(),quiescent:true},
        ]);
        Ok(())
    }
    fn poll_session(&self, session: &Session) -> Result<Vec<ports::ConversationObservation>> {
        if self.hold.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(vec![]);
        }
        Ok(self
            .pending
            .lock()
            .unwrap()
            .remove(&session.id)
            .unwrap_or_default())
    }
    fn stop_session(&self, _: &Session, execution: &RuntimeExecution) -> Result<ports::WorkerStop> {
        Ok(ports::WorkerStop {
            operation_id: execution.operation_id.clone(),
            writer_revoked: true,
            mode: "fixture".into(),
            requires_successor: false,
        })
    }
    fn cleanup_worker(&self, _: &Session, _: &RuntimeExecution) -> Result<()> {
        Ok(())
    }
}

#[test]
fn custom_agent_executes_actual_task_through_ports_and_persists_typed_artifact() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine(dir.path());
    let recorder = Arc::new(RecordingAgent::default());
    engine.harness = recorder.clone();
    let role = command(
        &engine,
        "role.create",
        None,
        json!({"name":"研究员","data":{"instructions":"调查具体输入的风险"}}),
    )
    .unwrap();
    let cap=command(&engine,"capability.create",None,json!({"name":"调研能力","data":{"model_ref":DEFAULT_MODEL_ID,"harness_ref":DEFAULT_HARNESS_ID,"skills":[],"tools":[DEFAULT_NATIVE_TOOL_ID]}})).unwrap();
    let agent=command(&engine,"agent.create",None,json!({"name":"调研 Agent","data":{"role_ref":role["id"],"capability_ref":cap["id"],"permission_selection":default_permission_selection()}})).unwrap();
    let output=command(&engine,"delivery_type.create",None,json!({"name":"风险清单","data":{"body":"风险列表","format":"json","content_schema":{"type":"object","required":["risks"],"properties":{"risks":{"type":"array","items":{"type":"string"}}},"additionalProperties":false}}})).unwrap();
    let workflow = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"inquiry","name":"调查","default_role_ref":role["definition_id"]}],"steps":[{
        "key":"study","name":"识别风险","stage":"inquiry","executor":{"type":"agent"},"task_instructions":"只调查当前材料，提交风险清单。",
        "outputs":{"risks":{"delivery_ref":output["id"]}}}]}),
    );
    let project = command(
        &engine,
        "project.create",
        None,
        json!({"title":"调研项目","workspace":"."}),
    )
    .unwrap();
    let feature = id();
    engine
        .store
        .transact(None, &mut |state| {
            state.features.push(Feature {
                id: feature.clone(),
                project_id: project["id"].as_str().unwrap().into(),
                title: "Feature 调研".into(),
                status: Status::Queued,
                revision: 1,
                input_version: 1,

                predecessor: None,
                source_materials: vec![],
            });
            Ok(Value::Null)
        })
        .unwrap();
    let instance=command(&engine,"workflow.start",Some(&feature),json!({"workflow_ref":workflow["id"],"role_agent_bindings":{role["definition_id"].as_str().unwrap():agent["id"]}})).unwrap();
    let binding:RuntimeBinding=serde_json::from_value(json!({"installation":{"id":"fixture-runtime","key":{"provider":"codex","version":"fixture","os":"linux","arch":"aarch64"},
        "artifact":{"kind":"docker_image","image_id":format!("sha256:{}","a".repeat(64))},"source":"explicit native port fixture","license":"test"},
        "adapter_version":"fixture/v1","model":"fixture-model","tool_policy":"read_only","work_type":"discussion","configuration_digest":"a".repeat(64)})).unwrap();
    for _ in 0..12 {
        control_fixture::evaluate(&engine).unwrap();
        for intent in engine.pending_workflow_intents().unwrap() {
            let method = intent.data["method"].as_str().unwrap();
            let mut c: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
            if method == "session.open" {
                c.payload["binding"] = json!(binding);
            }
            let result = engine.execute(method, c);
            engine.observe_workflow_intent(&intent.id, &result).unwrap();
            result.unwrap();
        }
        engine.tick_conversations(4).unwrap();
    }
    let state = engine.store.snapshot().unwrap();
    let row = state
        .configuration(instance["id"].as_str().unwrap())
        .unwrap();
    assert_eq!(row.data["status"], "completed", "{}", row.data);
    let output_id = row.data["steps"][0]["outputs"]["risks"].as_str().unwrap();
    let output = state.configuration(output_id).unwrap();
    assert_eq!(output.data["value"], json!({"risks":["权限隔离"]}));
    let artifact = state
        .artifacts
        .iter()
        .find(|a| Some(a.id.as_str()) == output.data["artifact_id"].as_str())
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&engine.artifacts.read(artifact).unwrap()).unwrap(),
        output.data["value"]
    );
    let calls = recorder.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let task = calls[0].configured_task.as_ref().unwrap();
    assert_eq!(
        task.task["task_instructions"],
        "只调查当前材料，提交风险清单。"
    );
    assert!(task.task["outputs"]["risks"]["definition"]["data"]["content_schema"].is_object());
    assert!(
        engine
            .query(
                "query.execution_task",
                &json!({"turn_id":calls[0].turn.id}),
                4
            )
            .unwrap()["configured"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn reusable_children_and_collection_members_have_distinct_execution_paths() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let text = command(
        &engine,
        "delivery_type.create",
        None,
        json!({"name":"条目","data":{"body":"输入条目","format":"text"}}),
    )
    .unwrap();
    let array=command(&engine,"delivery_type.create",None,json!({"name":"条目集合","data":{"body":"输入集合","format":"json","content_schema":{"type":"array","items":{"type":"string"},"minItems":2}}})).unwrap();
    let child = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"work","name":"处理"}],"inputs":{"entry":{"delivery_ref":text["id"]}},
        "steps":[{"key":"gate","name":"完成条目","stage":"work","executor":{"type":"control"}}]}),
    );
    let parent = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"work","name":"分发"}],"inputs":{"entries":{"delivery_ref":array["id"]}},"steps":[{
        "key":"members","name":"处理集合","stage":"work","executor":{"type":"workflow","workflow_ref":child["id"],"foreach":{
            "source":{"step":"$input","port":"entries"},"item_input":"entry","max_concurrency":1,"min_items":2}}}]}),
    );
    let project = command(
        &engine,
        "project.create",
        None,
        json!({"title":"集合","workspace":"."}),
    )
    .unwrap();
    assert!(
        command(
            &engine,
            "workflow.start",
            project["id"].as_str(),
            json!({"workflow_ref":parent["id"],"role_agent_bindings":{},"inputs":{"entries":[]}})
        )
        .is_err()
    );
    let instance=command(&engine,"workflow.start",project["id"].as_str(),json!({"workflow_ref":parent["id"],"role_agent_bindings":{},"inputs":{"entries":["A","B"]}})).unwrap();
    for _ in 0..6 {
        control_fixture::evaluate(&engine).unwrap();
    }
    let state = engine.store.snapshot().unwrap();
    let root = state
        .configuration(instance["id"].as_str().unwrap())
        .unwrap();
    assert_eq!(root.data["status"], "completed");
    let children: Vec<_> = state
        .governance
        .iter()
        .filter(|r| r.kind == "workflow_instance" && r.data["parent_execution_id"].is_string())
        .collect();
    assert_eq!(children.len(), 2);
    assert_ne!(
        children[0].data["steps"][0]["id"],
        children[1].data["steps"][0]["id"]
    );
    assert_ne!(
        children[0].data["steps"][0]["path"],
        children[1].data["steps"][0]["path"]
    );
    assert_ne!(
        children[0].data["inputs"]["entry"],
        children[1].data["inputs"]["entry"]
    );
}

fn fixture_binding() -> RuntimeBinding {
    serde_json::from_value(json!({"installation":{"id":"fixture-runtime","key":{"provider":"codex","version":"fixture","os":"linux","arch":"aarch64"},"artifact":{"kind":"docker_image","image_id":format!("sha256:{}","a".repeat(64))},"source":"explicit native port fixture","license":"test"},"adapter_version":"fixture/v1","model":"fixture-model","tool_policy":"read_only","work_type":"workflow","configuration_digest":"a".repeat(64)})).unwrap()
}
fn drive(engine: &Engine, rounds: usize) {
    for _ in 0..rounds {
        control_fixture::evaluate(&engine).unwrap();
        for row in engine.pending_workflow_intents().unwrap() {
            let method = row.data["method"].as_str().unwrap();
            let mut c: Command = serde_json::from_value(row.data["command"].clone()).unwrap();
            let result = if method == "workflow.tool.execute" {
                let request = engine.configured_platform_request(&c).unwrap().unwrap();
                engine.enqueue_platform_tool(&c, &request)
            } else {
                if method == "session.open" {
                    c.payload["binding"] = json!(fixture_binding());
                }
                engine.execute(method, c)
            };
            engine.observe_workflow_intent(&row.id, &result).unwrap();
            result.unwrap();
        }
        engine.drive_platform_tools().unwrap();
        engine.tick_conversations(4).unwrap();
    }
}
fn research_agent(engine: &Engine) -> (Value, Value) {
    let role = command(
        engine,
        "role.create",
        None,
        json!({"name":"合规调查员","data":{"instructions":"分析明确输入并提供依据"}}),
    )
    .unwrap();
    let capability=command(engine,"capability.create",None,json!({"name":"调查能力","data":{"model_ref":DEFAULT_MODEL_ID,"harness_ref":DEFAULT_HARNESS_ID,"skills":[],"tools":[DEFAULT_NATIVE_TOOL_ID]}})).unwrap();
    let agent=command(engine,"agent.create",None,json!({"name":"调查 Agent","data":{"role_ref":role["id"],"capability_ref":capability["id"],"permission_selection":default_permission_selection()}})).unwrap();
    (role, agent)
}
fn launch(engine: &Engine, role: &Value, agent: &Value, mut step: Value) -> Value {
    step["key"] = json!("study");
    step["name"] = json!("调查");
    step["stage"] = json!("work");
    step["executor"] = json!({"type":"agent","role_ref":role["definition_id"]});
    if step["task_instructions"].is_null() {
        step["task_instructions"] = json!("只调查本次任务，使用已加载的 Skill 方法。");
    }
    let workflow = publish(
        engine,
        json!({"schema_version":5,"stages":[{"key":"work","name":"调查"}],"steps":[step]}),
    );
    let project = command(engine, "project.create", None, json!({"title":"调查项目"})).unwrap();
    command(engine,"workflow.start",project["id"].as_str(),json!({"workflow_ref":workflow["id"],"role_agent_bindings":{role["definition_id"].as_str().unwrap():agent["id"]}})).unwrap()
}
fn instance(engine: &Engine, id: &Value) -> Value {
    engine
        .store
        .snapshot()
        .unwrap()
        .configuration(id.as_str().unwrap())
        .unwrap()
        .data
        .clone()
}

#[test]
fn shipped_configuration_compiles_and_reset_references_form_a_complete_new_graph() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let defaults = engine.configuration_defaults(None).unwrap();
    let state = engine.store.snapshot().unwrap();
    let bindings = serde_json::from_value(defaults["role_agent_bindings"].clone()).unwrap();
    let plan = engine
        .compile_configured_workflow(
            &state,
            state
                .configuration(defaults["workflow_ref"].as_str().unwrap())
                .unwrap(),
            &Scope::Global,
            Some(&bindings),
        )
        .unwrap();
    assert_eq!(plan.roles.len(), 6);
    let feature_test_author = plan
        .document
        .steps
        .iter()
        .find(|step| step.tags.iter().any(|tag| tag == "release_recipe"))
        .unwrap();
    assert!(
        !feature_test_author.access.writable_paths.is_empty(),
        "Feature tests must have a declared author with actual test-directory access"
    );
    let task: WorkflowDefinition = state
        .configuration(defaults["task_workflow_ref"].as_str().unwrap())
        .unwrap()
        .decode()
        .unwrap();
    let review=task.steps.iter().find(|step|step.inputs.contains_key("verify")).expect("the shipped implementation Spec requires an actual delivery verification before review");
    for key in ["documentation", "review", "candidate_review"] {
        let consumer = task.steps.iter().find(|step| step.key == key).unwrap();
        let InputBinding::Source { source } = &consumer.inputs["evidence"] else {
            panic!("raw evidence must be an explicit upstream delivery")
        };
        let producer = task
            .steps
            .iter()
            .find(|step| step.key == source.step)
            .unwrap();
        assert!(producer.outputs[&source.port].required);
        assert_eq!(
            producer.outputs[&source.port].delivery_ref,
            "builtin-delivery-execution-evidence-v1"
        );
        assert!(task.ancestors(consumer).contains(producer.key.as_str()));
    }
    let baseline = task
        .steps
        .iter()
        .find(|step| step.key == "red_baseline")
        .unwrap();
    let red = task.steps.iter().find(|step| step.key == "red").unwrap();
    assert!(task.ancestors(red).contains(baseline.key.as_str()));
    let sealed_tests = task
        .steps
        .iter()
        .find(|step| step.key == "tests_author_snapshot")
        .unwrap();
    assert!(matches!(sealed_tests.executor, StepExecutor::Tool { .. }));
    assert!(task.ancestors(baseline).contains(sealed_tests.key.as_str()));
    assert!(
        matches!(&baseline.inputs["workspace"], InputBinding::Source { source } if source.step == sealed_tests.key)
    );
    let red_snapshot = task
        .steps
        .iter()
        .find(|step| step.key == "tests_snapshot")
        .unwrap();
    let StepExecutor::Tool { arguments, .. } = &red_snapshot.executor else {
        panic!("Red snapshot must be an actual platform tool")
    };
    assert!(
        matches!(&arguments["workspace"], InputBinding::Source { source } if source.step == sealed_tests.key)
    );
    assert!(
        baseline
            .access
            .read_only_paths
            .iter()
            .any(|path| path == "tests")
    );
    let InputBinding::Source {
        source: verify_source,
    } = &review.inputs["verify"]
    else {
        panic!("verification must come from an actual execution")
    };
    let verify = task
        .steps
        .iter()
        .find(|step| step.key == verify_source.step)
        .unwrap();
    let StepExecutor::Tool {
        tool_ref,
        arguments,
        ..
    } = &verify.executor
    else {
        panic!("verification must execute a real test runner")
    };
    assert_eq!(
        state.configuration(tool_ref).unwrap().data["qualified_name"],
        "tests.run"
    );
    assert!(task.ancestors(review).contains(verify.key.as_str()));
    let InputBinding::Source {
        source: documentation,
    } = &review.inputs["documentation"]
    else {
        panic!("review must receive the delivered documentation")
    };
    assert!(task.ancestors(verify).contains(documentation.step.as_str()));
    let InputBinding::Source {
        source: green_source,
    } = &review.inputs["green"]
    else {
        panic!("review must receive actual Green")
    };
    let green = task
        .steps
        .iter()
        .find(|step| step.key == green_source.step)
        .unwrap();
    let StepExecutor::Tool {
        arguments: green_arguments,
        ..
    } = &green.executor
    else {
        panic!("Green must execute actual tests")
    };
    assert_eq!(arguments["workspace"], green_arguments["workspace"]);
    assert_eq!(arguments["recipe"], green_arguments["recipe"]);
    assert!(
        plan.document
            .steps
            .iter()
            .any(|s| matches!(s.executor, StepExecutor::Workflow { .. }))
    );
    assert!(
        plan.document
            .steps
            .iter()
            .filter(|s| matches!(s.executor, StepExecutor::Agent { .. }))
            .all(|s| s
                .task_instructions
                .as_deref()
                .is_some_and(|v| !v.trim().is_empty()))
    );
    let mut restored = state.clone();
    restore_template_catalog(&mut restored, "configured-reset").unwrap();
    runtime::bind_default_permissions(&engine);
    let defaults = current_template_defaults(&restored).unwrap();
    let definition: WorkflowDefinition = restored
        .configuration(defaults["workflow_ref"].as_str().unwrap())
        .unwrap()
        .decode()
        .unwrap();
    definition.validate().unwrap();
    for step in &definition.steps {
        for port in step.outputs.values() {
            assert!(restored.configuration(&port.delivery_ref).is_ok());
        }
        if let Some(role) = definition.role(step) {
            assert!(
                restored
                    .governance
                    .iter()
                    .any(|r| r.kind == "role" && r.definition_id == role)
            );
        }
    }
}

#[test]
fn dynamic_spec_rejection_cannot_be_confirmed_and_rework_keeps_the_original_report() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine(dir.path());
    let recorder = Arc::new(RecordingAgent::default());
    engine.harness = recorder.clone();
    recorder.responses.lock().unwrap().extend([json!({"status":"completed","summary":"发现问题","outputs":{"report":{"approve":false,"summary":"存在隔离问题","checks":[{"key":"privacy_isolation","passed":false,"evidence":"输入第 2 节缺少隔离"}]}}}),json!({"status":"completed","summary":"核对完成","outputs":{"report":{"approve":true,"summary":"依据齐备","checks":[{"key":"privacy_isolation","passed":true,"evidence":"输入第 2 节已明确隔离边界"}]}}})]);
    let (role, agent) = research_agent(&engine);
    let spec=command(&engine,"spec.create",None,json!({"name":"隔离规范","data":{"schema_version":3,"body":"核对隐私边界","checks":[{"key":"privacy_isolation","name":"隐私隔离","instructions":"逐项查明隔离边界与依据"}]}})).unwrap();
    let run = launch(
        &engine,
        &role,
        &agent,
        json!({"spec_refs":[spec["id"]],"outputs":{"report":{"delivery_ref":"builtin-delivery-review-v1","content_checks":true}},"completion":{"op":"equals","source":{"step":"study","port":"report","pointer":"/approve"},"value":true},"confirmation":"human"}),
    );
    drive(&engine, 12);
    let before = instance(&engine, &run["id"]);
    assert_eq!(before["steps"][0]["status"], "waiting_confirmation");
    assert!(command(&engine,"workflow.step.confirm",run["id"].as_str(),json!({"step":"study","input_digest":before["steps"][0]["input_digest"],"outputs":before["steps"][0]["outputs"],"override_completion":true,"reason":"force"})).is_err());
    let output = before["steps"][0]["outputs"]["report"]
        .as_str()
        .unwrap()
        .to_owned();
    command(
        &engine,
        "workflow.step.rework",
        run["id"].as_str(),
        json!({"step":"study"}),
    )
    .unwrap();
    drive(&engine, 12);
    let after = instance(&engine, &run["id"]);
    assert_ne!(before["steps"][0]["id"], after["steps"][0]["id"]);
    assert_eq!(
        engine
            .store
            .snapshot()
            .unwrap()
            .configuration(&output)
            .unwrap()
            .data["value"]["approve"],
        false
    );
    command(&engine,"workflow.step.confirm",run["id"].as_str(),json!({"step":"study","input_digest":after["steps"][0]["input_digest"],"outputs":after["steps"][0]["outputs"]})).unwrap();
    drive(&engine, 2);
    assert_eq!(instance(&engine, &run["id"])["status"], "completed");
}

#[test]
fn explicit_manual_approval_requires_reason_and_exact_execution_and_preserves_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine(dir.path());
    let recorder = Arc::new(RecordingAgent::default());
    engine.harness = recorder.clone();
    let report = json!({"approve":false,"summary":"原始证据缺失","checks":[]});
    recorder
        .responses
        .lock()
        .unwrap()
        .push(json!({"status":"completed","summary":"检查未通过","outputs":{"report":report}}));
    let (role, agent) = research_agent(&engine);
    let run = launch(
        &engine,
        &role,
        &agent,
        json!({
            "outputs":{"report":{"delivery_ref":"builtin-delivery-object-v1"}},
            "completion":{"op":"equals","source":{"step":"study","port":"report","pointer":"/approve"},"value":true}
        }),
    );
    drive(&engine, 12);
    let before = instance(&engine, &run["id"]);
    let execution = &before["steps"][0];
    assert_eq!(execution["status"], "awaiting_acceptance");
    let payload = json!({"step":"study","execution_id":execution["id"],"input_digest":execution["input_digest"],"outputs":execution["outputs"],"reason":"已核对功能，接受本轮历史证据缺失"});
    for (key, value) in [
        ("reason", json!("  ")),
        ("execution_id", json!("old-execution")),
        ("input_digest", json!("old-input")),
        ("outputs", json!({})),
    ] {
        let mut invalid = payload.clone();
        invalid[key] = value;
        assert!(
            command(
                &engine,
                "workflow.step.approve",
                run["id"].as_str(),
                invalid
            )
            .is_err(),
            "{key}"
        );
    }
    // Ordinary confirmation still respects the frozen workflow's override policy.
    assert!(
        command(
            &engine,
            "workflow.step.confirm",
            run["id"].as_str(),
            payload.clone()
        )
        .is_err()
    );
    // A stale instance revision from another step must not invalidate this
    // exact execution's unchanged inputs and outputs.
    engine
        .execute(
            "workflow.step.approve",
            Command {
                command_id: id(),
                target: run["id"].as_str().map(str::to_owned),
                expected_revision: run["revision"].as_u64(),
                payload: payload.clone(),
            },
        )
        .unwrap();
    drive(&engine, 2);
    let after = instance(&engine, &run["id"]);
    assert_eq!(after["status"], "completed");
    assert_eq!(after["plan"], before["plan"]);
    assert_eq!(after["steps"][0]["outputs"], execution["outputs"]);
    let view = engine
        .query("query.workflow_instance", &json!({"id":run["id"]}), 4)
        .unwrap();
    let completion = &view["completion_status"][execution["id"].as_str().unwrap()];
    assert_eq!(completion["satisfied"], false);
    assert_eq!(completion["confirmation"]["manual_approval"], true);
    assert_eq!(completion["confirmation"]["override_completion"], true);
    assert_eq!(completion["confirmation"]["reason"], payload["reason"]);
    assert_eq!(
        view["outputs"][execution["outputs"]["report"].as_str().unwrap()]["value"],
        report
    );
    assert!(
        command(
            &engine,
            "workflow.step.rework",
            run["id"].as_str(),
            json!({"step":"study"}),
        )
        .is_err()
    ); // Completed roots are immutable; later work uses a successor.
    assert!(
        command(
            &engine,
            "workflow.step.approve",
            run["id"].as_str(),
            payload
        )
        .is_err()
    );
}

fn execution_evidence_fixture() -> Value {
    json!({"runs":[
        {"sequence":1,"phase":"red","command":"node --test tests/behavior.mjs","cwd":"/work","started_at":"2026-11-10T01:00:00Z","finished_at":"2026-11-10T01:00:01Z","exit_code":1,"stdout":"expected true, actual false","stderr":"","files":[{"path":"src/behavior.mjs","sha256":"a".repeat(64),"content":"export default false"},{"path":"tests/behavior.mjs","sha256":"b".repeat(64),"content":"test the public behavior"}]},
        {"sequence":2,"phase":"green","command":"node --test tests/behavior.mjs","cwd":"/work","started_at":"2026-11-10T01:00:02Z","finished_at":"2026-11-10T01:00:03Z","exit_code":0,"stdout":"pass 1, fail 0","stderr":"","files":[{"path":"src/behavior.mjs","sha256":"c".repeat(64),"content":"export default true"},{"path":"tests/behavior.mjs","sha256":"b".repeat(64),"content":"test the public behavior"}]}
    ]})
}

#[test]
fn raw_evidence_contract_rejects_temp_paths_without_contents() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine(dir.path());
    let recorder = Arc::new(RecordingAgent::default());
    engine.harness = recorder.clone();
    recorder.responses.lock().unwrap().push(json!({"status":"completed","summary":"记录在临时目录","outputs":{"evidence":{"path":"/tmp/evidence","sha256":"a".repeat(64)}}}));
    let (role, agent) = research_agent(&engine);
    let run = launch(
        &engine,
        &role,
        &agent,
        json!({"outputs":{"evidence":{"delivery_ref":"builtin-delivery-execution-evidence-v1"}}}),
    );
    drive(&engine, 12);
    let runtime = instance(&engine, &run["id"]);
    assert_eq!(runtime["steps"][0]["status"], "failed");
    assert_eq!(runtime["steps"][0]["outputs"], json!({}));
}

#[test]
fn declared_delivery_inputs_carry_persisted_raw_evidence_without_other_outputs() {
    let dir = tempfile::tempdir().unwrap();
    let mut workbench = engine(dir.path());
    let recorder = Arc::new(RecordingAgent::default());
    workbench.harness = recorder.clone();
    let evidence = execution_evidence_fixture();
    let (role, agent) = research_agent(&workbench);
    recorder.responses.lock().unwrap().extend([
        json!({"status":"completed","summary":"实际记录已交付","outputs":{"evidence":evidence,"unrelated":{"private_note":"unbound output must not reach reviewer"}}}),
        json!({"status":"completed","summary":"已核对完整历史记录","outputs":{"result":{"checked":true}}})
    ]);
    let workflow = publish(
        &workbench,
        json!({"schema_version":5,"stages":[{"key":"work","name":"证据交付","default_role_ref":role["definition_id"]}],"steps":[
            {"key":"produce","name":"记录执行","stage":"work","executor":{"type":"agent"},"task_instructions":"将真实记录作为 evidence 输出交付。","outputs":{"evidence":{"delivery_ref":"builtin-delivery-execution-evidence-v1"},"unrelated":{"delivery_ref":"builtin-delivery-object-v1"}}},
            {"key":"inspect","name":"检查证据","stage":"work","dependencies":["produce"],"executor":{"type":"agent"},"task_instructions":"只检查声明的 evidence 输入。","inputs":{"evidence":{"type":"source","source":{"step":"produce","port":"evidence"}}},"outputs":{"result":{"delivery_ref":"builtin-delivery-object-v1"}}}
        ]}),
    );
    let project = command(
        &workbench,
        "project.create",
        None,
        json!({"title":"证据范围"}),
    )
    .unwrap();
    let run = command(&workbench, "workflow.start", project["id"].as_str(), json!({"workflow_ref":workflow["id"],"role_agent_bindings":{role["definition_id"].as_str().unwrap():agent["id"]}})).unwrap();
    drive(&workbench, 24);
    let state = workbench.store.snapshot().unwrap();
    let runtime = instance(&workbench, &run["id"]);
    assert_eq!(runtime["status"], "completed");
    let output_id = runtime["steps"][0]["outputs"]["evidence"].as_str().unwrap();
    let output = state.configuration(output_id).unwrap();
    let artifact_id = output.data["artifact_id"].as_str().unwrap();
    let artifact = state
        .artifacts
        .iter()
        .find(|a| a.id == artifact_id)
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&workbench.artifacts.read(artifact).unwrap()).unwrap(),
        evidence
    );
    let calls = recorder.calls.lock().unwrap();
    let inspect: Value = serde_json::from_str(&calls[1].turn.input).unwrap();
    assert_eq!(inspect["inputs"], json!({"evidence":evidence}));
    assert_eq!(
        inspect["input_sources"]["evidence"]["source_output_id"],
        output_id
    );
    assert_eq!(
        inspect["input_sources"]["evidence"]["value_digest"],
        configuration_hash(&evidence)
    );
    assert!(
        !calls[1]
            .turn
            .input
            .contains("unbound output must not reach reviewer")
    );
    assert!(
        inspect["authorization"]["writable_paths"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let artifact = artifact.clone();
    let output_id = output_id.to_owned();
    drop(calls);
    drop(workbench);
    let reopened = SqliteStore::open(dir.path()).unwrap();
    assert_eq!(
        reopened
            .snapshot()
            .unwrap()
            .configuration(&output_id)
            .unwrap()
            .data["value"],
        evidence
    );
    assert_eq!(
        serde_json::from_slice::<Value>(
            &FileArtifacts::new(dir.path().into())
                .read(&artifact)
                .unwrap()
        )
        .unwrap(),
        evidence
    );
}

#[test]
fn supplemental_input_uses_the_same_canonical_task_and_cannot_rewrite_the_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine(dir.path());
    let recorder = Arc::new(RecordingAgent::default());
    engine.harness = recorder.clone();
    recorder
        .responses
        .lock()
        .unwrap()
        .push(json!({"status":"needs_input","summary":"需要调查范围","outputs":{}}));
    let (role, agent) = research_agent(&engine);
    let run = launch(
        &engine,
        &role,
        &agent,
        json!({"outputs":{"risks":{"delivery_ref":"builtin-delivery-object-v1"}}}),
    );
    drive(&engine, 12);
    let data = instance(&engine, &run["id"]);
    assert_eq!(data["steps"][0]["status"], "waiting_input");
    command(
        &engine,
        "workflow.step.input",
        run["id"].as_str(),
        json!({"step":"study","input":"只调查当前提供的材料"}),
    )
    .unwrap();
    drive(&engine, 12);
    let calls = recorder.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    let first = calls[0].configured_task.as_ref().unwrap();
    let second = calls[1].configured_task.as_ref().unwrap();
    assert_eq!(
        first.task["task_instructions"],
        second.task["task_instructions"]
    );
    assert_eq!(second.task["supplemental_input"], "只调查当前提供的材料");
    assert_eq!(
        first.task["source"]["execution_id"],
        second.task["source"]["execution_id"]
    );
    assert_eq!(instance(&engine, &run["id"])["status"], "completed");
    let mut forged = second.clone();
    forged.task["task_instructions"] = json!("更换为别的任务");
    assert!(forged.validate().is_err());
}

#[test]
fn invalid_typed_output_fails_and_retry_creates_a_new_execution_without_changing_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine(dir.path());
    let recorder = Arc::new(RecordingAgent::default());
    engine.harness = recorder.clone();
    recorder.responses.lock().unwrap().push(json!({"status":"completed","summary":"无效输出","outputs":{"risks":"should be an object"}}));
    let (role, agent) = research_agent(&engine);
    let run = launch(
        &engine,
        &role,
        &agent,
        json!({"inputs":{"scope":{"type":"constant","value":"current input"}},"outputs":{"risks":{"delivery_ref":"builtin-delivery-object-v1"}}}),
    );
    drive(&engine, 12);
    let before = instance(&engine, &run["id"]);
    assert_eq!(before["status"], "failed");
    assert_eq!(before["steps"][0]["outputs"], json!({}));
    command(
        &engine,
        "workflow.step.retry",
        run["id"].as_str(),
        json!({"step":"study"}),
    )
    .unwrap();
    drive(&engine, 12);
    let after = instance(&engine, &run["id"]);
    assert_eq!(after["status"], "completed");
    assert_eq!(before["steps"][0]["inputs"], after["steps"][0]["inputs"]);
    assert_ne!(before["steps"][0]["id"], after["steps"][0]["id"]);
    assert!(
        engine
            .store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.kind == "workflow_execution_history")
    );
}

#[test]
fn platform_step_runs_without_a_placeholder_agent_session_and_seals_real_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let workflow = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"work","name":"资料封存"}],"steps":[{"key":"capture","name":"封存输入","stage":"work","executor":{"type":"tool","tool_ref":"builtin-tool-materials-capture-v1","arguments":{"material":{"type":"constant","value":{"source":"exact input"}}}},"outputs":{"receipt":{"delivery_ref":"builtin-delivery-object-v1"}}}]}),
    );
    let project = command(&engine, "project.create", None, json!({"title":"资料项目"})).unwrap();
    let run = command(
        &engine,
        "workflow.start",
        project["id"].as_str(),
        json!({"workflow_ref":workflow["id"],"role_agent_bindings":{}}),
    )
    .unwrap();
    drive(&engine, 6);
    let data = instance(&engine, &run["id"]);
    assert_eq!(data["status"], "completed");
    let state = engine.store.snapshot().unwrap();
    assert!(state.sessions.is_empty());
    assert!(state.executions.is_empty());
    let call = state
        .governance
        .iter()
        .find(|r| r.kind == "platform_call")
        .unwrap();
    assert_eq!(call.data["request"]["execution_id"], data["steps"][0]["id"]);
    assert_eq!(
        call.data["receipt"]["inputs"]["material"]["source"],
        "exact input"
    );
    assert!(
        engine
            .store
            .transact(None, &mut |state| {
                let row = state
                    .governance
                    .iter_mut()
                    .find(|r| r.id == run["id"].as_str().unwrap())
                    .unwrap();
                row.data["inputs"] = json!({"forged":true});
                row.content_hash = configuration_hash(&row.data);
                Ok(Value::Null)
            })
            .is_err()
    );
}

#[test]
fn invalid_child_dispatch_is_rejected_without_blocking_independent_work() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let child = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"work","name":"检查"}],"inputs":{"material":{"delivery_ref":"builtin-delivery-prd-v1"}},"steps":[{"key":"finish","name":"完成","stage":"work","executor":{"type":"control"}}]}),
    );
    let parent = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"work","name":"分支"}],"steps":[{"key":"bad","name":"无效子流程输入","stage":"work","executor":{"type":"workflow","workflow_ref":child["id"]},"inputs":{"material":{"type":"constant","value":123}}},{"key":"good","name":"独立分支","stage":"work","executor":{"type":"control"}}]}),
    );
    let project = command(&engine, "project.create", None, json!({"title":"分支项目"})).unwrap();
    let run = command(
        &engine,
        "workflow.start",
        project["id"].as_str(),
        json!({"workflow_ref":parent["id"],"role_agent_bindings":{}}),
    )
    .unwrap();
    drive(&engine, 2);
    let data = instance(&engine, &run["id"]);
    assert_eq!(data["steps"][0]["status"], "queued");
    assert!(
        control_fixture::command(
            &engine,
            "workflow.step.dispatch",
            run["id"].as_str().unwrap(),
            json!({"step":"bad"})
        )
        .is_err()
    );
    assert_eq!(data["steps"][1]["status"], "completed");
}

#[test]
fn stale_start_and_unprepared_agent_session_are_rejected_before_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let workflow = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"work","name":"工作"}],"steps":[{"key":"gate","name":"确认","stage":"work","executor":{"type":"control"}}]}),
    );
    let project = command(&engine, "project.create", None, json!({"title":"版本保护"})).unwrap();
    let error = engine
        .execute(
            "workflow.start",
            Command {
                command_id: id(),
                target: Some(project["id"].as_str().unwrap().into()),
                expected_revision: Some(0),
                payload: json!({"workflow_ref":workflow["id"],"role_agent_bindings":{}}),
            },
        )
        .unwrap_err();
    assert_eq!(error.code, "revision_conflict");
    assert!(engine.configured_agent_request(&Command{command_id:id(),target:project["id"].as_str().map(str::to_owned),expected_revision:None,payload:json!({"configured_execution_id":"fabricated","agent":default_agent_id("analyst")})}).is_err());
    assert!(
        !engine
            .store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.kind == "workflow_instance")
    );
}

#[test]
fn cancelling_one_child_does_not_cancel_its_sibling_and_rework_replaces_the_old_children() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let child = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"work","name":"工作"}],"steps":[{"key":"gate","name":"确认","stage":"work","executor":{"type":"control"},"confirmation":"human"}]}),
    );
    let parent = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"work","name":"并行工作"}],"steps":[
      {"key":"left","name":"分支一","stage":"work","executor":{"type":"workflow","workflow_ref":child["id"]}},
      {"key":"right","name":"分支二","stage":"work","executor":{"type":"workflow","workflow_ref":child["id"]}}]}),
    );
    let project = command(
        &engine,
        "project.create",
        None,
        json!({"title":"子流程范围"}),
    )
    .unwrap();
    let run = command(
        &engine,
        "workflow.start",
        project["id"].as_str(),
        json!({"workflow_ref":parent["id"],"role_agent_bindings":{}}),
    )
    .unwrap();
    control_fixture::evaluate(&engine).unwrap();
    control_fixture::evaluate(&engine).unwrap();
    let state = engine.store.snapshot().unwrap();
    let root = state.configuration(run["id"].as_str().unwrap()).unwrap();
    let first = root.data["steps"][0]["id"].clone();
    let second = root.data["steps"][1]["id"].clone();
    let left = state
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.data["parent_execution_id"] == first)
        .unwrap()
        .clone();
    let right = state
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.data["parent_execution_id"] == second)
        .unwrap()
        .clone();
    command(&engine, "workflow.cancel", Some(&left.id), json!({})).unwrap();
    control_fixture::evaluate(&engine).unwrap();
    control_fixture::evaluate(&engine).unwrap();
    let state = engine.store.snapshot().unwrap();
    assert_eq!(
        state.configuration(&left.id).unwrap().data["status"],
        "cancelled"
    );
    assert_eq!(
        state.configuration(&right.id).unwrap().data["cancel_requested"],
        false
    );
    assert_eq!(
        state.configuration(&right.id).unwrap().data["steps"][0]["status"],
        "waiting_confirmation"
    );
    command(
        &engine,
        "workflow.step.rework",
        run["id"].as_str(),
        json!({"step":"left"}),
    )
    .unwrap();
    control_fixture::evaluate(&engine).unwrap();
    let state = engine.store.snapshot().unwrap();
    let root = state.configuration(run["id"].as_str().unwrap()).unwrap();
    assert_ne!(root.data["steps"][0]["id"], first);
    assert_eq!(root.data["steps"][1]["id"], second);
    assert!(
        state
            .governance
            .iter()
            .any(|r| r.kind == "workflow_execution_history"
                && r.definition_id == first.as_str().unwrap())
    );
}

#[test]
fn configured_workspace_snapshot_and_candidate_use_actual_git_and_reject_forged_requests() {
    actual_git_candidate(false);
}

#[test]
fn explicit_waivers_allow_actual_git_without_inventing_test_or_human_receipts() {
    actual_git_candidate(true);
}

fn actual_git_candidate(waived_advance: bool) {
    fn git(path: &std::path::Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@localhost",
                "-c",
                "commit.gpgSign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "-C",
            ])
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine(dir.path());
    let recorder = Arc::new(RecordingAgent::default());
    engine.harness = recorder.clone();
    let (role, agent) = research_agent(&engine);
    let project = command(&engine, "project.create", None, json!({"title":"实际 Git"})).unwrap();
    let project = project["id"].as_str().unwrap().to_owned();
    let repo = dir
        .path()
        .join("projects")
        .join(&project)
        .join("repository");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    std::fs::write(repo.join("base.txt"), "baseline\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "baseline"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let adapter = Arc::new(devdesk_adapters::CliGit::new(dir.path()).unwrap());
    let feature = id();
    let task = id();
    let main = adapter
        .prepare_mainline(&project, "integration-main", &base, &id())
        .unwrap();
    let feature_work = adapter
        .prepare_worktree(&project, &feature, "features", &base, &id())
        .unwrap();
    let work = adapter
        .prepare_worktree(&project, &task, "tasks", &base, &id())
        .unwrap();
    engine
        .store
        .transact(None, &mut |state| {
            let p = state.projects.iter_mut().find(|p| p.id == project).unwrap();
            p.workspace_definition = Some(Workspace {
                repository: "fixture/local".into(),
                path: std::fs::canonicalize(&repo)
                    .unwrap()
                    .to_string_lossy()
                    .into(),
                mainline: "integration-main".into(),
            });
            state.features.push(Feature {
                id: feature.clone(),
                project_id: project.clone(),
                title: "实际代码".into(),
                status: Status::Queued,
                revision: 1,
                input_version: 1,

                predecessor: None,
                source_materials: vec![],
            });
            state.tasks.push(Task {
                id: task.clone(),
                feature_id: feature.clone(),
                title: "实际 Task".into(),
                status: Status::Draft,
                revision: 1,

                inherited_snapshot: None,
                dependencies: vec![],
                worktree: work.path.clone(),
                scope: TaskScope::default(),
            });
            state.heads.push(GitHead {
                id: task.clone(),
                project_id: project.clone(),
                revision: base.clone(),
            });
            state.heads.push(GitHead {
                id: project.clone(),
                project_id: project.clone(),
                revision: base.clone(),
            });
            state.heads.push(GitHead {
                id: feature.clone(),
                project_id: project.clone(),
                revision: base.clone(),
            });
            for (receipt, kind) in [
                (&main, WorktreeKind::Mainline),
                (&feature_work, WorktreeKind::Feature),
                (&work, WorktreeKind::Task),
            ] {
                state.repository_worktrees.push(RepositoryWorktree {
                    id: receipt.owner.clone(),
                    owner_id: receipt.owner.clone(),
                    project_id: project.clone(),
                    kind,
                    branch: receipt.branch.clone(),
                    path: receipt.path.clone(),
                    base: base.clone(),
                    operation_id: receipt.operation.clone(),
                });
                state.repository_operations.push(RepositoryOperation {
                    id: receipt.operation.clone(),
                    project_id: project.clone(),
                    action: RepositoryAction::CreateWorktree {
                        owner: receipt.owner.clone(),
                        worktree_kind: kind,
                        base: base.clone(),
                    },
                    status: RepositoryOperationStatus::Completed,
                    receipt: Some(RepositoryReceipt::Worktree {
                        worktree: state.repository_worktrees.last().unwrap().clone(),
                    }),
                    reason: String::new(),
                    revision: 1,
                });
            }
            Ok(Value::Null)
        })
        .unwrap();
    engine.git = adapter;
    std::fs::write(
        std::path::Path::new(&work.path).join("addition.txt"),
        "actual change\n",
    )
    .unwrap();
    let mut document = json!({"schema_version":5,"stages":[{"key":"work","name":"Git 工具"}],"steps":[
        {"key":"current","name":"当前版本","stage":"work","executor":{"type":"tool","tool_ref":"builtin-tool-workspace-current-v1","arguments":{"owner_id":{"type":"constant","value":task}}},"outputs":{"workspace":{"delivery_ref":"builtin-delivery-object-v1"}}},
        {"key":"snapshot","name":"固定实际代码","stage":"work","dependencies":["current"],"executor":{"type":"tool","tool_ref":"builtin-tool-workspace-snapshot-v1","arguments":{"workspace":{"type":"source","source":{"step":"current","port":"workspace"}},"allowed_paths":{"type":"constant","value":["addition.txt"]}}},"outputs":{"workspace":{"delivery_ref":"builtin-delivery-object-v1"}}},
        {"key":"candidate","name":"实际候选","stage":"work","dependencies":["snapshot"],"executor":{"type":"tool","tool_ref":"builtin-tool-git-candidate-v1","arguments":{"workspace":{"type":"source","source":{"step":"snapshot","port":"workspace"}},"target_revision":{"type":"constant","value":base}}},"outputs":{"candidate":{"delivery_ref":"builtin-delivery-object-v1"}}}
      ,{"key":"inspect","name":"读取授权的 Task 候选","stage":"work","dependencies":["candidate"],"executor":{"type":"agent","role_ref":role["definition_id"]},"task_instructions":"检查明确提供的代码视图。","inputs":{"workspace":{"type":"source","source":{"step":"candidate","port":"candidate"}}},"access":{"workspace_input":"workspace"},"outputs":{"risks":{"delivery_ref":"builtin-delivery-object-v1"}}}
    ]});
    if waived_advance {
        document["action_requirements"] = json!({"git.advance":["tests","confirmation"]});
        document["steps"].as_array_mut().unwrap().push(json!({"key":"advance","name":"带明确例外接纳","stage":"work","dependencies":["candidate"],"executor":{"type":"tool","tool_ref":"builtin-tool-git-advance-v1","arguments":{"workspace":{"type":"source","source":{"step":"current","port":"workspace"}},"candidate":{"type":"source","source":{"step":"candidate","port":"candidate"}}}},"outputs":{"accepted":{"delivery_ref":"builtin-delivery-object-v1"}}}));
    }
    let workflow = publish(&engine, document);
    let run = engine
        .execute(
            "workflow.start",
            Command {
                command_id: id(),
                target: Some(feature),
                expected_revision: Some(1),
                payload: json!({"workflow_ref":workflow["id"],"role_agent_bindings":{role["definition_id"].as_str().unwrap():agent["id"]}}),
            },
        )
        .unwrap();
    if waived_advance {
        let current = instance(&engine, &run["id"]);
        let definition = current["plan"]["document"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["key"] == "advance")
            .unwrap();
        let changes = json!({"plan_revision_id":current["active_plan_revision_id"],"reason":"专用验收：明确不运行候选测试且不请求人工确认，保留未验证事实","operations":[{"type":"waive","node_id":definition["node_id"],"requirement":"tests"},{"type":"waive","node_id":definition["node_id"],"requirement":"confirmation"}]});
        let preview = engine
            .query(
                "workflow.change.preview",
                &json!({"id":run["id"],"changeset":changes}),
                4,
            )
            .unwrap();
        let r = engine
            .store
            .snapshot()
            .unwrap()
            .configuration(run["id"].as_str().unwrap())
            .unwrap()
            .revision;
        engine.execute("workflow.change.apply",Command{command_id:id(),target:Some(run["id"].as_str().unwrap().into()),expected_revision:Some(r),payload:json!({"plan_revision_id":current["active_plan_revision_id"],"changeset":changes,"preview_hash":preview["preview_hash"]})}).unwrap();
    }
    control_fixture::evaluate(&engine).unwrap();
    let intent = engine.pending_workflow_intents().unwrap().remove(0);
    let command: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
    let mut forged = engine
        .configured_platform_request(&command)
        .unwrap()
        .unwrap();
    forged.arguments = json!({"owner_id":"other"});
    assert!(engine.enqueue_platform_tool(&command, &forged).is_err());
    drive(&engine, 12);
    let runtime = instance(&engine, &run["id"]);
    assert_eq!(runtime["status"], "completed", "{runtime}");
    let state = engine.store.snapshot().unwrap();
    let output = state
        .configuration(
            runtime["steps"][2]["outputs"]["candidate"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
    let candidate = output.data["value"]["revision"].as_str().unwrap();
    assert_eq!(
        git(&repo, &["rev-list", "--parents", "-n", "1", candidate]),
        format!("{candidate} {base}")
    );
    assert_eq!(
        git(&repo, &["show", &format!("{candidate}:addition.txt")]),
        "actual change"
    );
    if waived_advance {
        assert_eq!(git(&repo, &["rev-parse", &feature_work.branch]), candidate);
        let permits: Vec<_> = state
            .governance
            .iter()
            .filter(|r| r.kind == "workflow_action_permit")
            .collect();
        assert!(!permits.is_empty());
        assert_eq!(
            permits.last().unwrap().data["requirements"],
            json!({"tests":"waived","confirmation":"waived"})
        );
        assert!(
            !state
                .governance
                .iter()
                .any(|r| r.kind == "workflow_confirmation"
                    || (r.kind == "platform_call"
                        && r.data["request"]["definition"]["qualified_name"] == "tests.run"))
        );
    }
    let calls = recorder.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0]
            .turn
            .code_view
            .as_ref()
            .unwrap()
            .context
            .source
            .owner,
        task
    );
    assert_ne!(calls[0].session.owner_id, task);
}

#[test]
fn ancestor_pause_suspends_nested_dispatch_without_overriding_a_child_pause() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let leaf = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"work","name":"末层"}],"steps":[{"key":"capture","name":"封存","stage":"work","executor":{"type":"tool","tool_ref":"builtin-tool-materials-capture-v1"}}]}),
    );
    let middle = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"work","name":"中层"}],"steps":[{"key":"child","name":"子流程","stage":"work","executor":{"type":"workflow","workflow_ref":leaf["id"]}}]}),
    );
    let root = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"work","name":"根层"}],"steps":[{"key":"child","name":"子流程","stage":"work","executor":{"type":"workflow","workflow_ref":middle["id"]}}]}),
    );
    let project = command(&engine, "project.create", None, json!({"title":"暂停边界"})).unwrap();
    let run = command(
        &engine,
        "workflow.start",
        project["id"].as_str(),
        json!({"workflow_ref":root["id"],"role_agent_bindings":{}}),
    )
    .unwrap();
    for _ in 0..3 {
        control_fixture::evaluate(&engine).unwrap();
    }
    let intent = engine.pending_workflow_intents().unwrap().remove(0);
    let prepared: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
    command(&engine, "workflow.pause", run["id"].as_str(), json!({})).unwrap();
    assert!(engine.pending_workflow_intents().unwrap().is_empty());
    assert!(engine.configured_platform_request(&prepared).is_err());
    let state = engine.store.snapshot().unwrap();
    let child = state
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.data["workflow_ref"] == middle["id"])
        .unwrap()
        .id
        .clone();
    command(&engine, "workflow.pause", Some(&child), json!({})).unwrap();
    command(&engine, "workflow.resume", run["id"].as_str(), json!({})).unwrap();
    assert!(engine.pending_workflow_intents().unwrap().is_empty());
    command(&engine, "workflow.resume", Some(&child), json!({})).unwrap();
    assert_eq!(engine.pending_workflow_intents().unwrap().len(), 1);
    drive(&engine, 8);
    assert_eq!(instance(&engine, &run["id"])["status"], "completed");
}

#[test]
fn supplemental_human_wait_does_not_consume_the_configured_execution_budget() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine(dir.path());
    let recorder = Arc::new(RecordingAgent::default());
    recorder
        .responses
        .lock()
        .unwrap()
        .push(json!({"status":"needs_input","summary":"请补充资料","outputs":{}}));
    engine.harness = recorder;
    let (role, agent) = research_agent(&engine);
    let run = launch(
        &engine,
        &role,
        &agent,
        json!({"budget":{"max_turns":3,"timeout_seconds":10,"retry_limit":1},"outputs":{"risks":{"delivery_ref":"builtin-delivery-object-v1"}}}),
    );
    drive(&engine, 8);
    assert_eq!(
        instance(&engine, &run["id"])["steps"][0]["status"],
        "waiting_input"
    );
    let fake_past = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        - 60_000;
    engine
        .store
        .transact(None, &mut |state| {
            let row = state
                .governance
                .iter_mut()
                .find(|r| Some(r.id.as_str()) == run["id"].as_str())
                .unwrap();
            row.data["steps"][0]["started"] = json!(fake_past);
            row.data["steps"][0]["budget_wait_started"] = json!(fake_past);
            row.content_hash = configuration_hash(&row.data);
            Ok(Value::Null)
        })
        .unwrap();
    command(
        &engine,
        "workflow.step.input",
        run["id"].as_str(),
        json!({"step":"study","input":"当前输入资料"}),
    )
    .unwrap();
    drive(&engine, 8);
    let runtime = instance(&engine, &run["id"]);
    assert_eq!(runtime["status"], "completed");
    assert!(runtime["steps"][0]["budget_waited_ms"].as_u64().unwrap() >= 60_000);
}

#[test]
fn task_creation_keeps_business_metadata_and_dependent_worktrees_use_the_latest_parent_revision() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let project = command(&engine, "project.create", None, json!({"title":"任务依赖"})).unwrap();
    let project = project["id"].as_str().unwrap().to_owned();
    let feature = id();
    engine
        .store
        .transact(None, &mut |state| {
            state.features.push(Feature {
                id: feature.clone(),
                project_id: project.clone(),
                title: "工程范围".into(),
                status: Status::Draft,
                revision: 1,
                input_version: 1,

                predecessor: None,
                source_materials: vec![],
            });
            state.heads.push(GitHead {
                id: feature.clone(),
                project_id: project.clone(),
                revision: "a".repeat(40),
            });
            Ok(Value::Null)
        })
        .unwrap();
    let workflow = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"work","name":"任务创建"}],"steps":[{"key":"tasks","name":"建立任务","stage":"work","executor":{"type":"tool","tool_ref":"builtin-tool-tasks-create-v1","arguments":{"tasks":{"type":"constant","value":[{"key":"first","title":"第一项","instructions":"初始工作","dependencies":[],"business_metadata":{"category":"investigation"}},{"key":"second","title":"后继项","instructions":"使用前项结果","dependencies":["first"]}]}}},"outputs":{"tasks":{"delivery_ref":"builtin-delivery-object-v1"}}}]}),
    );
    let run = engine
        .execute(
            "workflow.start",
            Command {
                command_id: id(),
                target: Some(feature.clone()),
                expected_revision: Some(1),
                payload: json!({"workflow_ref":workflow["id"],"role_agent_bindings":{}}),
            },
        )
        .unwrap();
    drive(&engine, 6);
    let state = engine.store.snapshot().unwrap();
    let row = state
        .configuration(
            instance(&engine, &run["id"])["steps"][0]["outputs"]["tasks"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        row.data["value"]["tasks"][0]["business_metadata"]["category"],
        "investigation"
    );
    let first = state
        .tasks
        .iter()
        .find(|t| t.title == "第一项")
        .unwrap()
        .id
        .clone();
    let second = state
        .tasks
        .iter()
        .find(|t| t.title == "后继项")
        .unwrap()
        .id
        .clone();
    engine.prepare_task_worktrees().unwrap();
    let state = engine.store.snapshot().unwrap();
    assert!(state.repository_operations.iter().any(|op|matches!(&op.action,RepositoryAction::CreateWorktree{owner,base,..} if owner==&first&&base==&"a".repeat(40))));
    assert!(!state.repository_operations.iter().any(
        |op| matches!(&op.action,RepositoryAction::CreateWorktree{owner,..} if owner==&second)
    ));
    // Explicit scheduling fixture: the dependency's external publication has been projected.
    engine
        .store
        .transact(None, &mut |state| {
            state
                .tasks
                .iter_mut()
                .find(|t| t.id == first)
                .unwrap()
                .status = Status::Completed;
            state
                .heads
                .iter_mut()
                .find(|h| h.id == feature)
                .unwrap()
                .revision = "b".repeat(40);
            Ok(Value::Null)
        })
        .unwrap();
    engine.prepare_task_worktrees().unwrap();
    assert!(engine.store.snapshot().unwrap().repository_operations.iter().any(|op|matches!(&op.action,RepositoryAction::CreateWorktree{owner,base,..} if owner==&second&&base==&"b".repeat(40))));
}

#[test]
fn canonical_output_contract_describes_text_values_dynamic_checks_and_conditional_required_ports() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let role = command(
        &engine,
        "role.create",
        None,
        json!({"name":"自由评估员","data":{"instructions":"核对当前输入"}}),
    )
    .unwrap();
    let spec=command(&engine,"spec.create",None,json!({"name":"安全检查","data":{"schema_version":3,"body":"只核对本次安全事实","applicable_to":[{"role_ref":role["definition_id"]}],"checks":[{"key":"safety","name":"安全","instructions":"记录明确依据"}]}})).unwrap();
    let text = command(
        &engine,
        "delivery_type.create",
        None,
        json!({"name":"调研正文","data":{"body":"普通 Markdown 正文","format":"markdown"}}),
    )
    .unwrap();
    let report = command(
        &engine,
        "delivery_type.create",
        None,
        json!({"name":"动态报告","data":{"body":"按本次检查集合提交报告","format":"json"}}),
    )
    .unwrap();
    let workflow = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"work","name":"自定义评估"}],"steps":[{"key":"evaluate","name":"评估","stage":"work","executor":{"type":"agent","role_ref":role["id"]},"task_instructions":"输出正文与内容判断。","spec_refs":[spec["id"]],"outputs":{"document":{"delivery_ref":text["id"]},"review":{"delivery_ref":report["id"],"content_checks":true}}}]}),
    );
    let preview = engine
        .query(
            "query.workflow_task_preview",
            &json!({"id":workflow["id"],"step":"evaluate"}),
            4,
        )
        .unwrap();
    let schema = &preview["task"]["output_contract"];
    let validator = devdesk_adapters::integrations::NativeTools::new(dir.path()).unwrap();
    let value = json!({"status":"completed","summary":"已检查","outputs":{"document":"# 正文","review":{"approve":true,"summary":"已核实","checks":[{"key":"safety","passed":true,"evidence":"输入一"}]}}});
    validator.validate_arguments(schema, &value).unwrap();
    let mut wrong = value.clone();
    wrong["outputs"]["document"] = json!({"title":"旧包装","body":"正文"});
    assert!(validator.validate_arguments(schema, &wrong).is_err());
    let mut wrong = value.clone();
    wrong["outputs"]["review"]["checks"][0]["key"] = json!("traceability");
    assert!(validator.validate_arguments(schema, &wrong).is_err());
    assert!(
        validator
            .validate_arguments(
                schema,
                &json!({"status":"completed","summary":"缺少输出","outputs":{}})
            )
            .is_err()
    );
    validator
        .validate_arguments(
            schema,
            &json!({"status":"needs_input","summary":"需要澄清","outputs":{}}),
        )
        .unwrap();
}

#[test]
fn failed_child_send_is_reported_and_retry_resumes_the_parent_invocation() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine(dir.path());
    engine.harness = Arc::new(RecordingAgent::default());
    let (role, agent) = research_agent(&engine);
    let child = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"work","name":"子工作"}],"steps":[{"key":"study","name":"调查","stage":"work","executor":{"type":"agent","role_ref":role["definition_id"]},"task_instructions":"处理当前任务","outputs":{"risks":{"delivery_ref":"builtin-delivery-object-v1"}}}]}),
    );
    let parent = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"work","name":"父工作"}],"steps":[{"key":"child","name":"子调用","stage":"work","executor":{"type":"workflow","workflow_ref":child["id"]}}]}),
    );
    let project = command(&engine, "project.create", None, json!({"title":"派发恢复"})).unwrap();
    let run=command(&engine,"workflow.start",project["id"].as_str(),json!({"workflow_ref":parent["id"],"role_agent_bindings":{role["definition_id"].as_str().unwrap():agent["id"]}})).unwrap();
    for _ in 0..6 {
        control_fixture::evaluate(&engine).unwrap();
        for intent in engine.pending_workflow_intents().unwrap() {
            let method = intent.data["method"].as_str().unwrap();
            let mut cmd: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
            let result = if method == "conversation.send" {
                Err(err("blocked", "explicit dispatch failure fixture"))
            } else {
                cmd.payload["binding"] = json!(fixture_binding());
                engine.execute(method, cmd)
            };
            engine.observe_workflow_intent(&intent.id, &result).unwrap();
        }
    }
    control_fixture::evaluate(&engine).unwrap();
    control_fixture::evaluate(&engine).unwrap();
    let state = engine.store.snapshot().unwrap();
    let child = state
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.data["parent_execution_id"].is_string())
        .unwrap()
        .clone();
    assert_eq!(child.data["steps"][0]["status"], "failed");
    assert!(
        child.data["steps"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("dispatch failure")
    );
    assert_eq!(instance(&engine, &run["id"])["status"], "failed");
    command(
        &engine,
        "workflow.step.retry",
        Some(&child.id),
        json!({"step":"study"}),
    )
    .unwrap();
    assert_eq!(instance(&engine, &run["id"])["status"], "running");
    drive(&engine, 8);
    assert_eq!(instance(&engine, &run["id"])["status"], "completed");
}

#[derive(Default)]
struct PlatformCallingAgent {
    base: RecordingAgent,
    replies: Mutex<Vec<ToolReceipt>>,
}
impl ports::Harness for PlatformCallingAgent {
    fn open_session(&self, request: &ports::SessionStart) -> Result<ports::SessionReceipt> {
        self.base.open_session(request)
    }
    fn dispatch_turn(&self, request: &ports::TurnDispatch) -> Result<()> {
        use ports::ConversationObservation as Event;
        self.base.calls.lock().unwrap().push(request.clone());
        self.base.pending.lock().unwrap().insert(
            request.session.id.clone(),
            vec![
                Event::Admitted {
                    turn: request.turn.id.clone(),
                    message: Some("native-message".into()),
                    external_turn: Some(format!("fixture-{}", request.turn.id)),
                },
                Event::Consumed {
                    turn: request.turn.id.clone(),
                    message: Some("native-message".into()),
                },
                Event::ToolCall {
                    turn: request.turn.id.clone(),
                    request: json!(47),
                    thread: format!("fixture-{}", request.session.id),
                    external_turn: format!("fixture-{}", request.turn.id),
                    call: "native-call-47".into(),
                    tool: "devdesk_materials_capture".into(),
                    arguments: json!({"note":"可复核的实际输入"}),
                    worktree: String::new(),
                },
            ],
        );
        Ok(())
    }
    fn answer_tool_call(
        &self,
        session: &Session,
        request: &Value,
        receipt: &ToolReceipt,
    ) -> Result<()> {
        assert_eq!(*request, json!(47));
        self.replies.lock().unwrap().push(receipt.clone());
        let calls = self.base.calls.lock().unwrap();
        let turn = &calls.last().unwrap().turn;
        self.base.pending.lock().unwrap().insert(
            session.id.clone(),
            vec![
                ports::ConversationObservation::Message {
                    turn: turn.id.clone(),
                    key: "native-final".into(),
                    content:
                        json!({"status":"completed","summary":"已收到实际平台回执","outputs":{}})
                            .to_string(),
                    kind: ConversationEventKind::Assistant,
                    mode: MessageMode::Full,
                    item: "native-final".into(),
                },
                ports::ConversationObservation::Terminal {
                    turn: turn.id.clone(),
                    outcome: ports::ProviderTerminal::Completed,
                    reason: "explicit native bridge fixture".into(),
                    quiescent: true,
                },
            ],
        );
        Ok(())
    }
    fn poll_session(&self, session: &Session) -> Result<Vec<ports::ConversationObservation>> {
        self.base.poll_session(session)
    }
    fn stop_session(
        &self,
        session: &Session,
        execution: &RuntimeExecution,
    ) -> Result<ports::WorkerStop> {
        self.base.stop_session(session, execution)
    }
    fn cleanup_worker(&self, session: &Session, execution: &RuntimeExecution) -> Result<()> {
        self.base.cleanup_worker(session, execution)
    }
}

fn agent_platform_case(reject_schema: bool, cancel: bool, reject_receipt: bool) {
    let directory = tempfile::tempdir().unwrap();
    let mut engine = engine(directory.path());
    let harness = Arc::new(PlatformCallingAgent::default());
    engine.harness = harness.clone();
    let tool=command(&engine,"tool.create",None,json!({"name":"捕获调查材料","data":{"qualified_name":"materials.capture","implementation":"platform","input_schema":{"type":"object","required":["note"],"properties":{"note":{"type":"string","const":if reject_schema {"different input"} else {"可复核的实际输入"}}},"additionalProperties":false},"output_schema":if reject_receipt {json!({"type":"object","required":["missing"]})} else {json!({"type":"object"})}}})).unwrap();
    let role = command(
        &engine,
        "role.create",
        None,
        json!({"name":"材料调查员","data":{"instructions":"使用当前授权工具"}}),
    )
    .unwrap();
    let capability=command(&engine,"capability.create",None,json!({"name":"平台工具能力","data":{"model_ref":DEFAULT_MODEL_ID,"harness_ref":DEFAULT_HARNESS_ID,"skills":[],"tools":[DEFAULT_NATIVE_TOOL_ID,tool["id"]]}})).unwrap();
    let agent=command(&engine,"agent.create",None,json!({"name":"平台调查 Agent","data":{"role_ref":role["id"],"capability_ref":capability["id"],"permission_selection":default_permission_selection()}})).unwrap();
    let run = launch(
        &engine,
        &role,
        &agent,
        json!({"access":{"tool_refs":[tool["id"]]}}),
    );
    let mut checked = false;
    for _ in 0..16 {
        control_fixture::evaluate(&engine).unwrap();
        for intent in engine.pending_workflow_intents().unwrap() {
            let method = intent.data["method"].as_str().unwrap();
            let mut invocation: Command =
                serde_json::from_value(intent.data["command"].clone()).unwrap();
            let result = if method == "session.open" {
                let configured = engine
                    .configured_agent_request(&invocation)
                    .unwrap()
                    .unwrap();
                let state = engine.store.snapshot().unwrap();
                let selected = state.configuration(agent["id"].as_str().unwrap()).unwrap();
                let composition: AgentComposition = selected.decode().unwrap();
                let mut binding = fixture_binding();
                binding.agent_configuration = Some(AgentExecutionConfiguration {
                    integration_lock: Some(configured.integration_lock),
                    agent_instance_id: "actual-fixture-agent-instance".into(),
                    agent_revision_id: selected.id.clone(),
                    composition_hash: selected.content_hash.clone(),
                    model_revision_id: composition.model_ref,
                    harness_revision_id: composition.harness_ref,
                    model_provider: "openai".into(),
                    model_parameters: Default::default(),
                    tool_revisions: composition.tools,
                });
                binding.permissions = Some(
                    devdesk_adapters::permission_profiles::compile_permission_profiles(
                        &state,
                        &configured.permissions,
                        Some(ExecutionScope {
                            work_type: WorkType::Workflow,
                            writable_paths: vec![],
                            read_only_paths: vec![],
                        }),
                    )
                    .unwrap(),
                );
                invocation.payload["binding"] = json!(binding);
                engine.execute(method, invocation)
            } else if method == "workflow.tool.execute" {
                let expected = engine
                    .configured_platform_request(&invocation)
                    .unwrap()
                    .unwrap();
                assert!(expected.caller.is_some());
                let mut forged = expected.clone();
                forged.arguments = json!({"note":"篡改参数"});
                assert!(engine.enqueue_platform_tool(&invocation, &forged).is_err());
                checked = true;
                let result = engine.enqueue_platform_tool(&invocation, &expected);
                if cancel && result.is_ok() {
                    command(
                        &engine,
                        "tool.call.cancel",
                        Some(&expected.execution_id),
                        json!({}),
                    )
                    .unwrap();
                }
                result
            } else {
                engine.execute(method, invocation)
            };
            engine.observe_workflow_intent(&intent.id, &result).unwrap();
            result.unwrap();
        }
        engine.drive_platform_tools().unwrap();
        engine.tick_conversations(4).unwrap();
    }
    assert_eq!(checked, !reject_schema);
    assert_eq!(instance(&engine, &run["id"])["status"], "completed");
    let receipts = harness.replies.lock().unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(
        receipts[0].success,
        !reject_schema && !cancel && !reject_receipt
    );
    if receipts[0].success {
        assert_eq!(
            receipts[0].structured_content.as_ref().unwrap()["inputs"]["note"],
            "可复核的实际输入"
        );
    }
    let state = engine.store.snapshot().unwrap();
    let call = state
        .governance
        .iter()
        .find(|record| record.kind == "platform_call")
        .unwrap();
    let caller = &call.data["request"]["caller"];
    assert_eq!(caller["session_id"], state.sessions[0].id);
    assert_eq!(caller["turn_id"], state.turns[0].id);
    assert!(
        state
            .governance
            .iter()
            .any(|record| record.kind == "platform_tool_reply"
                && record.definition_id == call.id
                && record.data["status"] == "delivered")
    );
    assert!(
        state
            .governance
            .iter()
            .all(|record| record.kind != "tool_call")
    );
    let history = engine
        .query(
            "query.tool_calls",
            &json!({"turn_id":state.turns[0].id}),
            10,
        )
        .unwrap();
    assert_eq!(history["records"].as_array().unwrap().len(), 1);
    assert_eq!(history["records"][0]["id"], call.id);
    assert!(
        engine
            .query("query.tool_calls", &json!({"turn_id":"unrelated"}), 10)
            .unwrap()["records"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    if reject_receipt {
        assert_eq!(call.data["error"]["code"], "receipt_rejected");
        assert_eq!(
            call.data["observed_receipt"]["inputs"]["note"],
            "可复核的实际输入"
        );
    }
}

#[test]
fn agent_and_workflow_use_the_same_platform_handler_with_exact_native_call_identity() {
    agent_platform_case(false, false, false);
}
#[test]
fn agent_platform_schema_rejection_returns_an_error_without_executing_the_handler() {
    agent_platform_case(true, false, false);
}
#[test]
fn cancelling_an_agent_platform_call_prevents_dispatch_and_returns_the_cancelled_receipt() {
    agent_platform_case(false, true, false);
}

#[test]
fn rejected_platform_receipt_fails_promptly_and_keeps_the_observed_evidence() {
    agent_platform_case(false, false, true);
}

#[test]
fn configured_task_completion_works_without_git_and_releases_declared_dependencies() {
    let directory = tempfile::tempdir().unwrap();
    let engine = engine(directory.path());
    let project = command(
        &engine,
        "project.create",
        None,
        json!({"title":"纯材料工作"}),
    )
    .unwrap();
    let feature = id();
    engine
        .store
        .transact(None, &mut |state| {
            state.features.push(Feature {
                id: feature.clone(),
                project_id: project["id"].as_str().unwrap().into(),
                title: "调查任务".into(),
                status: Status::Draft,
                revision: 1,
                input_version: 1,

                predecessor: None,
                source_materials: vec![],
            });
            Ok(Value::Null)
        })
        .unwrap();
    let workflow = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"research","name":"材料调查"}],"steps":[
            {"key":"create","name":"创建明确任务","stage":"research","executor":{"type":"tool","tool_ref":"builtin-tool-tasks-create-v1","arguments":{"tasks":{"type":"constant","value":[{"key":"first","title":"整理材料","dependencies":[]},{"key":"second","title":"核对材料","dependencies":["first"]}]}}},"outputs":{"result":{"delivery_ref":"builtin-delivery-object-v1"}}},
            {"key":"first","name":"接纳第一项","stage":"research","dependencies":["create"],"executor":{"type":"tool","tool_ref":"builtin-tool-tasks-complete-v1","arguments":{"task_id":{"type":"source","source":{"step":"create","port":"result","pointer":"/tasks/0/id"}}}}},
            {"key":"second","name":"接纳后继项","stage":"research","dependencies":["first"],"executor":{"type":"tool","tool_ref":"builtin-tool-tasks-complete-v1","arguments":{"task_id":{"type":"source","source":{"step":"create","port":"result","pointer":"/tasks/1/id"}}}}}
        ]}),
    );
    let run = command(
        &engine,
        "workflow.start",
        Some(&feature),
        json!({"workflow_ref":workflow["id"],"role_agent_bindings":{}}),
    )
    .unwrap();
    drive(&engine, 14);
    let state = engine.store.snapshot().unwrap();
    assert_eq!(instance(&engine, &run["id"])["status"], "completed");
    assert_eq!(state.tasks.len(), 2);
    assert!(
        state
            .tasks
            .iter()
            .all(|task| task.status == Status::Completed)
    );
    assert!(state.sessions.is_empty());
    assert!(state.repository_operations.is_empty());
    assert!(
        state
            .governance
            .iter()
            .filter(|record| record.kind == "platform_call")
            .all(
                |record| !record.data["request"]["definition"]["qualified_name"]
                    .as_str()
                    .unwrap()
                    .starts_with("git.")
            )
    );
}

#[test]
fn task_agent_steps_own_distinct_sessions_even_while_the_same_agent_has_an_active_turn() {
    let directory = tempfile::tempdir().unwrap();
    let engine = engine(directory.path());
    let (role, agent) = research_agent(&engine);
    let project = command(
        &engine,
        "project.create",
        None,
        json!({"title":"独立步骤会话"}),
    )
    .unwrap();
    let feature = id();
    let task = id();
    engine
        .store
        .transact(None, &mut |state| {
            state.features.push(Feature {
                id: feature.clone(),
                project_id: project["id"].as_str().unwrap().into(),
                title: "调查".into(),
                status: Status::Draft,
                revision: 1,
                input_version: 1,
                predecessor: None,
                source_materials: vec![],
            });
            state.tasks.push(Task {
                id: task.clone(),
                feature_id: feature.clone(),
                title: "两项独立调查".into(),
                status: Status::Draft,
                revision: 1,
                inherited_snapshot: None,
                dependencies: vec![],
                worktree: String::new(),
                scope: TaskScope::default(),
            });
            Ok(Value::Null)
        })
        .unwrap();
    let workflow = publish(
        &engine,
        json!({"schema_version":5,"stages":[{"key":"research","name":"调查","default_role_ref":role["definition_id"]}],"steps":[
            {"key":"first","name":"第一项","stage":"research","executor":{"type":"agent"},"task_instructions":"分析第一项材料"},
            {"key":"gate","name":"继续独立调查","stage":"research","executor":{"type":"control"},"confirmation":"human"},
            {"key":"second","name":"第二项","stage":"research","dependencies":["gate"],"executor":{"type":"agent"},"task_instructions":"分析第二项材料"}
        ]}),
    );
    let run = command(&engine, "workflow.start", Some(&task), json!({"workflow_ref":workflow["id"],"role_agent_bindings":{role["definition_id"].as_str().unwrap():agent["id"]}})).unwrap();
    // Prepare the first Turn without dispatching it. It remains an actual active queued Turn.
    for _ in 0..2 {
        control_fixture::evaluate(&engine).unwrap();
        for intent in engine.pending_workflow_intents().unwrap() {
            let method = intent.data["method"].as_str().unwrap();
            let mut request: Command =
                serde_json::from_value(intent.data["command"].clone()).unwrap();
            if method == "session.open" {
                request.payload["binding"] = json!(fixture_binding());
            }
            let result = engine.execute(method, request);
            engine.observe_workflow_intent(&intent.id, &result).unwrap();
            result.unwrap();
        }
    }
    let current = instance(&engine, &run["id"]);
    let gate = &current["steps"][1];
    command(
        &engine,
        "workflow.step.confirm",
        run["id"].as_str(),
        json!({"step":"gate","input_digest":gate["input_digest"],"outputs":gate["outputs"]}),
    )
    .unwrap();
    control_fixture::evaluate(&engine).unwrap();
    let intents = engine.pending_workflow_intents().unwrap();
    assert_eq!(intents.len(), 1);
    let intent = &intents[0];
    assert_eq!(intent.data["method"], "session.open");
    let mut request: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
    request.payload["binding"] = json!(fixture_binding());
    let result = engine.execute("session.open", request);
    engine.observe_workflow_intent(&intent.id, &result).unwrap();
    result.unwrap();
    let state = engine.store.snapshot().unwrap();
    assert_eq!(state.sessions.len(), 2);
    assert_ne!(state.sessions[0].id, state.sessions[1].id);
    assert!(
        state
            .sessions
            .iter()
            .all(|session| session.task_id.as_deref() == Some(&task))
    );
    assert!(state.turns.iter().any(|turn| turn.status.active()));
    assert_ne!(state.sessions[0].runtime_binding, None);
}

#[test]
fn changing_a_running_task_waits_for_stop_and_never_reuses_the_old_contract() {
    use std::sync::atomic::Ordering;
    let directory = tempfile::tempdir().unwrap();
    let mut engine = engine(directory.path());
    let recorder = Arc::new(RecordingAgent::default());
    recorder.hold.store(true, Ordering::SeqCst);
    engine.harness = recorder.clone();
    let (role, agent) = research_agent(&engine);
    let run = launch(
        &engine,
        &role,
        &agent,
        json!({"task_instructions":"原任务只调查原材料。","outputs":{"risks":{"delivery_ref":"builtin-delivery-object-v1"}}}),
    );
    drive(&engine, 3);
    let before = instance(&engine, &run["id"]);
    assert_eq!(before["steps"][0]["status"], "running");
    let old = before["steps"][0].clone();
    assert!(old["turn_id"].is_string());
    let mut step = before["plan"]["document"]["steps"][0].clone();
    step["task_instructions"] = json!("新任务调查更新后的材料，不复用原调查。");
    let changes = json!({"plan_revision_id":before["active_plan_revision_id"],"reason":"用户修改进行中的调查范围","operations":[{"type":"upsert_step","step":step}]});
    let preview = engine
        .query(
            "workflow.change.preview",
            &json!({"id":run["id"],"changeset":changes}),
            4,
        )
        .unwrap();
    assert_eq!(preview["must_stop"].as_array().unwrap().len(), 1);
    let applied=command(&engine,"workflow.change.apply",run["id"].as_str(),json!({"plan_revision_id":before["active_plan_revision_id"],"changeset":changes,"preview_hash":preview["preview_hash"]})).unwrap();
    assert_eq!(applied["status"], "waiting_quiescence");
    assert_eq!(
        instance(&engine, &run["id"])["active_plan_revision_id"],
        before["active_plan_revision_id"]
    );
    drop(engine);
    let mut engine = self::engine(directory.path());
    engine.harness = recorder.clone();
    engine.tick_conversations(4).unwrap();
    engine.drive_workflow_changes().unwrap();
    let after = instance(&engine, &run["id"]);
    assert_ne!(
        after["active_plan_revision_id"],
        before["active_plan_revision_id"]
    );
    assert_ne!(after["steps"][0]["id"], old["id"]);
    assert_eq!(after["steps"][0]["outputs"], json!({}));
    let state = engine.store.snapshot().unwrap();
    assert!(
        state
            .executions
            .iter()
            .filter(|e| e.session_id == old["session_id"])
            .all(|e| e.writer_revoked)
    );
    assert!(
        state
            .governance
            .iter()
            .any(|r| r.kind == "workflow_execution_history"
                && r.definition_id == old["id"]
                && r.data["contract_digest"] == old["contract_digest"])
    );
    let history = engine
        .query("query.workflow_instance", &json!({"id":run["id"]}), 4)
        .unwrap();
    assert_eq!(
        history["history"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"] == "workflow_execution_history" && r["data"]["id"] == old["id"])
            .unwrap()["frozen_step"]["task_instructions"],
        "原任务只调查原材料。"
    );
    recorder.hold.store(false, Ordering::SeqCst);
    drive(&engine, 6);
    let final_state = instance(&engine, &run["id"]);
    assert_eq!(final_state["status"], "completed");
    assert_ne!(final_state["steps"][0]["session_id"], old["session_id"]);
    assert_ne!(
        final_state["steps"][0]["contract_digest"],
        old["contract_digest"]
    );
    assert!(
        recorder
            .pending
            .lock()
            .unwrap()
            .contains_key(old["session_id"].as_str().unwrap()),
        "late old messages stay associated with the retired worker"
    );
}
