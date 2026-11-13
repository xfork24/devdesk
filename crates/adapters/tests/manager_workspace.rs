//! Real native-call admission/receipts with an explicit scripted Harness; no model calls.
use devdesk_adapters::*;
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
#[derive(Default)]
struct Scripted {
    actions: Mutex<VecDeque<(String, Value)>>,
    pending: Mutex<HashMap<Id, Vec<ports::ConversationObservation>>>,
    turns: Mutex<HashMap<Id, Id>>,
    replies: Mutex<Vec<ToolReceipt>>,
}
impl Scripted {
    fn next(&self, s: &Session, turn: &str) -> ports::ConversationObservation {
        if let Some((method, mut arguments)) = self.actions.lock().unwrap().pop_front() {
            if arguments["preview_hash"] == "$previous" {
                arguments["preview_hash"] = self
                    .replies
                    .lock()
                    .unwrap()
                    .last()
                    .unwrap()
                    .structured_content
                    .as_ref()
                    .unwrap()["preview_hash"]
                    .clone();
            }
            if arguments["id"] == "$previous_transfer" {
                arguments["id"] = self
                    .replies
                    .lock()
                    .unwrap()
                    .last()
                    .unwrap()
                    .structured_content
                    .as_ref()
                    .unwrap()["id"]
                    .clone();
            }
            let reply_count = self.replies.lock().unwrap().len();
            ports::ConversationObservation::ToolCall {
                turn: turn.into(),
                request: json!(reply_count + 1),
                thread: s.external_session.clone().unwrap(),
                external_turn: format!("native-{turn}"),
                call: format!("call-{turn}-{reply_count}"),
                tool: format!("devdesk_{}", method.replace('.', "_")),
                arguments,
                worktree: String::new(),
            }
        } else {
            ports::ConversationObservation::Terminal {
                turn: turn.into(),
                outcome: ports::ProviderTerminal::Completed,
                reason: "script finished".into(),
                quiescent: true,
            }
        }
    }
    fn change(&self, change: ConfigurationChangeSet, scope: &str) {
        let mut actions = self.actions.lock().unwrap();
        actions.push_back((
            "configuration.change.preview".into(),
            json!({"changeset":change,"scope":scope}),
        ));
        actions.push_back((
            "configuration.change.apply".into(),
            json!({"changeset":change,"scope":scope,"preview_hash":"$previous"}),
        ));
    }
}
impl ports::Harness for Scripted {
    fn open_session(&self, r: &ports::SessionStart) -> Result<ports::SessionReceipt> {
        Ok(ports::SessionReceipt {
            external_session: format!("native-{}", r.session.id),
            container_id: "explicit-fixture".into(),
            image_digest: r.execution.image_digest.clone(),
            operation_id: r.execution.operation_id.clone(),
        })
    }
    fn dispatch_turn(&self, r: &ports::TurnDispatch) -> Result<()> {
        use ports::ConversationObservation as O;
        self.turns
            .lock()
            .unwrap()
            .insert(r.session.id.clone(), r.turn.id.clone());
        self.pending.lock().unwrap().insert(
            r.session.id.clone(),
            vec![
                O::Admitted {
                    turn: r.turn.id.clone(),
                    message: Some("input".into()),
                    external_turn: Some(format!("native-{}", r.turn.id)),
                },
                O::Consumed {
                    turn: r.turn.id.clone(),
                    message: Some("input".into()),
                },
                self.next(&r.session, &r.turn.id),
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
        self.replies.lock().unwrap().push(r.clone());
        let turn = self.turns.lock().unwrap()[&s.id].clone();
        self.pending
            .lock()
            .unwrap()
            .insert(s.id.clone(), vec![self.next(s, &turn)]);
        Ok(())
    }
    fn stop_session(&self, _: &Session, e: &RuntimeExecution) -> Result<ports::WorkerStop> {
        Ok(ports::WorkerStop {
            operation_id: e.operation_id.clone(),
            writer_revoked: true,
            mode: "fixture".into(),
            requires_successor: true,
        })
    }
    fn cleanup_worker(&self, _: &Session, _: &RuntimeExecution) -> Result<()> {
        Ok(())
    }
}
fn cmd(e: &Engine, method: &str, target: Option<&str>, payload: Value) -> Value {
    let s = e.store.snapshot().unwrap();
    let revision = target.and_then(|id| {
        s.configuration(id)
            .ok()
            .map(|r| r.revision)
            .or_else(|| s.projects.iter().find(|p| p.id == id).map(|p| p.revision))
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
    .unwrap()
}
fn setup(path: &std::path::Path) -> (Engine, Arc<Scripted>, String) {
    let global = Arc::new(GlobalStore::open(path).unwrap());
    let shared = Arc::new(
        Engine::new(
            global.clone(),
            Arc::new(FileArtifacts::new(path.into())),
            runtime::adapters(),
        )
        .with_tool_executor(Arc::new(integrations::NativeTools::new(path).unwrap()))
        .with_configuration_backend(Arc::new(ConfigurationBackend)),
    );
    shared.bootstrap().unwrap();
    shared
        .ensure_default_permissions(&permission_profiles::permission_contract)
        .unwrap();
    let creation = Command {
        command_id: id(),
        target: None,
        expected_revision: None,
        payload: json!({"title":"Manager workspace"}),
    };
    let project = global.reserve_project(&creation).unwrap();
    let store = Arc::new(SqliteStore::open_project(path, &project).unwrap());
    let harness = Arc::new(Scripted::default());
    let mut adapters = runtime::adapters();
    adapters.harness = harness.clone();
    let e = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(store.root().into())),
        adapters,
    )
    .with_tool_executor(Arc::new(integrations::NativeTools::new(path).unwrap()))
    .with_configuration_backend(Arc::new(ConfigurationBackend))
    .with_manager_services(ManagerServices {
        shared,
        inventory: global.clone(),
        gate: Arc::new(Mutex::new(())),
    });
    e.sync_shared_configuration(global.as_ref()).unwrap();
    e.bootstrap().unwrap();
    e.execute("project.create", creation).unwrap();
    global
        .publish_project(&e.store.snapshot().unwrap().projects[0])
        .unwrap();
    (e, harness, project)
}
fn drive(e: &Engine, rounds: usize) {
    for _ in 0..rounds {
        e.drive_controllers().unwrap();
        for intent in e.pending_workflow_intents().unwrap() {
            let method = intent.data["method"].as_str().unwrap();
            let mut c: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
            if method == "session.open" {
                let selected = e.configured_agent_request(&c).unwrap().unwrap();
                let s = e.store.snapshot().unwrap();
                let agent = s
                    .configuration(c.payload["agent"].as_str().unwrap())
                    .unwrap();
                let composition = s.resolve_agent(&agent.id).unwrap();
                let binding = RuntimeBinding {
                    installation: RuntimeInstallation {
                        id: "explicit-manager-runtime".into(),
                        recipe_digest: Some(runtime_preparation::default_codex_recipe_digest()),
                        key: RuntimeKey {
                            provider: HarnessProvider::Codex,
                            version: "fixture".into(),
                            os: "linux".into(),
                            arch: "aarch64".into(),
                        },
                        artifact: RuntimeArtifact::DockerImage {
                            image_id: format!("sha256:{}", "a".repeat(64)),
                        },
                        source: "scripted native port".into(),
                        license: "test".into(),
                    },
                    adapter_version: "fixture/v1".into(),
                    model: "fixture".into(),
                    tool_policy: ToolPolicy::ReadOnly,
                    writable_paths: vec![],
                    read_only_paths: vec![],
                    work_type: WorkType::Orchestration,
                    configuration_digest: "a".repeat(64),
                    credential_ref: None,
                    model_connection: None,
                    model_profile_digest: None,
                    model_parameters: Default::default(),
                    permissions: Some(
                        permission_profiles::compile_permission_profiles(
                            &s,
                            &selected.permissions,
                            Some(ExecutionScope {
                                work_type: WorkType::Orchestration,
                                writable_paths: vec![],
                                read_only_paths: vec![],
                            }),
                        )
                        .unwrap(),
                    ),
                    agent_configuration: Some(AgentExecutionConfiguration {
                        integration_lock: Some(selected.integration_lock),
                        agent_instance_id: "fixture-manager-instance".into(),
                        agent_revision_id: agent.id.clone(),
                        composition_hash: agent.content_hash.clone(),
                        model_revision_id: composition.model_ref,
                        harness_revision_id: composition.harness_ref,
                        model_provider: "openai".into(),
                        model_parameters: Default::default(),
                        tool_revisions: composition.tools,
                    }),
                };
                c.payload["binding"] = json!(binding);
            }
            let result = e.execute(method, c);
            e.observe_workflow_intent(&intent.id, &result).unwrap();
            result.unwrap();
        }
        e.tick_conversations(2).unwrap();
    }
}
fn source(kind: &str, identity: &str, project: &str, data: Value) -> SourceDefinition {
    SourceDefinition {
        r#type: kind.into(),
        identity: identity.into(),
        revision: 1,
        name: identity.into(),
        scope: Scope::project(project),
        refs: References::new(),
        data,
    }
}
fn create(source: SourceDefinition) -> ConfigurationOperation {
    ConfigurationOperation::Create { source }
}
fn apply(e: &Engine, operations: Vec<ConfigurationOperation>) {
    let change = ConfigurationChangeSet { operations };
    let preview = e.preview_configuration_change(&change).unwrap();
    cmd(
        e,
        "configuration.change.apply",
        None,
        json!({"changeset":change,"preview_hash":preview["preview_hash"]}),
    );
}
fn revise(mut source: SourceDefinition) -> ConfigurationOperation {
    let base_ref = source_reference(&source.identity, source.revision).unwrap();
    source.revision += 1;
    ConfigurationOperation::Revise {
        base_ref,
        expected_revision: 1,
        source,
    }
}
#[test]
fn manager_five_configuration_scenarios_have_native_receipts_without_feature_or_task() {
    let dir = tempfile::tempdir().unwrap();
    let (e, h, project) = setup(dir.path());
    let spec = source(
        "spec",
        "local-spec",
        &project,
        json!({"schema_version":3,"body":"Specific checks","checks":[{"key":"shape","name":"Shape","instructions":"Check the exact content"}]}),
    );
    let mut delivery = source(
        "delivery_type",
        "local-delivery",
        &project,
        json!({"format":"json","content_schema":{"type":"object"}}),
    );
    delivery
        .refs
        .insert("spec_refs".into(), json!(["local-spec:1"]));
    let workflow = source(
        "workflow",
        "local-workflow",
        &project,
        json!({"schema_version":6,"stages":[{"key":"work","name":"Work"}],"steps":[{"key":"first","name":"first","stage":"work","executor":{"type":"control"}}]}),
    );
    let skill = source(
        "skill",
        "local-skill",
        &project,
        json!({"body":"Original skill","rules":[]}),
    );
    let mut cap = source("capability", "local-capability", &project, json!({}));
    cap.refs = References::from([
        (
            "model_ref".into(),
            json!("builtin-model-gpt-6-1-sol-high:1"),
        ),
        (
            "harness_ref".into(),
            json!("builtin-harness-codex-managed:1"),
        ),
        ("skills".into(), json!(["local-skill:1"])),
        ("tools".into(), json!(["builtin-tool-codex-native:1"])),
    ]);
    let mut agent = source("agent", "local-agent", &project, json!({}));
    agent.refs = References::from([
        ("role_ref".into(), json!("builtin-role-analyst:1")),
        ("capability_ref".into(), json!("local-capability:1")),
    ]);
    let unused = source(
        "skill",
        "unused-skill",
        &project,
        json!({"body":"Unused","rules":[]}),
    );
    apply(
        &e,
        vec![
            create(agent.clone()),
            create(cap.clone()),
            create(skill.clone()),
            create(delivery.clone()),
            create(spec.clone()),
            create(workflow.clone()),
            create(unused),
        ],
    );
    h.actions
        .lock()
        .unwrap()
        .push_back(("project.overview".into(), json!({})));
    let mut changed = workflow.clone();
    changed.data["steps"].as_array_mut().unwrap().push(json!({"key":"review","name":"review","stage":"work","dependencies":["first"],"executor":{"type":"control"}}));
    h.change(
        ConfigurationChangeSet {
            operations: vec![revise(changed)],
        },
        "project",
    );
    h.change(
        ConfigurationChangeSet {
            operations: vec![ConfigurationOperation::ManageInstance {
                action: "create".into(),
                reference: None,
                expected_revision: None,
                name: Some("New project specialist".into()),
                agent_ref: Some("local-agent:1".into()),
            }],
        },
        "project",
    );
    let mut new_spec = spec.clone();
    new_spec.data["body"] = json!("Revised specification");
    let mut new_delivery = delivery.clone();
    new_delivery
        .refs
        .insert("spec_refs".into(), json!(["local-spec:2"]));
    h.change(
        ConfigurationChangeSet {
            operations: vec![revise(new_delivery), revise(new_spec)],
        },
        "project",
    );
    let mut new_skill = skill.clone();
    new_skill.data["body"] = json!("Revised method");
    let mut new_cap = cap.clone();
    new_cap
        .refs
        .insert("skills".into(), json!(["local-skill:2"]));
    let mut new_agent = agent.clone();
    new_agent
        .refs
        .insert("capability_ref".into(), json!("local-capability:2"));
    h.change(
        ConfigurationChangeSet {
            operations: vec![revise(new_agent), revise(new_cap), revise(new_skill)],
        },
        "project",
    );
    h.change(
        ConfigurationChangeSet {
            operations: vec![ConfigurationOperation::Retire {
                reference: "unused-skill:1".into(),
                expected_revision: 1,
                reason: "No active consumer".into(),
            }],
        },
        "project",
    );
    let manager = cmd(&e, "manager.open", Some(&project), json!({}));
    cmd(
        &e,
        "orchestration.input",
        manager["id"].as_str(),
        json!({"input":"Execute the five explicit future-configuration changes."}),
    );
    drive(&e, 65);
    let replies = h.replies.lock().unwrap();
    assert_eq!(replies.len(), 11);
    assert!(replies.iter().all(|r| r.success), "{:?}", *replies);
    let s = e.store.snapshot().unwrap();
    assert!(s.features.is_empty() && s.tasks.is_empty());
    assert_eq!(
        s.configuration("local-workflow:2").unwrap().data["steps"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        s.configuration("local-delivery:1").unwrap().refs["spec_refs"],
        json!(["local-spec:1"])
    );
    assert_eq!(
        s.configuration("local-delivery:2").unwrap().refs["spec_refs"],
        json!(["local-spec:2"])
    );
    assert_eq!(
        s.configuration("local-capability:1").unwrap().refs["skills"],
        json!(["local-skill:1"])
    );
    assert!(s.configuration("local-agent:2").is_ok());
    assert!(configuration_retired(
        &s,
        s.configuration("unused-skill:1").unwrap()
    ));
    assert!(
        s.governance
            .iter()
            .any(|r| r.r#type == "agent_instance" && r.name == "New project specialist")
    );
    let controller = s.configuration(manager["id"].as_str().unwrap()).unwrap();
    assert_eq!(controller.data["workspace_only"], true);
    assert!(controller.data["root_instance_id"].is_null());
    assert_eq!(
        s.governance
            .iter()
            .filter(|r| r.r#type == "manager_write_authorization")
            .count(),
        5
    );
}
#[test]
fn project_records_are_paginated_chunked_and_cross_project_queries_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (e, _, project) = setup(dir.path());
    let body = "可续读的规范正文\n".repeat(10000);
    apply(
        &e,
        vec![create(source(
            "skill",
            "large-skill",
            &project,
            json!({"body":body,"rules":[]}),
        ))],
    );
    let first = e
        .manager_knowledge("configuration.catalog", &project, &json!({"limit":2}))
        .unwrap();
    assert_eq!(first["records"].as_array().unwrap().len(), 2);
    assert!(first["next_offset"].is_number());
    let next=e.manager_knowledge("configuration.catalog",&project,&json!({"limit":2,"offset":first["next_offset"],"snapshot_digest":first["snapshot_digest"]})).unwrap();
    assert_ne!(first["records"][0]["id"], next["records"][0]["id"]);
    let mut text = String::new();
    let mut offset = json!(0);
    let mut digest = Value::Null;
    loop {
        let mut p =
            json!({"id":"large-skill:1","pointer":"/data/body","offset":offset,"max_bytes":1024});
        if !digest.is_null() {
            p["value_digest"] = digest.clone();
        }
        let page = e
            .manager_knowledge("configuration.read", &project, &p)
            .unwrap();
        text.push_str(page["value_chunk"].as_str().unwrap());
        digest = page["value_digest"].clone();
        offset = page["next_offset"].clone();
        if offset.is_null() {
            break;
        }
    }
    assert_eq!(serde_json::from_str::<String>(&text).unwrap(), body);
    assert!(
        e.manager_knowledge(
            "project.overview",
            &project,
            &json!({"project_id":"another-project"})
        )
        .is_err()
    );
}
#[test]
fn manager_global_scope_requires_frozen_grant_and_stale_native_calls_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (e, h, project) = setup(dir.path());
    let mut shared = source(
        "skill",
        "unauthorized-shared",
        &project,
        json!({"body":"shared source","rules":[]}),
    );
    shared.scope = Scope::global();
    h.actions.lock().unwrap().push_back((
        "configuration.change.preview".into(),
        json!({"scope":"global","changeset":{"operations":[{"type":"create","source":shared}]}}),
    ));
    let manager = cmd(&e, "manager.open", Some(&project), json!({}));
    cmd(
        &e,
        "orchestration.input",
        manager["id"].as_str(),
        json!({"input":"Attempt an explicitly ungranted shared write"}),
    );
    drive(&e, 12);
    assert_eq!(h.replies.lock().unwrap().len(), 1);
    assert!(!h.replies.lock().unwrap()[0].success);
    assert!(
        e.manager_services
            .as_ref()
            .unwrap()
            .shared
            .store
            .snapshot()
            .unwrap()
            .configuration("unauthorized-shared:1")
            .is_err()
    );
    h.actions
        .lock()
        .unwrap()
        .push_back(("project.overview".into(), json!({})));
    cmd(
        &e,
        "orchestration.input",
        manager["id"].as_str(),
        json!({"input":"Read current project"}),
    );
    let mut queued = None;
    for _ in 0..10 {
        drive(&e, 1);
        queued = e
            .store
            .snapshot()
            .unwrap()
            .governance
            .into_iter()
            .find(|r| r.r#type == "controller_tool_call" && r.data["status"] == "prepared");
        if queued.is_some() {
            break;
        }
    }
    let queued = queued.unwrap();
    cmd(
        &e,
        "orchestration.takeover",
        manager["id"].as_str(),
        json!({}),
    );
    e.drive_controllers().unwrap();
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .configuration(&queued.id)
            .unwrap()
            .data["status"],
        "failed"
    );
}
#[test]
fn explicitly_granted_shared_publication_keeps_actor_and_does_not_change_current_policy() {
    let dir = tempfile::tempdir().unwrap();
    let (e, h, project) = setup(dir.path());
    let s = e.store.snapshot().unwrap();
    let old = permission_context(&s).unwrap();
    let mut data = old.data.clone();
    data["control_policy"]["allowed_operations"]
        .as_array_mut()
        .unwrap()
        .push(json!("configuration.global.write"));
    let mut payload =
        json!({"name":"Human-granted shared editor","scope":Scope::project(&project),"data":data});
    for (k, v) in &old.refs {
        payload[k] = v.clone();
    }
    let permission = cmd(&e, "permission_context.create", None, payload);
    let mut shared = source(
        "skill",
        "authorized-shared",
        &project,
        json!({"body":"shared source","rules":[]}),
    );
    shared.scope = Scope::global();
    h.change(
        ConfigurationChangeSet {
            operations: vec![create(shared)],
        },
        "global",
    );
    let manager = cmd(
        &e,
        "manager.open",
        Some(&project),
        json!({"permission_context_ref":permission["id"]}),
    );
    cmd(
        &e,
        "orchestration.input",
        manager["id"].as_str(),
        json!({"input":"Publish this future shared skill"}),
    );
    drive(&e, 18);
    assert_eq!(h.replies.lock().unwrap().len(), 2);
    assert!(h.replies.lock().unwrap().iter().all(|r| r.success));
    let shared = e
        .manager_services
        .as_ref()
        .unwrap()
        .shared
        .store
        .snapshot()
        .unwrap();
    assert!(shared.configuration("authorized-shared:1").is_ok());
    assert!(shared.governance.iter().any(
        |r| r.r#type == "command_context" && r.data["context"]["actor"]["kind"] == "controller"
    ));
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .configuration(manager["id"].as_str().unwrap())
            .unwrap()
            .data["permission_context_ref"],
        permission["id"]
    );
}
#[test]
fn instance_changes_preview_stably_and_keep_old_sessions_and_revisions_readable() {
    let dir = tempfile::tempdir().unwrap();
    let (e, _, project) = setup(dir.path());
    let agent=e.configuration_defaults(Some(&project)).unwrap()["role_agent_bindings"]["builtin-role-analyst:1"].as_str().unwrap().to_owned();
    let change = ConfigurationChangeSet {
        operations: vec![ConfigurationOperation::ManageInstance {
            action: "create".into(),
            reference: None,
            expected_revision: None,
            name: Some("Stable instance".into()),
            agent_ref: Some(agent.clone()),
        }],
    };
    let a = e.preview_configuration_change(&change).unwrap();
    let b = e.preview_configuration_change(&change).unwrap();
    assert_eq!(a["preview_hash"], b["preview_hash"]);
    apply(&e, change.operations);
    let original = e
        .store
        .snapshot()
        .unwrap()
        .governance
        .into_iter()
        .find(|r| r.r#type == "agent_instance" && r.name == "Stable instance")
        .unwrap();
    apply(
        &e,
        vec![ConfigurationOperation::ManageInstance {
            action: "rebind".into(),
            reference: Some(original.id.clone()),
            expected_revision: Some(original.revision),
            name: None,
            agent_ref: Some(agent),
        }],
    );
    let s = e.store.snapshot().unwrap();
    let next = s
        .governance
        .iter()
        .find(|r| {
            r.r#type == "agent_instance"
                && r.definition_id == original.definition_id
                && r.version == 2
        })
        .unwrap();
    assert_eq!(s.configuration(&original.id).unwrap().data, original.data);
    apply(
        &e,
        vec![ConfigurationOperation::ManageInstance {
            action: "remove".into(),
            reference: Some(next.id.clone()),
            expected_revision: Some(next.revision),
            name: None,
            agent_ref: None,
        }],
    );
    let s = e.store.snapshot().unwrap();
    assert!(configuration_retired(
        &s,
        s.configuration(&original.id).unwrap()
    ));
    assert!(s.configuration(&next.id).is_ok());
}
#[test]
fn manager_workspace_edits_future_configuration_while_rust_owns_root() {
    let dir = tempfile::tempdir().unwrap();
    let (e, h, project) = setup(dir.path());
    let workflow = source(
        "workflow",
        "rust-work",
        &project,
        json!({"schema_version":6,"stages":[{"key":"work","name":"Work"}],"steps":[{"key":"one","name":"one","stage":"work","executor":{"type":"control"}}]}),
    );
    apply(&e, vec![create(workflow)]);
    let manager = cmd(&e, "manager.open", Some(&project), json!({}));
    let root = cmd(
        &e,
        "workflow.start",
        Some(&project),
        json!({"workflow_ref":"rust-work:1","role_agent_bindings":{},"orchestration":{"mode":"rust","policy_ref":"builtin-orchestration-policy-balanced:1"}}),
    );
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .configuration(manager["id"].as_str().unwrap())
            .unwrap()
            .data["status"],
        "active"
    );
    h.change(
        ConfigurationChangeSet {
            operations: vec![create(source(
                "skill",
                "future-skill",
                &project,
                json!({"body":"future","rules":[]}),
            ))],
        },
        "project",
    );
    cmd(
        &e,
        "orchestration.input",
        manager["id"].as_str(),
        json!({"input":"Edit future configuration while Rust retains root control"}),
    );
    drive(&e, 18);
    assert!(h.replies.lock().unwrap().iter().all(|r| r.success));
    let s = e.store.snapshot().unwrap();
    assert!(s.configuration("future-skill:1").is_ok());
    assert_eq!(
        e.workflow_driver_query(&json!({"id":root["id"]})).unwrap()["driver"]["data"]["mode"],
        "rust"
    );
    assert!(
        s.configuration(manager["id"].as_str().unwrap())
            .unwrap()
            .data["root_instance_id"]
            .is_null()
    );
}
#[test]
fn authorized_manager_cross_store_adoption_preserves_staged_actor() {
    let dir = tempfile::tempdir().unwrap();
    let (e, h, project) = setup(dir.path());
    let s = e.store.snapshot().unwrap();
    let old = permission_context(&s).unwrap();
    let mut data = old.data.clone();
    data["control_policy"]["allowed_operations"]
        .as_array_mut()
        .unwrap()
        .push(json!("configuration.global.write"));
    let mut payload = json!({"name":"Shared adopter","scope":Scope::project(&project),"data":data});
    for (k, v) in &old.refs {
        payload[k] = v.clone();
    }
    let permission = cmd(&e, "permission_context.create", None, payload);
    let mut spec = source(
        "spec",
        "transferred-spec",
        &project,
        json!({"schema_version":3,"body":"Transferred","checks":[{"key":"shape","name":"Shape","instructions":"Verify structure"}]}),
    );
    spec.scope = Scope::global();
    let mut delivery = source(
        "delivery_type",
        "adopted-delivery",
        &project,
        json!({"format":"json","content_schema":{"type":"object"}}),
    );
    delivery
        .refs
        .insert("spec_refs".into(), json!(["transferred-spec:1"]));
    h.actions.lock().unwrap().push_back(("configuration.transfer.stage".into(),json!({"global_changeset":{"operations":[{"type":"create","source":spec}]},"project_changeset":{"operations":[{"type":"create","source":delivery}]}})));
    h.actions.lock().unwrap().push_back((
        "configuration.transfer.apply".into(),
        json!({"id":"$previous_transfer"}),
    ));
    let manager = cmd(
        &e,
        "manager.open",
        Some(&project),
        json!({"permission_context_ref":permission["id"]}),
    );
    cmd(
        &e,
        "orchestration.input",
        manager["id"].as_str(),
        json!({"input":"Publish and adopt the exact staged configuration"}),
    );
    drive(&e, 18);
    let replies = h.replies.lock().unwrap();
    assert_eq!(replies.len(), 2);
    assert!(replies.iter().all(|r| r.success), "{:?}", *replies);
    let s = e.store.snapshot().unwrap();
    assert_eq!(
        s.configuration("adopted-delivery:1").unwrap().refs["spec_refs"],
        json!(["transferred-spec:1"])
    );
    let shared = e
        .manager_services
        .as_ref()
        .unwrap()
        .shared
        .store
        .snapshot()
        .unwrap();
    assert!(shared.configuration("transferred-spec:1").is_ok());
    let transfer = shared
        .governance
        .iter()
        .find(|r| r.r#type == "configuration_transfer")
        .unwrap();
    assert_eq!(transfer.data["status"], "completed");
    assert_eq!(transfer.data["actor"]["kind"], "controller");
}
#[test]
fn skill_package_and_permission_binding_share_preview_validation_without_execution() {
    let dir = tempfile::tempdir().unwrap();
    let (e, _, project) = setup(dir.path());
    let import = ConfigurationChangeSet {
        operations: vec![ConfigurationOperation::ImportSkill {
            name: "Packaged skill".into(),
            package: SkillPackage {
                body: "Use the declared tool".into(),
                description: "fixture package".into(),
                files: vec![SkillFile {
                    path: "notes.md".into(),
                    content: "inert package asset".into(),
                }],
                requires_tools: vec!["builtin-tool-codex-native:1".into()],
                source: "inline fixture".into(),
                scripts_disabled: true,
            },
            source_revision_id: None,
        }],
    };
    let a = e.preview_configuration_change(&import).unwrap();
    let b = e.preview_configuration_change(&import).unwrap();
    assert_eq!(a["preview_hash"], b["preview_hash"]);
    apply(&e, import.operations);
    let s = e.store.snapshot().unwrap();
    assert!(
        s.governance
            .iter()
            .any(|r| r.r#type == "skill_package" && r.name == "Packaged skill")
    );
    let mut harness = s
        .configuration("builtin-harness-codex-managed:1")
        .unwrap()
        .source();
    harness.identity = "project-harness".into();
    harness.scope = Scope::project(&project);
    harness.refs.insert(
        "runtime_resource_ref".into(),
        json!("builtin-runtime-codex:1"),
    );
    apply(&e, vec![create(harness)]);
    let s = e.store.snapshot().unwrap();
    let target = permission_target(&s, PermissionTargetKind::Harness, "project-harness:1").unwrap();
    let change = ConfigurationChangeSet {
        operations: vec![ConfigurationOperation::PermissionBindings {
            bindings: vec![
                json!({"target":target,"permission_revision_id":"builtin-permission-harness-read-only:1","expected_revision":0}),
            ],
        }],
    };
    let a = e.preview_configuration_change(&change).unwrap();
    assert_eq!(
        a["preview_hash"],
        e.preview_configuration_change(&change).unwrap()["preview_hash"]
    );
    apply(&e, change.operations);
    assert!(active_permission_binding(&e.store.snapshot().unwrap(), &target).is_some());
}
