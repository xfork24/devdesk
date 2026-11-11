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
                .and_then(|r| r.data["role_ref"].as_str())
                .and_then(|id| state.configuration(id).ok())
                .map(|r| r.definition_id.clone())
                .unwrap_or_else(|| format!("builtin-role-{role}"));
            bindings.insert(key, agent.clone());
        }
        payload.as_object_mut().unwrap().remove("agent_refs");
        payload["role_agent_bindings"] = serde_json::to_value(bindings).unwrap();
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
            .any(|r| r.kind == "tool" && r.data["implementation"] == "experience.lookup")
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
        .find(|r| r.kind == "agent" && r.data["role_ref"] == default_role_id("analyst"))
        .unwrap();
    let a = before.resolve_agent(&old.id).unwrap();
    let cap=command(&e,"capability.create",None,json!({"name":"Native analysis","data":{"model_ref":a.model_ref,"harness_ref":a.harness_ref,"tools":a.tools,"skills":a.skills}})).unwrap();
    let agent = command(
        &e,
        "agent.create",
        None,
        json!({"name":"Named analyst","data":{"role_ref":a.role_ref,"capability_ref":cap["id"]}}),
    )
    .unwrap();
    assert_eq!(agent["data"].as_object().unwrap().len(), 2);
    let resolved = e
        .store
        .snapshot()
        .unwrap()
        .resolve_agent(agent["id"].as_str().unwrap())
        .unwrap();
    assert_eq!(resolved.model_ref, a.model_ref);
    assert_eq!(agent["data"]["capability_ref"], cap["id"]);
    assert!(command(&e,"agent.create",None,json!({"name":"Forged","data":{"role_ref":a.role_ref,"capability_ref":cap["id"],"model_ref":"other"}})).is_err());
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
        let data = e
            .store
            .snapshot()
            .unwrap()
            .configuration(id)
            .unwrap()
            .data
            .clone();
        let revision = command(&e, method, Some(id), json!({"data":data})).unwrap();
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
    let result = e.store.transact(None, &mut |s| {
        let mut source = s.configuration(DEFAULT_MODEL_ID)?.clone();
        source.id = "arbitrary-alias".into();
        source.version += 1;
        s.governance.push(source);
        Ok(Value::Null)
    });
    assert!(result.is_err());
}

#[test]
fn factory_workflow_business_contracts_match_the_pre_refactor_baseline() {
    // Baselines from main 082c081: only exact-reference spelling and permission relocation normalized.
    for (source, digest) in [
        (
            include_str!(
                "../../../resources/configuration/workflows/builtin-workflow-feature-sol-high-v5.json"
            ),
            "ed453ae0e68249f612350dec3e5159c0c889ed7ff27c1b002c6be5d4473b0508",
        ),
        (
            include_str!(
                "../../../resources/configuration/workflows/builtin-workflow-task-sol-high-v5.json"
            ),
            "3124b77dffe25d4484585666625623a33d9a5bef7cf89be25105ac5eaa8cc581",
        ),
    ] {
        let value: Value = serde_json::from_str(source).unwrap();
        assert_eq!(configuration_hash(&value["data"]), digest);
    }
    // The 11 explicit grants from the original 42 steps moved without changing any path or tool scope.
    let permission: Value = serde_json::from_str(include_str!(
        "../../../resources/configuration/permissions/bindings/engineering.json"
    ))
    .unwrap();
    assert_eq!(
        configuration_hash(&permission["data"]["step_grants"]),
        "fb1cbd3c102766d4a847f780a8e0521895d1f42ac4cc1ea4ed700a381275138e"
    );
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
    let copied:u64=conn.query_row("SELECT COUNT(*) FROM source_definitions WHERE kind='agent' AND (json_extract(data,'$.model_ref') IS NOT NULL OR json_extract(data,'$.permission_selection') IS NOT NULL)",[],|r|r.get(0)).unwrap();
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
    let e = engine(dir.path());
    let p = command(&e, "project.create", None, json!({"title":"Discussion"})).unwrap();
    let pid = p["id"].as_str().unwrap();
    runtime::configure_project(&e, pid);
    let (template, _) = runtime::queue_conversation(&e, pid);
    let sid = id();
    e.store
        .transact(None, &mut |s| {
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
    let e = engine(dir.path());
    let project = command(
        &e,
        "project.create",
        None,
        json!({"title":"Independent permission selection"}),
    )
    .unwrap();
    let project_id = project["id"].as_str().unwrap();
    let defaults = e.configuration_defaults(Some(project_id)).unwrap();
    let payload = json!({"workflow_ref":defaults["workflow_ref"],"role_agent_bindings":defaults["role_agent_bindings"],"permission_context_ref":"missing:1"});
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
    let mut context = permission_context(&before).unwrap().data.clone();
    context["sandbox_ref"] = json!("missing:1");
    assert!(
        command(
            &e,
            "permission_context.create",
            None,
            json!({"name":"Invalid source","data":context})
        )
        .is_err()
    );
    let context = command(
        &e,
        "permission_context.create",
        None,
        json!({"name":"Project authorization","data":permission_context(&before).unwrap().data}),
    )
    .unwrap();
    let mut selected = payload;
    selected["permission_context_ref"] = context["id"].clone();
    let saved = command(
        &e,
        "project.configuration.update",
        Some(project_id),
        selected,
    )
    .unwrap();
    assert_eq!(saved["data"]["permission_context_ref"], context["id"]);
    assert!(saved["data"].get("specification_snapshot").is_none());
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
    for agent in before.governance.iter().filter(|r| r.kind == "agent") {
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
            json!({"name":"Dangling spec","data":{"format":"markdown","spec_refs":["missing:1"]}})
        )
        .is_err()
    );
    let spec = command(&e, "spec.create", None, json!({"name":"Shared document norm","data":{"schema_version":3,"body":"One source of document requirements","checks":[{"key":"content","name":"Content","instructions":"Verify document content and exact source."}]}})).unwrap();
    let delivery = command(
        &e,
        "delivery_type.create",
        None,
        json!({"name":"Shared document","data":{"format":"markdown","spec_refs":[spec["id"]]}}),
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
    let draft = command(&e, "workflow.create", None, json!({"name":"Document flow","document":{"schema_version":5,"stages":[{"key":"work","name":"Work","default_role_ref":"builtin-role-analyst:1"}],"steps":[
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
fn delivery_catalog_has_content_standards_and_real_diff_ports() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let state = e.store.snapshot().unwrap();
    let deliveries: Vec<_> = state
        .governance
        .iter()
        .filter(|r| r.kind == "delivery_type" && r.id != "fixture-delivery-object:1")
        .collect();
    assert_eq!(deliveries.len(), 21);
    for delivery in &deliveries {
        state
            .validate_delivery_specs(&delivery.decode().unwrap(), &delivery.scope)
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
    for kind in ["feature", "task"] {
        let workflow = state
            .configuration(&format!("builtin-workflow-{kind}-sol-high:5"))
            .unwrap();
        let plan = e
            .compile_configured_workflow(&state, workflow, &Scope::Global, None)
            .unwrap();
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
        .configuration("builtin-workflow-task-sol-high:5")
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
