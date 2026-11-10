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
fn capability_is_independent_and_agent_components_are_frozen_from_it() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let before = e.store.snapshot().unwrap();
    let old = before
        .governance
        .iter()
        .find(|r| r.kind == "agent" && r.data["role_ref"] == default_role_id("analyst"))
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
    )
    .with_tool_executor(Arc::new(
        devdesk_adapters::integrations::NativeTools::new(destination.path()).unwrap(),
    ));
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
