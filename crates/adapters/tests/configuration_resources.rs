#[path = "../../../tests/support/defaults.rs"]
mod default_fixture;
use default_fixture::*;
#[path = "../../../tests/support/workflows.rs"]
mod workflow_fixture;
use devdesk_adapters::{FileArtifacts, SqliteStore};
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
use std::sync::Arc;
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
fn engine(path: &std::path::Path) -> Engine {
    engine_with_store(path, SqliteStore::open(path).unwrap())
}
fn project_engine(path: &std::path::Path) -> Engine {
    engine_with_store(path, SqliteStore::open_fixture(path).unwrap())
}
fn engine_with_store(path: &std::path::Path, store: SqliteStore) -> Engine {
    let e = Engine::new(
        Arc::new(store),
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
fn command(e: &Engine, method: &str, target: Option<&str>, mut payload: Value) -> Result<Value> {
    if method.starts_with("workflow.") {
        for field in ["data", "document"] {
            if payload.get(field).is_some() {
                payload[field] = workflow_fixture::current(payload[field].clone());
            }
        }
    }
    if method == "project.configuration.update" && payload.get("agent_refs").is_some() {
        let mut bindings = e.configuration_defaults(target).unwrap()["role_agent_bindings"]
            .as_object()
            .unwrap()
            .clone();
        let state = e.store.snapshot().unwrap();
        for (role, agent) in payload["agent_refs"].as_object().unwrap() {
            let key = state
                .configuration(agent.as_str().unwrap())
                .ok()
                .and_then(|r| reference_value(&r.refs, "role_ref").as_str())
                .and_then(|id| state.configuration(id).ok())
                .map(|r| r.definition_id.clone())
                .unwrap_or_else(|| format!("builtin-role-{role}"));
            bindings.insert(key, agent.clone());
        }
        payload.as_object_mut().unwrap().remove("agent_refs");
        payload["role_agent_bindings"] = serde_json::to_value(bindings).unwrap();
    }
    if method == "permission.profile.create" && payload["data"].is_object() {
        let data = payload.as_object_mut().unwrap().remove("data").unwrap();
        payload["target_kind"] = data["target_kind"].clone();
        payload["policy"] = data["policy"].clone();
        payload.as_object_mut().unwrap().remove("scope");
        payload["identity"] = json!(id());
    }
    let revision = target.and_then(|id| {
        e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.revision)
            .or_else(|| {
                e.store
                    .snapshot()
                    .unwrap()
                    .projects
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.revision)
            })
            .or_else(|| {
                e.store
                    .snapshot()
                    .unwrap()
                    .features
                    .iter()
                    .find(|f| f.id == id)
                    .map(|f| f.revision)
            })
    });
    let mut result = e.execute(
        method,
        Command {
            command_id: id(),
            target: target.map(str::to_owned),
            expected_revision: revision,
            payload,
        },
    )?;
    if method == "permission.profile.create" {
        result["id"] = json!(source_reference(
            result["identity"].as_str().unwrap(),
            result["revision"].as_u64().unwrap()
        )?);
    }
    Ok(result)
}
#[test]
fn current_defaults_exclude_experience_tools_and_reject_them_at_creation() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    assert!(
        !e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .any(|r| r.r#type == "tool" && r.data["implementation"] == "experience.lookup")
    );
    assert!(command(&e,"tool.create",None,json!({"name":"Unsupported","data":{"qualified_name":"experience.lookup","implementation":"experience.lookup","input_schema":{},"output_schema":{},"capabilities":[]}})).is_err());
}

#[test]
fn role_crud_has_no_derived_agent_or_skill_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let before = e.store.snapshot().unwrap();
    let role=command(&e,"role.create",None,json!({"name":"Domain expert","data":{"instructions":"# Duty\nClarify inputs","visibility":{"kind":"global"}}})).unwrap();
    let after = e.store.snapshot().unwrap();
    assert_eq!(after.definitions.len(), before.definitions.len());
    assert_eq!(
        after
            .governance
            .iter()
            .filter(|r| r.is_definition())
            .count(),
        before
            .governance
            .iter()
            .filter(|r| r.is_definition())
            .count()
            + 1
    );
    command(&e, "role.delete", role["id"].as_str(), json!({})).unwrap();
    assert_eq!(
        e.store.snapshot().unwrap().definitions.len(),
        before.definitions.len()
    );
}
#[test]
fn agent_stores_only_sources_and_resolves_capability_without_copying() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let before = e.store.snapshot().unwrap();
    let old = before
        .governance
        .iter()
        .find(|r| r.r#type == "agent" && r.refs["role_ref"] == default_role_id("analyst"))
        .unwrap();
    let a = before.resolve_agent(&old.id).unwrap();
    let role_ref = a.role_ref.clone().unwrap();
    let cap=command(&e,"capability.create",None,json!({"name":"Native analysis","scope":"global","model_ref":a.model_ref,"harness_ref":a.harness_ref,"tools":a.tools,"skills":a.skills,"data":{}})).unwrap();
    let agent = command(
        &e,
        "agent.create",
        None,
        json!({"name":"Named analyst","scope":"global","role_ref":role_ref,"capability_ref":cap["id"],"data":{}}),
    )
    .unwrap();
    let resolved = e
        .store
        .snapshot()
        .unwrap()
        .resolve_agent(agent["id"].as_str().unwrap())
        .unwrap();
    assert_eq!(resolved.model_ref, a.model_ref);
    assert_eq!(agent["capability_ref"], cap["id"]);
    assert!(command(&e,"agent.create",None,json!({"name":"Forged","scope":"global","role_ref":role_ref,"capability_ref":cap["id"],"model_ref":"other","data":{}})).is_err());
    assert!(
        command(
            &e,
            "configuration.retire",
            cap["id"].as_str(),
            json!({"reason":"remove"})
        )
        .is_err()
    );
}

#[test]
fn defaults_pin_exact_revisions_without_discarding_published_history() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let initial = e.configuration_defaults(None).unwrap();
    for (method, id) in [
        ("model.revise", initial["model_ref"].as_str().unwrap()),
        ("harness.revise", initial["harness_ref"].as_str().unwrap()),
        (
            "agent.revise",
            initial["role_agent_bindings"]["builtin-role-analyst:1"]
                .as_str()
                .unwrap(),
        ),
    ] {
        let source = e
            .store
            .snapshot()
            .unwrap()
            .configuration(id)
            .unwrap()
            .clone();
        let mut payload = json!({"data":source.data});
        for (key, value) in &source.refs {
            payload[key] = value.clone();
        }
        let revision = command(&e, method, Some(id), payload).unwrap();
        let defaults = e.configuration_defaults(None).unwrap();
        let chosen = match method {
            "model.revise" => &defaults["model_ref"],
            "harness.revise" => &defaults["harness_ref"],
            _ => &defaults["role_agent_bindings"]["builtin-role-analyst:1"],
        };
        assert_eq!(chosen, &json!(id));
        assert_ne!(chosen, &revision["id"]);
        assert!(e.store.snapshot().unwrap().configuration(id).is_ok());
    }
}

#[test]
fn source_store_rejects_arbitrary_revision_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let result = e.store.seed_fixture_state(None, &mut |s| {
        let mut source = s.configuration(DEFAULT_MODEL_ID)?.clone();
        source.id = "arbitrary-alias".into();
        source.version += 1;
        s.governance.push(source);
        Ok(Value::Null)
    });
    assert!(result.is_err());
}

#[test]
fn database_persists_sources_and_rebuilds_execution_views_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let before = e.store.snapshot().unwrap();
    let agent = before.resolve_agent(&default_agent_id("analyst")).unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("engineering.sqlite3")).unwrap();
    let count:u64=conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('definitions','harnesses','sandboxes')",[],|r|r.get(0)).unwrap();
    assert_eq!(count, 0);
    let copied:u64=conn.query_row("SELECT COUNT(*) FROM source_definitions WHERE type='agent' AND (json_extract(refs,'$.model_ref') IS NOT NULL OR json_extract(data,'$.model_ref') IS NOT NULL OR json_extract(data,'$.permission_selection') IS NOT NULL)",[],|r|r.get(0)).unwrap();
    assert_eq!(copied, 0);
    drop(conn);
    drop(e);
    let e = engine(dir.path());
    let after = e.store.snapshot().unwrap();
    assert_eq!(
        after
            .resolve_agent(&default_agent_id("analyst"))
            .unwrap()
            .model_ref,
        agent.model_ref
    );
    assert_eq!(before.definitions.len(), after.definitions.len());
}

#[test]
fn configured_project_discussion_without_standard_captures_its_typed_agent() {
    let dir = tempfile::tempdir().unwrap();
    let e = project_engine(dir.path());
    let p = command(&e, "project.create", None, json!({"title":"Discussion"})).unwrap();
    let pid = p["id"].as_str().unwrap();
    runtime::configure_project(&e, pid);
    let (template, _) = runtime::queue_conversation(&e, pid);
    let sid = id();
    e.store
        .seed_fixture_state(None, &mut |s| {
            let mut session = s
                .sessions
                .iter()
                .find(|r| r.id == template)
                .unwrap()
                .clone();
            session.id = sid.clone();
            session.agent = default_configured_agent_id("analyst");
            s.sessions.push(session);
            Ok(Value::Null)
        })
        .unwrap();
    let turn = e
        .execute(
            "conversation.send",
            Command {
                command_id: id(),
                target: Some(sid.clone()),
                expected_revision: Some(1),
                payload: json!({"client_message_id":id(),"input":"Discuss PRD"}),
            },
        )
        .unwrap();
    let state = e.store.snapshot().unwrap();
    let t = state
        .turns
        .iter()
        .find(|t| Some(t.id.as_str()) == turn["id"].as_str())
        .unwrap();
    let snap = state
        .snapshots
        .iter()
        .find(|snap| Some(&snap.id) == t.input_snapshot.as_ref())
        .unwrap();
    assert_eq!(snap.capture.sessions, vec![sid]);
    assert_eq!(snap.runtime_bindings.len(), 1);
    assert_eq!(
        snap.runtime_bindings[0],
        state
            .sessions
            .iter()
            .find(|s| s.id == snap.capture.sessions[0])
            .unwrap()
            .runtime_binding
            .clone()
            .unwrap()
    );
}

#[test]
fn project_authorization_is_validated_before_selection_and_does_not_modify_agents() {
    let dir = tempfile::tempdir().unwrap();
    let e = project_engine(dir.path());
    let project = command(
        &e,
        "project.create",
        None,
        json!({"title":"Independent permission selection"}),
    )
    .unwrap();
    let project_id = project["id"].as_str().unwrap();
    let defaults = e.configuration_defaults(Some(project_id)).unwrap();
    let payload = json!({"workflow_ref":defaults["workflow_ref"],"role_agent_bindings":defaults["role_agent_bindings"],"harness_permission_ref":"missing:1"});
    assert!(
        command(
            &e,
            "project.configuration.update",
            Some(project_id),
            payload.clone()
        )
        .is_err()
    );
    let before = e.store.snapshot().unwrap();
    let mut context = selected_harness_profile(&before).unwrap().data.clone();
    context["policy"]["filesystem"] =
        json!({"mode":"scoped","rules":[{"path":"../outside","access":"read_write"}]});
    assert!(
        command(
            &e,
            "permission.profile.create",
            None,
            json!({"name":"Invalid source","data":context})
        )
        .is_err()
    );
    let context = command(&e, "permission.profile.create", None, {
        let source = selected_harness_profile(&before).unwrap();
        let mut payload =
            json!({"name":"Project authorization","scope":"global","data":source.data});
        for (key, value) in &source.refs {
            payload[key] = value.clone();
        }
        payload
    })
    .unwrap();
    let mut selected = payload;
    selected["harness_permission_ref"] = context["id"].clone();
    let saved = command(
        &e,
        "project.configuration.update",
        Some(project_id),
        selected,
    )
    .unwrap();
    assert_eq!(saved["data"]["harness_permission_ref"], context["id"]);
    assert!(saved["data"].get("specification_view").is_none());
    let integrations = command(
        &e,
        "integration.project.bind",
        None,
        json!({"project_id":project_id,"bindings":{}}),
    )
    .unwrap();
    assert_ne!(integrations["id"], saved["id"]);
    assert_eq!(integrations["version"], 1);
    assert_eq!(saved["version"], 1);

    let after = e.store.snapshot().unwrap();
    for agent in before.governance.iter().filter(|r| r.r#type == "agent") {
        assert_eq!(after.configuration(&agent.id).unwrap(), agent);
    }
}

#[test]
fn delivery_reuses_the_same_pinned_spec_for_producers_and_consumers() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    assert!(
        command(
            &e,
            "delivery_type.create",
            None,
            json!({"name":"Dangling spec","spec_refs":["missing:1"],"data":{"format":"markdown"}})
        )
        .is_err()
    );
    let spec = command(&e, "spec.create", None, json!({"name":"Shared document norm","data":{"schema_version":3,"body":"One source of document requirements","checks":[{"key":"content","name":"Content","instructions":"Verify document content and exact source."}]}})).unwrap();
    let delivery = command(
        &e,
        "delivery_type.create",
        None,
        json!({"name":"Shared document","scope":"global","spec_refs":[spec["id"]],"data":{"format":"markdown"}}),
    )
    .unwrap();
    assert!(delivery["data"].get("body").is_none());
    command(
        &e,
        "spec.revise",
        spec["id"].as_str(),
        json!({"name":"Shared document norm v2","data":{"schema_version":3,"body":"An explicitly adopted future norm","checks":[]}}),
    )
    .unwrap();
    let draft = command(&e, "workflow.create", None, json!({"name":"Document flow","document":{"schema_version":6,"stages":[{"key":"work","name":"Work","default_role_ref":"builtin-role-analyst:1"}],"steps":[
        {"key":"produce","name":"Produce","stage":"work","executor":{"type":"agent"},"task_instructions":"Write the document","outputs":{"document":{"delivery_ref":delivery["id"]}}},
        {"key":"consume","name":"Consume","stage":"work","executor":{"type":"agent"},"task_instructions":"Read the document","dependencies":["produce"],"inputs":{"document":{"type":"source","source":{"step":"produce","port":"document"}}}}
    ]}})).unwrap();
    let workflow = command(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    for step in ["produce", "consume"] {
        let preview = e.query("query.workflow_task_preview", &json!({"id":workflow["id"],"step":step,"preview_agent_ref":default_agent_id("analyst")}), 4).unwrap();
        assert_eq!(
            preview["task"]["document_specifications"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            preview["task"]["document_specifications"][0]["id"],
            spec["id"]
        );
        assert_eq!(
            preview["task"]["document_specifications"][0]["data"]["body"],
            spec["data"]["body"]
        );
        assert_eq!(preview["task"]["specifications"], json!([]));
    }
}

#[test]
fn detailed_delivery_catalog_has_content_standards_and_real_diff_ports() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let state = e.store.snapshot().unwrap();
    let deliveries: Vec<_> = state
        .governance
        .iter()
        .filter(|r| r.r#type == "delivery_type" && r.id != "fixture-delivery-object:1")
        .collect();
    let manifest: Value = serde_json::from_str(include_str!(
        "../../../resources/configuration/manifest.json"
    ))
    .unwrap();
    assert_eq!(
        deliveries.len(),
        manifest["types"]["delivery_type"].as_array().unwrap().len()
    );
    for delivery in &deliveries {
        state
            .validate_delivery_specs(&delivery.refs, &delivery.decode().unwrap(), &delivery.scope)
            .unwrap();
        assert_ne!(delivery.name, "结构化材料");
    }
    for name in [
        "Review Report",
        "Task Doc",
        "Test Doc",
        "Execution Report",
        "Task Report",
        "Test Report",
        "Diff",
    ] {
        assert!(deliveries.iter().any(|r| r.name == name), "{name}");
    }
    for (method, data) in [
        (
            "delivery_type.create",
            json!({"format":"text","body":"Only a description"}),
        ),
        (
            "delivery_type.create",
            json!({"format":"text","spec_refs":[]}),
        ),
        (
            "delivery_type.create",
            json!({"format":"text","spec_refs":["missing:1"]}),
        ),
    ] {
        assert!(command(&e, method, None, json!({"name":"Invalid", "data":data})).is_err());
    }
    let defaults = detailed_selection(e.configuration_defaults(None).unwrap());
    for (kind, reference) in [
        ("feature", &defaults["workflow_ref"]),
        ("task", &defaults["task_workflow_ref"]),
    ] {
        let workflow = state.configuration(reference.as_str().unwrap()).unwrap();
        let plan = e
            .compile_configured_workflow(&state, workflow, &Scope::global(), None)
            .unwrap();
        let positions = plan
            .document
            .stages
            .iter()
            .enumerate()
            .map(|(index, stage)| (stage.key.as_str(), index))
            .collect::<std::collections::BTreeMap<_, _>>();
        // Stage arrows must agree with every scheduling edge. In particular,
        // candidate preparation must precede the review that consumes it.
        for step in &plan.document.steps {
            for dependency in &step.dependencies {
                let source = plan
                    .document
                    .steps
                    .iter()
                    .find(|s| &s.key == dependency)
                    .unwrap();
                assert!(
                    positions[source.stage.as_str()] <= positions[step.stage.as_str()],
                    "{kind}: {} -> {} crosses stages backwards",
                    source.key,
                    step.key
                );
            }
        }
        assert_eq!(
            plan.document
                .steps
                .iter()
                .filter(|s| s.stage == "candidate_preparation")
                .count(),
            3
        );
        for step in &plan.document.steps {
            for port in step.outputs.values() {
                assert_ne!(port.delivery_ref, "builtin-delivery-object:1");
            }
        }
        for key in [
            "candidate",
            if kind == "task" {
                "implementation_snapshot"
            } else {
                "tests_snapshot"
            },
        ] {
            let step = plan.document.steps.iter().find(|s| s.key == key).unwrap();
            assert_eq!(step.outputs["diff"].delivery_ref, "builtin-delivery-diff:2");
        }
        for key in [
            "candidate_review",
            if kind == "task" {
                "review"
            } else {
                "closure_review"
            },
        ] {
            let step = plan.document.steps.iter().find(|s| s.key == key).unwrap();
            assert!(step.inputs.contains_key("candidate_diff"));
        }
    }
    let mut forged = state
        .configuration("builtin-workflow-task-sol-high:10")
        .unwrap()
        .data
        .clone();
    forged["steps"][0]["executor"] = json!({"type":"agent","role_ref":"builtin-role-developer:1"});
    forged["steps"][0]["task_instructions"] = json!("Pretend to make a real snapshot");
    let draft = command(
        &e,
        "workflow.create",
        None,
        json!({"name":"Forged producer","document":forged}),
    )
    .unwrap();
    assert!(command(&e, "workflow.publish", draft["id"].as_str(), json!({})).is_err());
}

#[test]
fn dsh_stacks_publish_recommended_models_and_native_only_capabilities() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let state = e.store.snapshot().unwrap();
    // Recommended context windows come from the harness model catalog sources.
    for (id, provider, wire, ctx, out) in [
        (
            "builtin-model-minimax-m3-1-flash:1",
            "minimax-cn",
            "MiniMax-M3.1-Flash",
            1000000u64,
            524288u64,
        ),
        (
            "builtin-model-glm-5-3-flash:1",
            "zai-coding-cn",
            "glm-5.3-flash",
            1000000,
            131072,
        ),
    ] {
        let model = state.configuration(id).unwrap();
        assert_eq!(model.data["provider"], json!(provider));
        assert_eq!(model.data["model"], json!(wire));
        assert_eq!(model.data["context_window"], json!(ctx));
        assert_eq!(model.data["max_output_tokens"], json!(out));
    }
    let mut compositions = Vec::new();
    for vendor in ["minimax", "glm"] {
        for role in [
            "analyst",
            "architect",
            "developer",
            "tester",
            "reviewer",
            "project-manager",
        ] {
            let composition = state
                .resolve_agent(&format!(
                    "builtin-agent-{vendor}-{role}:{}",
                    if role == "project-manager" { 5 } else { 2 }
                ))
                .unwrap();
            assert_eq!(
                composition.harness_ref,
                "builtin-harness-dsh-multi-provider:1"
            );
            assert_eq!(
                composition.tools,
                vec!["builtin-tool-dsh-native:1".to_string()],
                "{vendor}/{role} declares native business tools alongside the common context protocol"
            );
            compositions.push((vendor, role, composition));
        }
        let model_id = if vendor == "minimax" {
            "builtin-model-minimax-m3-1-flash:1"
        } else {
            "builtin-model-glm-5-3-flash:1"
        };
        let default = state
            .configuration(&format!("builtin-capability-{vendor}-dsh:2"))
            .unwrap();
        assert_eq!(default.refs["model_ref"], json!(model_id));
    }
    // Role skills mirror the codex counterparts, headed by the shared engineering context.
    for (vendor, role, composition) in &compositions {
        assert!(
            composition
                .skills
                .contains(&"builtin-skill-engineering-context:3".to_string()),
            "{vendor}/{role}"
        );
    }
    let (_, _, analyst) = compositions
        .iter()
        .find(|(v, r, _)| *v == "minimax" && *r == "analyst")
        .unwrap();
    assert!(analyst.skills.contains(&"builtin-skill-prd:2".to_string()));
    let (_, _, pm) = compositions
        .iter()
        .find(|(v, r, _)| *v == "glm" && *r == "project-manager")
        .unwrap();
    assert!(
        pm.skills
            .contains(&"builtin-skill-workflow-orchestration:5".to_string())
    );
}

#[test]
fn only_project_manager_roles_can_be_selected_for_project_orchestration() {
    let dir = tempfile::tempdir().unwrap();
    let e = project_engine(dir.path());
    let project = command(
        &e,
        "project.create",
        None,
        json!({"title":"Manager role boundary"}),
    )
    .unwrap();
    let project_id = project["id"].as_str().unwrap();
    let defaults = e.configuration_defaults(Some(project_id)).unwrap();
    let error = command(
        &e,
        "project.configuration.update",
        Some(project_id),
        json!({
            "workflow_ref": defaults["workflow_ref"],
            "role_agent_bindings": defaults["role_agent_bindings"],
            "orchestrator_role_ref": "builtin-role-analyst:1",
        }),
    )
    .unwrap_err();
    assert!(error.message.contains("只有项目经理角色"), "{error}");
    let saved = command(
        &e,
        "project.configuration.update",
        Some(project_id),
        json!({
            "workflow_ref": defaults["workflow_ref"],
            "role_agent_bindings": defaults["role_agent_bindings"],
            "orchestrator_role_ref": defaults["orchestrator_role_ref"],
        }),
    )
    .unwrap();
    assert_eq!(
        saved["data"]["orchestrator_role_ref"],
        defaults["orchestrator_role_ref"]
    );
    let state = e.store.snapshot().unwrap();
    assert!(state.features.is_empty() && state.tasks.is_empty() && state.sessions.is_empty());
    assert!(!state.governance.iter().any(|r| matches!(
        r.r#type.as_str(),
        "workflow_instance" | "workflow_controller"
    )));
}

#[test]
fn deferred_feature_creation_freezes_inputs_and_configuration_until_explicit_start() {
    let dir = tempfile::tempdir().unwrap();
    let e = project_engine(dir.path());
    let project = command(
        &e,
        "project.create",
        None,
        json!({"title":"Create then start"}),
    )
    .unwrap();
    let project_id = project["id"].as_str().unwrap();
    let defaults = e.configuration_defaults(Some(project_id)).unwrap();
    let spec = command(&e, "spec.create", None, json!({
        "name":"Start input content", "scope":format!("project:{project_id}"),
        "data":{"schema_version":3,"body":"提供明确的业务背景。","checks":[{"key":"brief","name":"业务背景","instructions":"说明工作目标。"}]}
    })).unwrap();
    let delivery = command(
        &e,
        "delivery_type.create",
        None,
        json!({
            "name":"Start input", "scope":format!("project:{project_id}"), "spec_refs":[spec["id"]],
            "data":{"format":"json","content_schema":{"type":"string"}}
        }),
    )
    .unwrap();
    let draft = command(
        &e,
        "workflow.create",
        None,
        json!({
            "name":"Explicit start", "document":{"schema_version":6,
                "inputs":{"brief":{"delivery_ref":delivery["id"],"required":true}},
                "stages":[{"key":"work","name":"Work"}],
                "steps":[{"key":"done","name":"Done","stage":"work","executor":{"type":"control"}}]
            }
        }),
    )
    .unwrap();
    let workflow = command(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let selected = command(&e, "project.configuration.update", Some(project_id), json!({
        "workflow_ref":workflow["id"], "role_agent_bindings":defaults["role_agent_bindings"],
        "orchestrator_role_ref":defaults["orchestrator_role_ref"], "orchestration":{"mode":"agent"}
    })).unwrap();
    let mut payload = json!({
        "project_id":project_id, "title":"Draft Feature", "initial_requirement":"保存需求后显式启动。",
        "configuration_revision_id":selected["id"], "start_workflow":false
    });
    let error = command(&e, "feature.create", None, payload.clone()).unwrap_err();
    assert!(error.message.contains("缺少流程输入 brief"), "{error}");
    payload["workflow_inputs"] = json!({"brief":"持久化的流程输入"});
    let feature = command(&e, "feature.create", None, payload).unwrap();
    let feature_id = feature["id"].as_str().unwrap();
    e.drive_controllers().unwrap();
    let state = e.store.snapshot().unwrap();
    assert_eq!(state.feature(feature_id).unwrap().status, Status::Draft);
    assert!(state.sessions.is_empty() && state.turns.is_empty());
    assert!(!state.governance.iter().any(|r| matches!(
        r.r#type.as_str(),
        "workflow_instance" | "workflow_controller" | "workflow_driver"
    )));
    let frozen = state
        .governance
        .iter()
        .find(|r| r.r#type == "work_configuration" && r.definition_id == feature_id)
        .unwrap()
        .clone();
    assert_eq!(frozen.data["source_configuration_ref"], selected["id"]);
    drop(e);
    let e = project_engine(dir.path());
    command(&e, "project.configuration.update", Some(project_id), json!({
        "workflow_ref":workflow["id"], "role_agent_bindings":defaults["role_agent_bindings"],
        "orchestrator_role_ref":defaults["orchestrator_role_ref"], "orchestration":{"mode":"manual"}
    })).unwrap();
    let started = command(&e, "workflow.start", Some(feature_id), json!({})).unwrap();
    let state = e.store.snapshot().unwrap();
    let root = state
        .configuration(started["id"].as_str().unwrap())
        .unwrap();
    assert_eq!(state.feature(feature_id).unwrap().status, Status::Running);
    assert_eq!(root.data["inputs"], json!({"brief":"持久化的流程输入"}));
    assert_eq!(
        root.data["user_request"]["initial_requirement"],
        "保存需求后显式启动。"
    );
    assert_eq!(
        root.data["plan"]["standard_ref"],
        frozen.data["standard_ref"]
    );
    let driver = state
        .governance
        .iter()
        .find(|r| r.r#type == "workflow_driver" && r.definition_id == root.id)
        .unwrap();
    assert_eq!(driver.data["mode"], "agent");
    assert!(command(&e, "workflow.start", Some(feature_id), json!({})).is_err());
}

#[test]
fn dsh_agents_cannot_be_bound_as_the_orchestrator() {
    let dir = tempfile::tempdir().unwrap();
    let e = project_engine(dir.path());
    let project = command(
        &e,
        "project.create",
        None,
        json!({"title":"Codex-only orchestration"}),
    )
    .unwrap();
    let project_id = project["id"].as_str().unwrap();
    let defaults = e.configuration_defaults(Some(project_id)).unwrap();
    // Binding the PM role to a dsh agent while designating it the orchestrator is rejected.
    let mut bindings = defaults["role_agent_bindings"].clone();
    bindings["builtin-role-project-manager:1"] = json!("builtin-agent-minimax-project-manager:5");
    let error = command(
        &e,
        "project.configuration.update",
        Some(project_id),
        json!({
            "workflow_ref": defaults["workflow_ref"],
            "role_agent_bindings": bindings,
            "orchestrator_role_ref": "builtin-role-project-manager:1",
        }),
    )
    .unwrap_err();
    assert!(
        error.message.contains("编排"),
        "expected orchestration harness rejection, got: {error}"
    );
    // The factory workflows grant platform tools (workflow.outputs.read, codex-native) on
    // nearly every agent step, so rebinding a worker role to a dsh agent cannot compile the
    // factory selection either — dsh agents serve standalone conversations until the tool
    // bridge (dsh-tool-bridge plan) lands.
    let mut workers = defaults["role_agent_bindings"].clone();
    workers["builtin-role-analyst:1"] = json!("builtin-agent-glm-analyst:2");
    let boundary = command(
        &e,
        "project.configuration.update",
        Some(project_id),
        json!({
            "workflow_ref": defaults["workflow_ref"],
            "role_agent_bindings": workers,
        }),
    )
    .unwrap_err();
    assert!(
        boundary.message.contains("不属于 Agent 能力集合"),
        "expected the factory tool-grant boundary, got: {boundary}"
    );
}

#[test]
fn dsh_workers_compile_custom_workflows_without_platform_tool_grants() {
    let dir = tempfile::tempdir().unwrap();
    let e = project_engine(dir.path());
    let before = e.store.snapshot().unwrap();
    // A custom permission context without the factory's codex-only step tool grants.
    let mut source = selected_harness_profile(&before).unwrap().clone();
    source.data["step_grants"] = json!([]);
    let context = command(&e, "permission.profile.create", None, {
        let mut payload = json!({"name":"Bridge-free context","scope":"global","data":source.data});
        for (key, value) in &source.refs {
            payload[key] = value.clone();
        }
        payload
    })
    .unwrap();
    let draft = command(
        &e,
        "workflow.create",
        None,
        json!({"name":"Dsh worker flow","document":{"schema_version":6,"stages":[{"key":"work","name":"Work","default_role_ref":"builtin-role-analyst:1"}],"steps":[
            {"key":"draft","name":"Draft","stage":"work","executor":{"type":"agent"},"task_instructions":"Write the document","outputs":{"document":{"delivery_ref":"builtin-delivery-review:2"}}}
        ]}}),
    )
    .unwrap();
    let workflow = command(&e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap();
    let project = command(&e, "project.create", None, json!({"title":"Dsh workers"})).unwrap();
    let project_id = project["id"].as_str().unwrap();
    // Binding the analyst role to a dsh agent compiles a workflow whose steps carry no
    // platform-tool grants; the factory flows are the codex-bound ones, not the mechanism.
    let mut bindings =
        e.configuration_defaults(Some(project_id)).unwrap()["role_agent_bindings"].clone();
    bindings["builtin-role-analyst:1"] = json!("builtin-agent-glm-analyst:2");
    let saved = command(
        &e,
        "project.configuration.update",
        Some(project_id),
        json!({
            "workflow_ref": workflow["id"],
            "role_agent_bindings": bindings,
            "harness_permission_ref": context["id"],
        }),
    )
    .unwrap();
    assert_eq!(
        saved["data"]["role_agent_bindings"]["builtin-role-analyst:1"],
        json!("builtin-agent-glm-analyst:2")
    );
}

#[test]
fn project_usage_follows_flattened_agent_and_capability_references() {
    let root = tempfile::tempdir().unwrap();
    let engine = project_engine(root.path());
    let project = command(
        &engine,
        "project.create",
        None,
        json!({"title":"Transitive configuration usage"}),
    )
    .unwrap();
    let defaults = engine
        .configuration_defaults(project["id"].as_str())
        .unwrap();
    let selection = command(
        &engine,
        "project.configuration.update",
        project["id"].as_str(),
        json!({
            "workflow_ref":defaults["workflow_ref"],
            "role_agent_bindings":defaults["role_agent_bindings"],
            "orchestrator_role_ref":defaults["orchestrator_role_ref"],
            "harness_permission_ref":defaults["harness_permission_ref"],
        }),
    )
    .unwrap();
    let state = engine.store.snapshot().unwrap();
    for reference in [DEFAULT_MODEL_ID, DEFAULT_HARNESS_ID, DEFAULT_NATIVE_TOOL_ID] {
        let uses =
            project_configuration_references(&state, state.configuration(reference).unwrap());
        assert!(
            uses.iter().any(|row| row["id"] == selection["id"]),
            "missed transitive use of {reference}"
        );
    }
}

#[test]
fn default_document_structure_and_review_locations_are_real_configuration_rules() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let state = e.store.snapshot().unwrap();
    let defaults = e.configuration_defaults(None).unwrap();
    let workflow: WorkflowDefinition = state
        .configuration(defaults["workflow_ref"].as_str().unwrap())
        .unwrap()
        .decode()
        .unwrap();
    for key in ["requirements", "design"] {
        let step = workflow.steps.iter().find(|s| s.key == key).unwrap();
        let reference = &step.outputs["result"].delivery_ref;
        let delivery = state.configuration(reference).unwrap();
        let spec: ConfiguredSpecification = state
            .configuration(
                reference_list(&delivery.refs, "spec_refs")
                    .unwrap()
                    .first()
                    .unwrap(),
            )
            .unwrap()
            .decode()
            .unwrap();
        let headings = spec
            .checks
            .iter()
            .find_map(|c| match &c.assertion {
                Some(MachineAssertion::RequiredSections { headings }) => Some(headings),
                _ => None,
            })
            .unwrap();
        let valid = headings
            .iter()
            .map(|h| format!("## {h}\nConcrete scope and evidence\n"))
            .collect::<String>();
        let report = e
            .specification_lint(&json!({"delivery_ref":reference,"value":valid}))
            .unwrap();
        assert_eq!(report["structure_valid"], true);
        assert_eq!(report["semantic_quality"], "requires_judgment");
        assert!(
            report["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|check| check["status"] == "requires_judgment")
        );
        let missing = valid.replace(
            &format!("## {}", headings[0]),
            "plain text cannot replace a heading",
        );
        assert_eq!(
            e.specification_lint(&json!({"delivery_ref":reference,"value":missing}))
                .unwrap()["structure_valid"],
            false
        );
    }
    for workflow_ref in ["workflow_ref", "task_workflow_ref"] {
        let doc: WorkflowDefinition = state
            .configuration(defaults[workflow_ref].as_str().unwrap())
            .unwrap()
            .decode()
            .unwrap();
        for step in doc
            .steps
            .iter()
            .filter(|s| s.outputs.values().any(|p| p.content_checks))
        {
            for reference in &step.spec_refs {
                let spec: ConfiguredSpecification =
                    state.configuration(reference).unwrap().decode().unwrap();
                assert!(!spec.checks.is_empty());
                assert!(
                    spec.checks
                        .iter()
                        .all(|c| c.evidence_required || c.assertion.is_some()),
                    "{}",
                    step.key
                );
            }
        }
    }
    assert!(
        !state
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_confirmation" || r.r#type == "workflow_output")
    );
}
