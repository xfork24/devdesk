//! Project standards are immutable effective graphs, with frozen consumers and durable provenance.
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
use devdesk_adapters::{FileArtifacts, SqliteStore, integrations::NativeTools};
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
fn cmd(e: &Engine, method: &str, target: Option<&str>, payload: Value) -> Result<Value> {
    let s = e.store.snapshot()?;
    let rev = target.and_then(|id| {
        s.configuration(id)
            .ok()
            .map(|r| r.revision)
            .or_else(|| s.projects.iter().find(|r| r.id == id).map(|r| r.revision))
            .or_else(|| s.features.iter().find(|r| r.id == id).map(|r| r.revision))
    });
    e.execute(
        method,
        Command {
            command_id: id(),
            target: target.map(str::to_owned),
            expected_revision: rev,
            payload,
        },
    )
}
fn select(defaults: &Value, overrides: Value) -> Value {
    json!({"standard_ref":defaults["standard_ref"],"standard_overrides":overrides,
        "role_agent_bindings":defaults["role_agent_bindings"],"permission_context_ref":defaults["permission_context_ref"],
        "orchestrator_role_ref":defaults["orchestrator_role_ref"]})
}

fn source_hashes(state: EngineeringState) -> std::collections::BTreeMap<Id, String> {
    state
        .governance
        .into_iter()
        .filter(|r| r.is_definition() || r.r#type == "standard_derivation")
        .map(|r| (r.id, r.content_hash))
        .collect()
}

#[test]
fn selecting_an_existing_standard_without_overrides_reuses_it_without_deriving_sources() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let project = cmd(
        &e,
        "project.create",
        None,
        json!({"title":"Reuse standard"}),
    )
    .unwrap();
    let pid = project["id"].as_str().unwrap();
    let defaults = e.configuration_defaults(Some(pid)).unwrap();
    let before = source_hashes(e.store.snapshot().unwrap());
    let preview = e.query("query.project_standard_preview", &json!({"project_id":pid,"standard_ref":defaults["standard_ref"],"permission_context_ref":defaults["permission_context_ref"]}), 4).unwrap();
    assert_eq!(preview["standard_ref"], defaults["standard_ref"]);
    assert_eq!(preview["default_workflow_ref"], defaults["workflow_ref"]);
    assert_eq!(preview["provenance"]["diff"]["equal"], true);
    assert_eq!(source_hashes(e.store.snapshot().unwrap()), before);
    for explicit_overrides in [false, true] {
        let mut selection = select(&defaults, json!([]));
        if !explicit_overrides {
            selection
                .as_object_mut()
                .unwrap()
                .remove("standard_overrides");
        }
        selection["standard_preview_hash"] = preview["preview_hash"].clone();
        let saved = cmd(&e, "project.configuration.update", Some(pid), selection).unwrap();
        assert_eq!(saved["data"]["standard_ref"], defaults["standard_ref"]);
        assert_eq!(saved["data"]["workflow_ref"], defaults["workflow_ref"]);
        assert_eq!(
            saved["data"]["permission_context_ref"],
            defaults["permission_context_ref"]
        );
        assert_eq!(source_hashes(e.store.snapshot().unwrap()), before);
    }
}

#[test]
fn project_override_rewrites_the_graph_and_permissions_without_changing_frozen_features() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let project = cmd(
        &e,
        "project.create",
        None,
        json!({"title":"Project standard"}),
    )
    .unwrap();
    let pid = project["id"].as_str().unwrap();
    let defaults = e.configuration_defaults(Some(pid)).unwrap();
    let base = defaults["standard_ref"].as_str().unwrap();
    let original = e.store.snapshot().unwrap();
    assert_eq!(
        standard_default_workflow(&original, base, &Scope::project(pid)).unwrap(),
        defaults["workflow_ref"]
    );
    let first = cmd(
        &e,
        "project.configuration.update",
        Some(pid),
        select(&defaults, json!([])),
    )
    .unwrap();
    let old_standard = first["data"]["standard_ref"].as_str().unwrap();
    assert_eq!(old_standard, base);
    let feature = cmd(
        &e,
        "feature.create",
        None,
        json!({"project_id":pid,"title":"Frozen","initial_requirement":"保持原规范"}),
    )
    .unwrap();
    let fid = feature["id"].as_str().unwrap();
    let old_root = e
        .store
        .snapshot()
        .unwrap()
        .governance
        .into_iter()
        .find(|r| r.r#type == "workflow_instance" && r.data["owner_id"] == fid)
        .unwrap();
    let old_spec = original.configuration("builtin-spec-stage-prd:2").unwrap();
    let mut data = old_spec.data.clone();
    data["body"] = json!(format!(
        "{}\n项目新增检查要求。",
        data["body"].as_str().unwrap()
    ));
    let revised = cmd(
        &e,
        "spec.revise",
        Some(&old_spec.id),
        json!({"name":"Project PRD requirements","data":data}),
    )
    .unwrap();
    let overrides = json!([{"source_ref":old_spec.id,"replacement_ref":revised["id"]}]);
    let before_preview = e.store.snapshot().unwrap();
    let preview = e.query("query.project_standard_preview", &json!({"project_id":pid,"standard_ref":base,"standard_overrides":overrides,"permission_context_ref":defaults["permission_context_ref"]}), 4).unwrap();
    assert_eq!(
        before_preview.governance.len(),
        e.store.snapshot().unwrap().governance.len()
    );
    assert_ne!(preview["default_workflow_ref"], defaults["workflow_ref"]);
    assert_eq!(preview["provenance"]["diff"]["equal"], false);
    assert!(
        preview["snapshot"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["reference"] == revised["id"])
    );
    assert!(
        !preview["snapshot"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["reference"] == old_spec.id)
    );
    let mut selection = select(&defaults, overrides.clone());
    selection["standard_preview_hash"] = preview["preview_hash"].clone();
    let saved = cmd(&e, "project.configuration.update", Some(pid), selection).unwrap();
    let effective = saved["data"]["standard_ref"].as_str().unwrap();
    let state = e.store.snapshot().unwrap();
    assert_eq!(
        state.configuration(effective).unwrap().scope,
        Scope::project(pid)
    );
    assert_eq!(
        state.configuration(base).unwrap(),
        original.configuration(base).unwrap()
    );
    assert_eq!(state.configuration(&old_spec.id).unwrap(), old_spec);
    assert_eq!(state.configuration(&old_root.id).unwrap(), &old_root);
    assert_eq!(old_root.data["plan"]["standard_ref"], old_standard);
    assert_ne!(
        saved["data"]["permission_context_ref"],
        defaults["permission_context_ref"]
    );
    let fresh = cmd(
        &e,
        "feature.create",
        None,
        json!({"project_id":pid,"title":"Uses override"}),
    )
    .unwrap();
    let fresh_root = e
        .store
        .snapshot()
        .unwrap()
        .governance
        .into_iter()
        .find(|r| r.r#type == "workflow_instance" && r.data["owner_id"] == fresh["id"])
        .unwrap();
    assert_eq!(fresh_root.data["plan"]["standard_ref"], effective);
    let other = cmd(
        &e,
        "project.create",
        None,
        json!({"title":"Another project"}),
    )
    .unwrap();
    let mut forbidden = select(&defaults, json!([]));
    forbidden["standard_ref"] = json!(effective);
    assert!(
        cmd(
            &e,
            "project.configuration.update",
            other["id"].as_str(),
            forbidden
        )
        .is_err()
    );
    drop(e);
    let e = engine(dir.path());
    let restored = e
        .query(
            "query.standard",
            &json!({"id":effective,"project_id":pid,"limit":1000}),
            4,
        )
        .unwrap();
    assert_eq!(restored["provenance"]["base_standard_ref"], base);
    assert_eq!(restored["provenance"]["overrides"], overrides);
    assert_eq!(
        restored["snapshot_digest"],
        preview["snapshot"]["snapshot_digest"]
    );
    let mut restored_runtime = e
        .store
        .snapshot()
        .unwrap()
        .configuration(&old_root.id)
        .unwrap()
        .data
        .clone();
    let mut original_runtime = old_root.data.clone();
    // Startup marks the runtime dirty for evaluation; the entire frozen contract stays identical.
    restored_runtime.as_object_mut().unwrap().remove("dirty");
    original_runtime.as_object_mut().unwrap().remove("dirty");
    assert_eq!(
        configuration_hash(&restored_runtime),
        configuration_hash(&original_runtime)
    );
    let before_revert = e.store.snapshot().unwrap();
    let reverted = cmd(
        &e,
        "project.configuration.update",
        Some(pid),
        select(&defaults, json!([])),
    )
    .unwrap();
    assert_eq!(reverted["data"]["standard_ref"], base);
    assert_eq!(
        reverted["data"]["permission_context_ref"],
        defaults["permission_context_ref"]
    );
    let after_revert = e.store.snapshot().unwrap();
    assert_eq!(
        after_revert.configuration(effective).unwrap(),
        before_revert.configuration(effective).unwrap()
    );
    assert_eq!(
        after_revert.configuration(&old_root.id).unwrap(),
        before_revert.configuration(&old_root.id).unwrap()
    );
    assert_eq!(source_hashes(after_revert), source_hashes(before_revert));
}

#[test]
fn project_standard_rejects_stale_duplicate_and_wrong_type_overrides_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let project = cmd(
        &e,
        "project.create",
        None,
        json!({"title":"Invalid overrides"}),
    )
    .unwrap();
    let pid = project["id"].as_str().unwrap();
    let defaults = e.configuration_defaults(Some(pid)).unwrap();
    let before = e.store.snapshot().unwrap().governance.len();
    let mut selection = select(&defaults, json!([]));
    selection["standard_preview_hash"] = json!("stale");
    assert!(
        cmd(&e, "project.configuration.update", Some(pid), selection)
            .unwrap_err()
            .message
            .contains("预览")
    );
    for overrides in [
        json!([{"source_ref":"builtin-spec-stage-prd:2","replacement_ref":defaults["workflow_ref"]}]),
        json!([{"source_ref":"not-in-snapshot:1","replacement_ref":"builtin-spec-stage-prd:2"}]),
        json!([{"source_ref":"builtin-spec-stage-prd:2","replacement_ref":"builtin-spec-stage-architecture:2"},{"source_ref":"builtin-spec-stage-prd:2","replacement_ref":"builtin-spec-stage-architecture:2"}]),
    ] {
        assert!(
            cmd(
                &e,
                "project.configuration.update",
                Some(pid),
                select(&defaults, overrides)
            )
            .is_err()
        );
        assert_eq!(before, e.store.snapshot().unwrap().governance.len());
    }
}

#[test]
fn standard_comparison_ignores_missing_and_different_default_entries() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let defaults = e.configuration_defaults(None).unwrap();
    let base = defaults["standard_ref"].as_str().unwrap();
    let state = e.store.snapshot().unwrap();
    let roots = reference_list(
        &state.configuration(base).unwrap().refs,
        "root_workflow_refs",
    )
    .unwrap();
    for entry in [Value::Null, defaults["task_workflow_ref"].clone()] {
        let after = cmd(
            &e,
            "standard.capture",
            None,
            json!({"name":"Different default", "root_workflow_refs":roots, "default_workflow_ref":entry}),
        )
        .unwrap();
        assert_ne!(after["default_workflow_ref"], defaults["workflow_ref"]);
        for (before, after) in [
            (json!(base), after["id"].clone()),
            (after["id"].clone(), json!(base)),
        ] {
            let diff = e
                .query(
                    "query.standard_diff",
                    &json!({"before_ref":before,"after_ref":after}),
                    4,
                )
                .unwrap();
            assert_eq!(diff["before_digest"], diff["after_digest"]);
            assert_eq!(diff["equal"], true);
            assert_eq!(diff["changes"], json!([]));
        }
    }
}
