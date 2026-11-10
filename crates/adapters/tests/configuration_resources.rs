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
    );
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    e
}
fn command(e: &Engine, method: &str, target: Option<&str>, mut payload: Value) -> Result<Value> {
    if method == "agent.create" && payload["data"].get("permission_selection").is_none() {
        payload["data"]["permission_selection"] =
            serde_json::to_value(default_permission_selection()).unwrap();
    }
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
    let role=command(&e,"role.create",None,json!({"name":"Domain expert","data":{"execution_role":"analyst","instructions":"# Duty\nClarify inputs","visibility":{"kind":"global"}}})).unwrap();
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
fn capability_is_independent_and_agent_components_are_frozen_from_it() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let before = e.store.snapshot().unwrap();
    let old = before
        .governance
        .iter()
        .find(|r| r.kind == "agent" && r.data["role"] == "analyst")
        .unwrap();
    let a: AgentComposition = old.decode().unwrap();
    let cap=command(&e,"capability.create",None,json!({"name":"Native analysis","data":{"model_ref":a.model_ref,"harness_ref":a.harness_ref,"tools":a.tools,"skills":a.skills}})).unwrap();
    let agent = command(
        &e,
        "agent.create",
        None,
        json!({"name":"Named analyst","data":{"role_ref":a.role_ref,"capability_ref":cap["id"]}}),
    )
    .unwrap();
    assert_eq!(agent["data"]["model_ref"], old.data["model_ref"]);
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
fn project_visibility_and_baseline_empty_matches_fail_closed() {
    let visibility = DefinitionVisibility::Projects {
        project_ids: vec!["p1".into(), "p2".into()],
    };
    visibility.validate().unwrap();
    assert!(visibility.permits(Some("p1")));
    assert!(!visibility.permits(Some("other")));
    assert!(!visibility.permits(None));
    let baseline = StandardDefinition {
        body: "Production".into(),
        baseline_rules: vec![BaselineRule {
            selector: ActionSelector {
                kind: "delivery.review".into(),
                phase: None,
                delivery_kind: None,
            },
            required_spec_refs: vec!["review-spec-v1".into()],
            min_matches: 1,
        }],
    };
    let result = check_baseline("standard-v1", &baseline, &[]).unwrap();
    assert!(!result.valid);
    assert_eq!(result.errors.len(), 1);
}
#[test]
fn v2_subworkflow_expands_actual_nodes_and_rejects_missing_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let doc = json!({"schema_version":2,"process_kind":"subworkflow","stages":[{"key":"work","name":"Work"}],"steps":[{"key":"finish","name":"Finish","kind":"join","stage":"work"}]});
    let child = command(
        &e,
        "workflow.create",
        None,
        json!({"name":"Child","document":doc}),
    )
    .unwrap();
    let child = command(&e, "workflow.publish", child["id"].as_str(), json!({})).unwrap();
    let parent=command(&e,"workflow.create",None,json!({"name":"Parent","document":{"schema_version":2,"process_kind":"subworkflow","stages":[{"key":"work","name":"Work"}],"steps":[{"key":"call","name":"Call child","kind":"workflow.call","stage":"work","subworkflow_ref":child["id"]}]}})).unwrap();
    let parent = command(&e, "workflow.publish", parent["id"].as_str(), json!({})).unwrap();
    let requirements = e
        .query(
            "query.workflow_requirements",
            &json!({"id":parent["id"]}),
            4,
        )
        .unwrap();
    assert_eq!(requirements["plan"]["steps"].as_array().unwrap().len(), 3);
    assert_eq!(requirements["plan"]["steps"][2]["kind"], "workflow.call");
    let input_child = publish(
        &e,
        "Child with input",
        json!({"schema_version":2,"process_kind":"subworkflow","stages":[{"key":"work","name":"Work"}],"input_ports":["context"],"steps":[{"key":"consume","name":"Consume","kind":"join","stage":"work","inputs":{"context":{"step":"$input","port":"context"}}}]}),
    );
    let invalid=command(&e,"workflow.create",None,json!({"name":"Missing input","document":{"schema_version":2,"process_kind":"subworkflow","stages":[{"key":"work","name":"Work"}],"steps":[{"key":"call","name":"Call","kind":"workflow.call","stage":"work","subworkflow_ref":input_child["id"]}]}})).unwrap();
    assert!(command(&e, "workflow.publish", invalid["id"].as_str(), json!({})).is_err());
}

fn reference_configuration(e: &Engine) -> (String, Value) {
    let s = e.store.snapshot().unwrap();
    let wf = s
        .governance
        .iter()
        .find(|r| {
            r.kind == "workflow"
                && r.data["schema_version"] == 3
                && r.data["process_kind"] == "feature"
        })
        .unwrap()
        .id
        .clone();
    let roles: serde_json::Map<String, Value> = s
        .governance
        .iter()
        .filter(|r| r.kind == "agent")
        .map(|a| {
            (
                s.configuration(a.data["role_ref"].as_str().unwrap())
                    .unwrap()
                    .definition_id
                    .clone(),
                json!(a.id),
            )
        })
        .collect();
    (wf, json!(roles))
}
fn publish(e: &Engine, name: &str, doc: Value) -> Value {
    let draft = command(
        e,
        "workflow.create",
        None,
        json!({"name":name,"document":doc}),
    )
    .unwrap();
    command(e, "workflow.publish", draft["id"].as_str(), json!({})).unwrap()
}
#[test]
fn explicit_project_bindings_freeze_agent_capability_and_spec_closure_without_standard() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let p = command(&e, "project.create", None, json!({"title":"Explicit"})).unwrap();
    let pid = p["id"].as_str().unwrap();
    assert!(
        command(
            &e,
            "feature.create",
            None,
            json!({"project_id":pid,"title":"Pending"})
        )
        .is_err()
    );
    runtime::configure_project(&e, pid);
    let first = command(
        &e,
        "feature.create",
        None,
        json!({"project_id":pid,"title":"Frozen"}),
    )
    .unwrap();
    let s = e.store.snapshot().unwrap();
    let lock = s
        .feature_configuration(first["id"].as_str().unwrap())
        .unwrap();
    assert_eq!(lock.compiler_version, 3);
    let mut retired = json!(lock);
    retired["compiler_version"] = json!(2);
    assert!(serde_json::from_value::<FeatureConfiguration>(retired).is_err());
    assert!(lock.standard_ref.is_empty());
    assert!(lock.baseline_check.is_none());
    assert!(!lock.specification_snapshot.is_empty());
    assert!(
        lock.definition_hashes
            .keys()
            .any(|id| id.contains("capability-"))
    );
    for role in ["analyst", "architect", "developer", "tester", "reviewer"] {
        let identity = s
            .configuration(&default_role_id(role))
            .unwrap()
            .definition_id
            .clone();
        assert_eq!(
            lock.role_agent_bindings[&identity],
            default_configured_agent_id(role)
        );
    }
    let before = json!(lock);
    runtime::configure_project(&e, pid);
    assert_eq!(
        json!(
            e.store
                .snapshot()
                .unwrap()
                .feature_configuration(first["id"].as_str().unwrap())
                .unwrap()
        ),
        before
    );
}
#[test]
fn same_execution_class_cannot_substitute_an_unrelated_role_identity() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let (wf, mut roles) = reference_configuration(&e);
    let role=command(&e,"role.create",None,json!({"name":"Other Analyst","data":{"execution_role":"analyst","instructions":"Other duty"}})).unwrap();
    let s = e.store.snapshot().unwrap();
    let cap = s
        .configuration(&default_configured_agent_id("analyst"))
        .unwrap()
        .data["capability_ref"]
        .clone();
    let a = command(
        &e,
        "agent.create",
        None,
        json!({"name":"Other Agent","data":{"role_ref":role["id"],"capability_ref":cap}}),
    )
    .unwrap();
    let identity = s
        .configuration(&default_role_id("analyst"))
        .unwrap()
        .definition_id
        .clone();
    roles[identity] = a["id"].clone();
    let p = command(&e, "project.create", None, json!({"title":"Wrong role"})).unwrap();
    assert!(
        command(
            &e,
            "project.configuration.update",
            p["id"].as_str(),
            json!({"workflow_ref":wf,"role_agent_bindings":roles})
        )
        .unwrap_err()
        .message
        .contains("Role 身份")
    );
}
#[test]
fn baseline_checks_placement_and_audit_detects_context_changes_with_same_specs() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let (wf, roles) = reference_configuration(&e);
    let audit = e
        .query("query.workflow_requirements", &json!({"id":wf}), 4)
        .unwrap();
    let bindings = audit["specification_snapshot"].as_array().unwrap();
    let review = bindings
        .iter()
        .find(|b| b["action"] == "delivery.review")
        .unwrap();
    let spec = review["spec_refs"][0].clone();
    let baseline=command(&e,"standard.create",None,json!({"name":"Review baseline","data":{"body":"Review requirements","baseline_rules":[{"selector":{"kind":"delivery.review"},"required_spec_refs":[spec],"min_matches":1}]}})).unwrap();
    let checked = e
        .query(
            "query.workflow_requirements",
            &json!({"id":wf,"standard_ref":baseline["id"]}),
            4,
        )
        .unwrap();
    assert_eq!(checked["baseline_check"]["valid"], false); // other phases use different exact Specs
    let p = command(&e, "project.create", None, json!({"title":"Baseline"})).unwrap();
    assert!(
        command(
            &e,
            "project.configuration.update",
            p["id"].as_str(),
            json!({"workflow_ref":wf,"role_agent_bindings":roles,"standard_ref":baseline["id"]})
        )
        .is_err()
    );
    let mut doc = e
        .store
        .snapshot()
        .unwrap()
        .configuration(&wf)
        .unwrap()
        .data
        .clone();
    let step = doc["steps"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["kind"] == "delivery.review")
        .unwrap();
    step["parameters"]["audit_context"] = json!("changed output contract");
    let preview = e
        .query(
            "query.specification_audit",
            &json!({"document":doc,"previous_snapshot":bindings}),
            4,
        )
        .unwrap();
    assert!(
        preview["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["kind"] == "changed")
    );
}
#[test]
fn reusable_subworkflow_waits_for_confirmation_and_exports_actual_snapshot_after_resume() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let child = publish(
        &e,
        "Reusable snapshot",
        json!({"schema_version":2,"process_kind":"subworkflow","stages":[{"key":"context","name":"Context"}],"steps":[{"key":"capture","name":"Capture","kind":"snapshot.capture","stage":"context"}],"output_ports":{"snapshot":{"step":"capture","port":"snapshot"}}}),
    );
    let (reference, roles) = reference_configuration(&e);
    let mut doc = e
        .store
        .snapshot()
        .unwrap()
        .configuration(&reference)
        .unwrap()
        .data
        .clone();
    let steps = doc["steps"].as_array_mut().unwrap();
    let first = steps
        .iter_mut()
        .find(|s| s["key"] == "initial_snapshot")
        .unwrap();
    first["kind"] = json!("workflow.call");
    first["subworkflow_ref"] = child["id"].clone();
    first["advance_mode"] = json!("human_confirmation");
    let parent = publish(&e, "With child", doc);
    let p = command(&e, "project.create", None, json!({"title":"Calls"})).unwrap();
    command(
        &e,
        "project.configuration.update",
        p["id"].as_str(),
        json!({"workflow_ref":parent["id"],"role_agent_bindings":roles}),
    )
    .unwrap();
    let f = command(
        &e,
        "feature.create",
        None,
        json!({"project_id":p["id"],"title":"Call"}),
    )
    .unwrap();
    let fid = f["id"].as_str().unwrap();
    e.evaluate_workflows().unwrap();
    let s = e.store.snapshot().unwrap();
    let r: WorkflowRuntime = s
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.definition_id == fid)
        .unwrap()
        .decode()
        .unwrap();
    let gate = r
        .steps
        .iter()
        .find(|s| s.status == "waiting_confirmation")
        .unwrap();
    assert!(!s.snapshots.iter().any(|s| s.kind == "workflow"));
    assert!(
        r.steps
            .iter()
            .any(|s| s.key.contains("capture") && s.status == "pending")
    );
    e.execute(
        "workflow.step.confirm",
        Command {
            command_id: id(),
            target: Some(gate.id.clone()),
            expected_revision: Some(gate.revision),
            payload: json!({"input_hash":gate.input_hash}),
        },
    )
    .unwrap();
    e.evaluate_workflows().unwrap();
    for intent in e.pending_workflow_intents().unwrap() {
        if intent.data["method"] == "snapshot.save" {
            let cmd: Command = serde_json::from_value(intent.data["command"].clone()).unwrap();
            let result = e.execute("snapshot.save", cmd);
            e.observe_workflow_intent(&intent.id, &result).unwrap();
            result.unwrap();
        }
    }
    e.evaluate_workflows().unwrap();
    let s = e.store.snapshot().unwrap();
    let r: WorkflowRuntime = s
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.definition_id == fid)
        .unwrap()
        .decode()
        .unwrap();
    let call = r
        .steps
        .iter()
        .find(|s| s.key == "initial_snapshot")
        .unwrap();
    assert_eq!(call.status, "completed");
    assert!(
        s.snapshots
            .iter()
            .any(|s| Some(&s.id) == call.outputs.get("snapshot"))
    );
}
#[test]
fn defaults_follow_current_identity_revisions_without_discarding_published_history() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let initial = e.configuration_defaults(None).unwrap();
    for (method, id) in [
        ("model.revise", initial["model_ref"].as_str().unwrap()),
        ("harness.revise", initial["harness_ref"].as_str().unwrap()),
        (
            "agent.revise",
            initial["role_agent_bindings"]["builtin-role-analyst"]
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
            _ => &defaults["role_agent_bindings"]["builtin-role-analyst"],
        };
        assert_eq!(chosen, &revision["id"]);
        assert!(e.store.snapshot().unwrap().configuration(id).is_ok());
    }
}

#[test]
fn selected_standard_is_preserved_in_the_frozen_feature_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let project = command(
        &e,
        "project.create",
        None,
        json!({"title":"Explicit baseline"}),
    )
    .unwrap();
    let defaults = e.configuration_defaults(None).unwrap();
    let pid = project["id"].as_str().unwrap();
    command(&e, "project.configuration.update", Some(pid), json!({"workflow_ref":defaults["workflow_ref"],"role_agent_bindings":defaults["role_agent_bindings"],"standard_ref":DEFAULT_STANDARD_ID})).unwrap();
    let feature = command(
        &e,
        "feature.create",
        None,
        json!({"project_id":pid,"title":"Frozen baseline"}),
    )
    .unwrap();
    let lock = e
        .store
        .snapshot()
        .unwrap()
        .feature_configuration(feature["id"].as_str().unwrap())
        .unwrap();
    assert_eq!(lock.standard_ref, DEFAULT_STANDARD_ID);
    assert_eq!(
        lock.baseline_check.unwrap().standard_ref,
        DEFAULT_STANDARD_ID
    );
    assert!(lock.definition_hashes.contains_key(DEFAULT_STANDARD_ID));
}

#[test]
fn compacted_defaults_survive_bootstrap_and_resolve_revised_model_and_harness() {
    use std::collections::{BTreeMap, BTreeSet};
    fn remap(value: &mut Value, aliases: &BTreeMap<String, String>) {
        match value {
            Value::String(id) => {
                if let Some(latest) = aliases.get(id) {
                    *id = latest.clone();
                }
            }
            Value::Array(values) => values.iter_mut().for_each(|v| remap(v, aliases)),
            Value::Object(values) => values.values_mut().for_each(|v| remap(v, aliases)),
            _ => {}
        }
    }
    let source_dir = tempfile::tempdir().unwrap();
    let source = engine(source_dir.path());
    let mut compact = source.store.snapshot().unwrap();
    for old in [DEFAULT_MODEL_ID, DEFAULT_HARNESS_ID, DEFAULT_NATIVE_TOOL_ID] {
        let mut revised = compact.configuration(old).unwrap().clone();
        revised.id = format!("revised-{old}");
        revised.version += 1;
        revised.supersedes = Some(old.into());
        compact.governance.push(revised);
    }
    let mut latest = BTreeMap::new();
    for r in &compact.governance {
        if r.is_definition() && r.definition_id.starts_with("builtin-") {
            let entry = latest
                .entry((r.kind.clone(), r.definition_id.clone()))
                .or_insert(r);
            if r.version > entry.version {
                *entry = r;
            }
        }
    }
    let aliases: BTreeMap<_, _> = compact
        .governance
        .iter()
        .filter_map(|r| {
            let latest = latest.get(&(r.kind.clone(), r.definition_id.clone()))?;
            (r.id != latest.id).then(|| (r.id.clone(), latest.id.clone()))
        })
        .collect();
    compact.governance.retain(|r| !aliases.contains_key(&r.id));
    compact.definitions.retain(|r| !aliases.contains_key(&r.id));
    let mut value = json!(compact);
    remap(&mut value, &aliases);
    compact = serde_json::from_value(value).unwrap();
    for r in &mut compact.governance {
        r.supersedes = None;
        r.content_hash = configuration_hash(&r.data);
    }
    for d in &mut compact.definitions {
        d.supersedes = None;
    }
    let expected: BTreeSet<_> = compact.governance.iter().map(|r| r.id.clone()).collect();
    let destination = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(destination.path()).unwrap());
    store
        .transact(None, &mut |s| {
            *s = compact.clone();
            Ok(json!({}))
        })
        .unwrap();
    let e = Engine::new(
        store,
        Arc::new(FileArtifacts::new(destination.path().into())),
        runtime::adapters(),
    );
    for _ in 0..2 {
        e.bootstrap().unwrap();
        runtime::bind_default_permissions(&e);
        let state = e.store.snapshot().unwrap();
        let ids: BTreeSet<_> = state.governance.iter().map(|r| r.id.clone()).collect();
        assert_eq!(ids, expected);
        for old in aliases.keys() {
            assert!(!state.definitions.iter().any(|d| &d.id == old));
        }
        let defaults = e.configuration_defaults(None).unwrap();
        assert_eq!(defaults["model_ref"], format!("revised-{DEFAULT_MODEL_ID}"));
        assert_eq!(
            defaults["harness_ref"],
            format!("revised-{DEFAULT_HARNESS_ID}")
        );
        for old in aliases.keys() {
            assert!(
                !serde_json::to_string(&defaults)
                    .unwrap()
                    .contains(&format!("\"{old}\""))
            );
        }
    }
}

#[test]
fn shipped_defaults_have_stage_markdown_and_distinct_role_methods() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let defaults = e.configuration_defaults(None).unwrap();
    assert_eq!(
        defaults["role_agent_bindings"].as_object().unwrap().len(),
        6
    );
    let s = e.store.snapshot().unwrap();
    let workflow: WorkflowDocument = s
        .configuration(DEFAULT_FEATURE_WORKFLOW_V3_ID)
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(workflow.stages.len(), 8);
    assert!(
        workflow
            .stages
            .iter()
            .all(|stage| stage.description.starts_with('#'))
    );
    let pm = s
        .configuration(&default_configured_agent_id("project-manager"))
        .unwrap();
    let architect = s
        .configuration(&default_configured_agent_id("architect"))
        .unwrap();
    assert_ne!(pm.data["role_ref"], architect.data["role_ref"]);
    assert_ne!(pm.data["capability_ref"], architect.data["capability_ref"]);
    assert!(
        pm.data["skills"]
            .as_array()
            .unwrap()
            .contains(&json!("builtin-skill-project-planning-v2"))
    );
    assert!(
        architect.data["skills"]
            .as_array()
            .unwrap()
            .contains(&json!("builtin-skill-architecture-v2"))
    );
    assert!(
        !architect.data["skills"]
            .as_array()
            .unwrap()
            .contains(&json!("builtin-skill-project-planning-v2"))
    );
    let task = workflow
        .steps
        .iter()
        .find(|step| step.key == "task_discussion")
        .unwrap();
    assert_eq!(
        task.role_ref.as_deref(),
        Some("builtin-role-project-manager")
    );
    assert_eq!(
        task.spec_refs,
        vec!["builtin-spec-stage-project-planning-v1"]
    );
    for action in workflow.steps.iter().filter(|step| {
        [
            "agent.interact",
            "attempt.execute",
            "delivery.review",
            "merge.prepare",
            "merge.review",
        ]
        .contains(&step.kind.as_str())
    }) {
        assert!(!action.spec_refs.is_empty());
        for id in &action.spec_refs {
            let spec = s.configuration(id).unwrap();
            assert!(
                spec.data["body"]
                    .as_str()
                    .unwrap()
                    .contains("## 完成与拒绝标准")
            );
        }
    }
    let audit = e
        .query(
            "query.workflow_requirements",
            &json!({"id":DEFAULT_FEATURE_WORKFLOW_V3_ID}),
            4,
        )
        .unwrap();
    assert_eq!(audit["roles"].as_array().unwrap().len(), 6);
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
    assert!(snap.binding.standard.is_empty());
    assert_eq!(snap.binding.agent, default_configured_agent_id("analyst"));
}

#[test]
fn a_reused_session_dispatches_each_turn_with_its_frozen_step_specs() {
    use devdesk_core::ports;
    use std::sync::Mutex;
    struct Capture(Arc<Mutex<Vec<Vec<Id>>>>);
    impl ports::Harness for Capture {
        fn identity(&self) -> String {
            "fixture:captured-step-specs".into()
        }
        fn open_session(&self, r: &ports::SessionStart) -> Result<ports::SessionReceipt> {
            Ok(ports::SessionReceipt {
                external_session: format!("fixture-{}", r.session.id),
                container_id: "fixture-container".into(),
                image_digest: r.execution.image_digest.clone(),
                operation_id: r.execution.operation_id.clone(),
            })
        }
        fn dispatch_turn(&self, r: &ports::TurnDispatch) -> Result<()> {
            self.0
                .lock()
                .unwrap()
                .push(r.specifications.iter().map(|d| d.id.clone()).collect());
            Ok(())
        }
        fn poll_session(&self, _: &Session) -> Result<Vec<ports::ConversationObservation>> {
            Ok(vec![])
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let seen = Arc::new(Mutex::new(vec![]));
    let mut adapters = runtime::adapters();
    adapters.harness = Arc::new(Capture(seen.clone()));
    let e = Engine::new(
        Arc::new(SqliteStore::open(dir.path()).unwrap()),
        Arc::new(FileArtifacts::new(dir.path().into())),
        adapters,
    );
    e.bootstrap().unwrap();
    runtime::bind_default_permissions(&e);
    let p = command(
        &e,
        "project.create",
        None,
        json!({"title":"Frozen turn specs"}),
    )
    .unwrap();
    let pid = p["id"].as_str().unwrap();
    runtime::configure_project(&e, pid);
    let (template, _) = runtime::queue_conversation(&e, pid);
    let sid = id();
    let add_source = |s: &mut EngineeringState, spec: &str| {
        let data = json!({"spec_refs":[spec],"step":"fixture-prd","workflow_step_instance_id":"fixture-step"});
        s.governance.push(ConfigurationRecord {
            id: id(),
            kind: "execution_specification".into(),
            definition_id: sid.clone(),
            scope: Scope::Project {
                project_id: pid.into(),
            },
            name: "Explicit fixture Spec source".into(),
            version: 1,
            supersedes: None,
            content_hash: configuration_hash(&data),
            data,
            immutable: true,
            revision: 1,
            created_at: 1,
        });
    };
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
            add_source(s, "builtin-spec-stage-prd-v1");
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
    e.store
        .transact(None, &mut |s| {
            add_source(s, "builtin-spec-stage-architecture-v1");
            Ok(Value::Null)
        })
        .unwrap();
    let s = e.store.snapshot().unwrap();
    let input = s
        .turns
        .iter()
        .find(|t| Some(t.id.as_str()) == turn["id"].as_str())
        .unwrap()
        .input_snapshot
        .as_ref()
        .unwrap();
    let captured: Value = serde_json::from_str(
        &s.snapshots
            .iter()
            .find(|snap| &snap.id == input)
            .unwrap()
            .content,
    )
    .unwrap();
    assert_eq!(
        captured["specifications"][0]["id"],
        "builtin-spec-stage-prd-v1"
    );
    assert_eq!(
        captured["specification_source"]["workflow_step_instance_id"],
        "fixture-step"
    );
    e.tick_conversations(4).unwrap();
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .any(|ids| ids == &vec!["builtin-spec-stage-prd-v1".to_owned()])
    );
}

#[test]
fn subworkflow_failure_propagates_and_rework_restarts_its_children() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let child = publish(
        &e,
        "Child snapshot",
        json!({"schema_version":2,"process_kind":"subworkflow","stages":[{"key":"context","name":"Context"}],"steps":[{"key":"capture","name":"Capture","kind":"snapshot.capture","stage":"context"}]}),
    );
    let defaults = e.configuration_defaults(None).unwrap();
    let mut doc = e
        .store
        .snapshot()
        .unwrap()
        .configuration(defaults["workflow_ref"].as_str().unwrap())
        .unwrap()
        .data
        .clone();
    let step = doc["steps"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|step| step["key"] == "initial_snapshot")
        .unwrap();
    step["kind"] = json!("workflow.call");
    step["subworkflow_ref"] = child["id"].clone();
    let parent = publish(&e, "Call failure", doc);
    let p = command(&e, "project.create", None, json!({"title":"Recover child"})).unwrap();
    command(
        &e,
        "project.configuration.update",
        p["id"].as_str(),
        json!({"workflow_ref":parent["id"],"role_agent_bindings":defaults["role_agent_bindings"]}),
    )
    .unwrap();
    let f = command(
        &e,
        "feature.create",
        None,
        json!({"project_id":p["id"],"title":"Child recovery"}),
    )
    .unwrap();
    let fid = f["id"].as_str().unwrap();
    e.evaluate_workflows().unwrap();
    let intent = e
        .pending_workflow_intents()
        .unwrap()
        .into_iter()
        .find(|i| i.data["method"] == "snapshot.save")
        .unwrap();
    e.observe_workflow_intent(
        &intent.id,
        &Err(err("fixture_unavailable", "Declared snapshot failure")),
    )
    .unwrap();
    e.evaluate_workflows().unwrap();
    let read = || {
        e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .find(|r| r.kind == "workflow_instance" && r.definition_id == fid)
            .unwrap()
            .decode::<WorkflowRuntime>()
            .unwrap()
    };
    let runtime = read();
    let call = runtime
        .steps
        .iter()
        .find(|s| s.key == "initial_snapshot")
        .unwrap();
    assert_eq!(call.status, "blocked");
    e.execute(
        "workflow.step.rework",
        Command {
            command_id: id(),
            target: Some(call.id.clone()),
            expected_revision: Some(call.revision),
            payload: json!({"from_step":"initial_snapshot"}),
        },
    )
    .unwrap();
    let runtime = read();
    assert!(
        runtime
            .steps
            .iter()
            .filter(|s| s.key == "initial_snapshot"
                || s.key.contains("capture")
                || s.key.ends_with("entry"))
            .all(|s| s.round == 2 && s.status == "pending")
    );
}

#[test]
fn specification_applicability_uses_the_actual_discussion_output_contract() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let doc = json!({"schema_version":2,"process_kind":"subworkflow","stages":[{"key":"work","name":"Work"}],"steps":[{"key":"design","name":"Design","stage":"work","kind":"agent.interact","role_ref":"builtin-role-architect","spec_refs":["builtin-spec-stage-prd-v1"],"parameters":{"phase":"requirement"},"interaction":{"goal":"Produce a design","output_kind":"design","max_turns":4}}]});
    assert!(
        e.query(
            "query.workflow_requirements",
            &json!({"document":workflow_fixture::current(doc.clone())}),
            4
        )
        .unwrap_err()
        .message
        .contains("不适用")
    );
}
