#[path = "../../../tests/support/runtime.rs"]
mod runtime;
use devdesk_adapters::*;
use devdesk_core::{ports::*, *};
use devdesk_domain::*;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct CallingHarness {
    pending: Mutex<Vec<ConversationObservation>>,
    replies: Mutex<Vec<ToolReceipt>>,
}
impl devdesk_core::ports::Harness for CallingHarness {
    fn open_session(&self, r: &SessionStart) -> Result<SessionReceipt> {
        Ok(SessionReceipt {
            external_session: "context-thread".into(),
            container_id: "fixture".into(),
            image_digest: r.execution.image_digest.clone(),
            operation_id: r.execution.operation_id.clone(),
        })
    }
    fn dispatch_turn(&self, r: &TurnDispatch) -> Result<()> {
        let mut events = self.pending.lock().unwrap();
        events.push(ConversationObservation::Admitted {
            turn: r.turn.id.clone(),
            message: Some("message".into()),
            external_turn: Some("context-turn".into()),
        });
        events.push(ConversationObservation::Consumed {
            turn: r.turn.id.clone(),
            message: Some("message".into()),
        });
        for (call, tool, args) in [
            ("list", "devdesk_agent_experience_list", json!({})),
            (
                "forged",
                "devdesk_agent_experience_list",
                json!({"agent_instance_id":"someone-else"}),
            ),
            (
                "lint",
                "devdesk_spec_lint",
                json!({"delivery_ref":"fixture-delivery-object:1","value":{}}),
            ),
            (
                "missing",
                "devdesk_workflow_outputs_read",
                json!({"ids":["unknown"]}),
            ),
            (
                "foreign-spec",
                "devdesk_spec_lint",
                json!({"delivery_ref":"fixture-delivery-private:1","value":{}}),
            ),
        ] {
            events.push(ConversationObservation::ToolCall {
                turn: r.turn.id.clone(),
                request: json!(call),
                thread: "context-thread".into(),
                external_turn: "context-turn".into(),
                call: call.into(),
                tool: tool.into(),
                arguments: args,
                worktree: "fixture".into(),
            });
        }
        Ok(())
    }
    fn poll_session(&self, _: &Session) -> Result<Vec<ConversationObservation>> {
        Ok(std::mem::take(&mut *self.pending.lock().unwrap()))
    }
    fn answer_tool_call(&self, _: &Session, _: &Value, r: &ToolReceipt) -> Result<()> {
        self.replies.lock().unwrap().push(r.clone());
        Ok(())
    }
}
#[derive(Default)]
struct ContextReader {
    instances: Mutex<Vec<String>>,
}
impl AgentContextReader for ContextReader {
    fn experience(&self, instance: &str, _: Option<&str>, _: &Value) -> Result<Value> {
        self.instances.lock().unwrap().push(instance.into());
        Ok(json!({"records":[],"total":0}))
    }
    fn workflow_outputs(&self, _: &str, _: Option<&str>, _: &Value) -> Result<Value> {
        Err(err("context_required", "读取产物需要 instance_id"))
    }
}
fn engine(root: &std::path::Path) -> Engine {
    let e = Engine::new(
        Arc::new(SqliteStore::open(root).unwrap()),
        Arc::new(FileArtifacts::new(root.into())),
        runtime::adapters(),
    )
    .with_tool_executor(Arc::new(integrations::NativeTools::new(root).unwrap()));
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    e
}
#[test]
fn discussion_keeps_agent_tools_and_common_context_without_a_workflow() {
    context_case(false);
    context_case(true);
}
fn context_case(detached: bool) {
    let dir = tempfile::tempdir().unwrap();
    let mut e = engine(dir.path());
    let harness = Arc::new(CallingHarness::default());
    let reader = Arc::new(ContextReader::default());
    e.harness = harness.clone();
    e.context_reader = Some(reader.clone());
    e.store
        .transact(None, &mut |state| {
            let mut source = state.configuration("fixture-delivery-object:1")?.source();
            source.identity = "fixture-delivery-private".into();
            source.scope = Scope::project(id());
            state.governance.push(source.into_record()?);
            Ok(Value::Null)
        })
        .unwrap();
    let project = if detached {
        GLOBAL_DISCUSSION_NAMESPACE.into()
    } else {
        e.execute(
            "project.create",
            Command {
                command_id: id(),
                target: None,
                expected_revision: None,
                payload: json!({"title":"No workflow"}),
            },
        )
        .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let instance=e.execute("agent.instance.create",Command{command_id:id(),target:None,expected_revision:None,payload:json!({"name":"Identity","agent_revision_id":"builtin-agent-sol-high-analyst:6","scope":"global"})}).unwrap()["id"].as_str().unwrap().to_owned();
    let (session, turn) = runtime::queue_conversation_with(&e, &project, |state, binding| {
        let agent = state.resolve_agent("builtin-agent-sol-high-analyst:6")?;
        let model: ModelDefinition = state.configuration(&agent.model_ref)?.decode()?;
        let integration_lock = freeze_discussion_integrations(state, &agent, Some(&project))?;
        assert_eq!(integration_lock.platform_tools.len(), 2);
        let permissions =
            resolve_permission_lock(state, &agent.harness_ref, &permission_selection(state)?)?;
        let permissions = permission_profiles::compile_permission_profiles(
            state,
            &permissions,
            Some(ExecutionScope {
                work_type: WorkType::Discussion,
                writable_paths: vec![],
                read_only_paths: vec![],
            }),
        )?;
        let configuration = AgentExecutionConfiguration {
            context_protocol: Some(AGENT_CONTEXT_PROTOCOL.into()),
            integration_lock: Some(integration_lock),
            agent_instance_id: instance.clone(),
            agent_revision_id: "builtin-agent-sol-high-analyst:6".into(),
            composition_hash: state
                .configuration("builtin-agent-sol-high-analyst:6")?
                .content_hash
                .clone(),
            model_revision_id: agent.model_ref,
            harness_revision_id: agent.harness_ref,
            model_provider: model.provider,
            model_parameters: model.parameters,
            tool_revisions: agent.tools,
        };
        binding.agent_configuration = Some(configuration);
        binding.model = model.model;
        runtime::set_permissions(binding, permissions);
        Ok(())
    });
    e.tick_conversations(1).unwrap();
    e.tick_conversations(1).unwrap();
    e.drive_platform_tools().unwrap();
    let replies = harness.replies.lock().unwrap();
    assert_eq!(replies.len(), 5);
    assert_eq!(replies.iter().filter(|r| r.success).count(), 2);
    assert_eq!(&*reader.instances.lock().unwrap(), &[instance]);
    assert!(replies.iter().any(|r| {
        r.error
            .as_ref()
            .is_some_and(|v| v["code"] == "context_required")
    }));
    drop(replies);
    e.drive_platform_tools().unwrap();
    assert_eq!(harness.replies.lock().unwrap().len(), 5);
    let snapshot = e.store.snapshot().unwrap();
    assert!(
        !snapshot
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_instance")
    );
    assert_eq!(
        snapshot.turns.iter().find(|t| t.id == turn).unwrap().status,
        TurnStatus::Running
    );
    assert_eq!(
        e.query("query.tool_calls", &json!({"session_id":session}), 1)
            .unwrap()["total"],
        5
    );
}

#[test]
fn agent_specs_are_exact_capability_references_and_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let payload = json!({"name":"Analysis with norms","model_ref":"builtin-model-gpt-6-1-sol-high:1","harness_ref":"builtin-harness-codex-managed:1","tools":["builtin-tool-codex-native:1","builtin-tool-spec-lint:1"],"skills":["builtin-skill-prd:2"],"spec_refs":["fixture-spec:1"],"data":{}});
    let capability = e
        .execute(
            "capability.create",
            Command {
                command_id: id(),
                target: None,
                expected_revision: None,
                payload: payload.clone(),
            },
        )
        .unwrap();
    let agent=e.execute("agent.create",Command{command_id:id(),target:None,expected_revision:None,payload:json!({"name":"Analyst","role_ref":"builtin-role-analyst:1","capability_ref":capability["id"],"data":{}})}).unwrap();
    let mut invalid = payload;
    invalid["spec_refs"] = json!(["builtin-tool-codex-native:1"]);
    assert!(
        e.execute(
            "capability.create",
            Command {
                command_id: id(),
                target: None,
                expected_revision: None,
                payload: invalid
            }
        )
        .is_err()
    );
    drop(e);
    let e = engine(dir.path());
    let state = e.store.snapshot().unwrap();
    let composition = state.resolve_agent(agent["id"].as_str().unwrap()).unwrap();
    assert_eq!(composition.spec_refs, vec!["fixture-spec:1"]);
    let instance=e.execute("agent.instance.create",Command{command_id:id(),target:None,expected_revision:None,payload:json!({"name":"Detached","agent_revision_id":agent["id"],"scope":"global"})}).unwrap();
    let mut command = Command {
        command_id: id(),
        target: Some("global".into()),
        expected_revision: Some(0),
        payload: json!({"agent_instance_id":instance["id"]}),
    };
    e.resolve_session_composition(
        &runtime::ResourceFixture {
            store: e.store.as_ref(),
        },
        &mut command,
    )
    .unwrap();
    assert_eq!(
        command.payload["agent_configuration"]["integration_lock"]["platform_tools"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(e.store.snapshot().unwrap().projects.is_empty());
}

#[test]
fn missing_external_account_keeps_the_declared_tool_available_for_discussion() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let command = |method: &str, payload: Value| {
        e.execute(
            method,
            Command {
                command_id: id(),
                target: payload["id"].as_str().map(str::to_owned),
                expected_revision: None,
                payload,
            },
        )
        .unwrap()
    };
    let source = command(
        "integration.create",
        json!({"name":"Tickets","data":{"source":{"kind":"http_api","base_url":"https://tickets.example.test"}}}),
    );
    let tool = command(
        "integration.tool.create",
        json!({"name":"Lookup","data":{"schema_version":2,"qualified_name":"ticket_lookup","description":"Find a ticket","integration_ref":source["id"],"implementation":{"kind":"http_api","method":"GET","path":"/tickets","parameters":[]},"input_schema":{"type":"object"},"effect":"read_only"}}),
    );
    let capability = command(
        "capability.create",
        json!({"name":"Unbound capability","model_ref":"builtin-model-gpt-6-1-sol-high:1","harness_ref":"builtin-harness-codex-managed:1","tools":["builtin-tool-codex-native:1",tool["id"]],"skills":[],"data":{}}),
    );
    let agent = command(
        "agent.create",
        json!({"name":"Unbound analyst","role_ref":"builtin-role-analyst:1","capability_ref":capability["id"],"data":{}}),
    );
    let state = e.store.snapshot().unwrap();
    let composition = state.resolve_agent(agent["id"].as_str().unwrap()).unwrap();
    let lock = freeze_discussion_integrations(&state, &composition, None).unwrap();
    assert_eq!(lock.unbound_tools.len(), 1);
    assert_eq!(lock.unbound_tools[0].id, tool["id"]);
    assert!(lock.tools.is_empty());
    assert!(state.projects.is_empty());
    assert!(lock.unbound_reasons[tool["id"].as_str().unwrap()].contains("选择项目接入绑定"));
    command("integration.disable", json!({"id":source["id"]}));
    let disabled = e.store.snapshot().unwrap();
    let lock = freeze_discussion_integrations(&disabled, &composition, None).unwrap();
    assert_eq!(lock.unbound_tools[0].id, tool["id"]);
    assert!(lock.unbound_reasons[tool["id"].as_str().unwrap()].contains("停用"));
    assert!(freeze_integrations(&disabled, &composition, None).is_err());
}

#[test]
fn historical_outputs_are_read_from_the_source_without_starting_its_workflow() {
    let root = tempfile::tempdir().unwrap();
    let global = Arc::new(GlobalStore::open(root.path()).unwrap());
    let shared = Engine::new(
        global.clone(),
        Arc::new(FileArtifacts::new(root.path().into())),
        runtime::adapters(),
    );
    shared.bootstrap().unwrap();
    let create_instance = || {
        shared.execute("agent.instance.create", Command {command_id:id(),target:None,expected_revision:None,payload:json!({"name":"Historical reader","agent_revision_id":"builtin-agent-sol-high-analyst:6","scope":"global"})}).unwrap()["id"].as_str().unwrap().to_owned()
    };
    let instance = create_instance();
    let other = create_instance();
    let create = Command {
        command_id: id(),
        target: None,
        expected_revision: None,
        payload: json!({"title":"Historical outputs"}),
    };
    let project = global
        .reserve_project(&create, &devdesk_core::InvocationContext::human(None))
        .unwrap();
    let path = SqliteStore::project_path(root.path(), &project).unwrap();
    let e = Engine::new(
        Arc::new(SqliteStore::open_project(root.path(), &project).unwrap()),
        Arc::new(FileArtifacts::new(path.clone())),
        runtime::adapters(),
    )
    .with_tool_executor(Arc::new(integrations::NativeTools::new(&path).unwrap()));
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    e.execute("project.create", create).unwrap();
    runtime::configure_project(&e, &project);
    global
        .publish_project(&e.store.snapshot().unwrap().projects[0])
        .unwrap();
    e.store
        .transact(None, &mut |state| {
            state
                .governance
                .push(global.snapshot()?.configuration(&instance)?.clone());
            Ok(Value::Null)
        })
        .unwrap();
    let (session, _) = runtime::queue_conversation_with(&e, &project, |state, binding| {
        let agent = state.resolve_agent("builtin-agent-sol-high-analyst:6")?;
        binding.agent_configuration = Some(AgentExecutionConfiguration {
            context_protocol: Some(AGENT_CONTEXT_PROTOCOL.into()),
            integration_lock: None,
            agent_instance_id: instance.clone(),
            agent_revision_id: "builtin-agent-sol-high-analyst:6".into(),
            composition_hash: state
                .configuration("builtin-agent-sol-high-analyst:6")?
                .content_hash
                .clone(),
            model_revision_id: agent.model_ref,
            harness_revision_id: agent.harness_ref,
            model_provider: "openai".into(),
            model_parameters: Default::default(),
            tool_revisions: agent.tools,
        });
        Ok(())
    });
    let state = e.store.snapshot().unwrap();
    let defaults = e.configuration_defaults(Some(&project)).unwrap();
    let workflow = state
        .configuration(defaults["workflow_ref"].as_str().unwrap())
        .unwrap();
    let bindings = serde_json::from_value(defaults["role_agent_bindings"].clone()).unwrap();
    let mut plan = e
        .compile_configured_workflow(
            &state,
            workflow,
            &Scope::project(project.clone()),
            Some(&bindings),
        )
        .unwrap();
    let standard = state
        .governance
        .iter()
        .find(|r| r.r#type == "project_configuration")
        .unwrap()
        .data["standard_ref"]
        .as_str()
        .unwrap();
    plan.standard_ref = Some(standard.into());
    plan.definitions.insert(
        standard.into(),
        state.configuration(standard).unwrap().clone(),
    );
    let workflow_id = id();
    let material = id();
    let historical = ConfiguredWorkflowInstance {
        active_plan_revision_id: "later-plan".into(),
        control_version: 1,
        schema_version: 6,
        owner_id: project.clone(),
        workflow_ref: workflow.id.clone(),
        project_id: project.clone(),
        plan,
        role_agent_bindings: bindings,
        inputs: Default::default(),
        user_request: Value::Null,
        human_governance: false,
        steps: vec![],
        status: StepExecutionStatus::Completed,
        paused: false,
        parent_execution_id: None,
        root_instance_id: workflow_id.clone(),
        cancel_requested: false,
    };
    let fact = |kind: &str, key: &str, definition: &str, data: Value| ConfigurationRecord {
        id: key.into(),
        r#type: kind.into(),
        definition_id: definition.into(),
        scope: Scope::project(project.clone()),
        name: kind.into(),
        version: 1,
        supersedes: None,
        content_hash: content_digest(&References::new(), &data),
        refs: References::new(),
        data,
        immutable: true,
        revision: 1,
        created_at: 0,
    };
    e.store.transact(None,&mut |state| {
        for revision in ["original-plan","later-plan"] {
            state.governance.push(fact("workflow_plan_revision",revision,&workflow_id,json!({"owner_id":project,"instance_id":workflow_id,"workflow_ref":historical.workflow_ref,"standard_ref":historical.plan.standard_ref,"plan":historical.plan,"inputs":historical.inputs,"role_agent_bindings":historical.role_agent_bindings})));
        }
        state.governance.push(fact("workflow_instance",&workflow_id,&workflow_id,json!(historical)));
        state.governance.push(fact("configured_session",&id(),&session,json!({"instance_id":workflow_id,"execution_id":"historical-step","plan_revision_id":"original-plan"})));
        state.governance.push(fact("typed_material",&material,&material,json!({"root_instance_id":workflow_id,"project_id":project,"plan_revision_id":"original-plan","value":{"requirements":"Historical accepted output"}})));
        Ok(Value::Null)
    }).unwrap();
    let before = configuration_hash(&e.store.snapshot().unwrap());
    let reader = experience::Reader::new(root.path().into());
    let params = json!({"instance_id":workflow_id,"ids":[material]});
    let result = reader.workflow_outputs(&instance, None, &params).unwrap();
    assert_eq!(result["source"]["project_id"], project);
    assert_eq!(
        result["outputs"][0]["data"]["value"]["requirements"],
        "Historical accepted output"
    );
    assert_eq!(
        result["outputs"][0]["data"]["plan_revision_id"],
        "original-plan"
    );
    assert!(result["source"].get("plan_revision_id").is_none());
    assert!(reader.workflow_outputs(&other, None, &params).is_err());
    assert_eq!(configuration_hash(&e.store.snapshot().unwrap()), before);
}
