//! Contracts for Profile-only permissions, owned installation inputs and Workflow scheduling.
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
use devdesk_adapters::{
    FileArtifacts, SqliteStore, permission_profiles::compile_permission_profiles,
};
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::json;
use std::sync::Arc;
fn engine(path: &std::path::Path, bind: bool) -> Engine {
    let engine = Engine::new(
        Arc::new(SqliteStore::open(path).unwrap()),
        Arc::new(FileArtifacts::new(path.into())),
        runtime::adapters(),
    );
    engine.bootstrap().unwrap();
    if bind {
        runtime::bind_default_permissions(&engine);
    }
    engine
}
#[test]
fn step_profiles_are_the_only_scope_and_do_not_share_a_lock_by_agent() {
    let directory = tempfile::tempdir().unwrap();
    let engine = engine(directory.path(), true);
    let state = engine.store.snapshot().unwrap();
    let defaults = engine.configuration_defaults(None).unwrap();
    let workflow = state
        .configuration(defaults["task_workflow_ref"].as_str().unwrap())
        .unwrap();
    let bindings = serde_json::from_value(defaults["role_agent_bindings"].clone()).unwrap();
    let plan = engine
        .compile_configured_workflow(&state, workflow, &Scope::global(), Some(&bindings))
        .unwrap();
    let test = plan
        .permission_locks
        .get(&format!("{}:tests", workflow.id))
        .unwrap();
    let implementation = plan
        .permission_locks
        .get(&format!("{}:implementation", workflow.id))
        .unwrap();
    let scope = compile_permission_profiles(&state, test, None)
        .unwrap()
        .scope;
    assert_eq!(scope.writable_paths, vec!["tests"]);
    let scope = compile_permission_profiles(&state, implementation, None)
        .unwrap()
        .scope;
    assert_eq!(scope.writable_paths, vec!["."]);
    assert_eq!(scope.read_only_paths, vec!["tests"]);
    assert_ne!(test.harness.profile.id, implementation.harness.profile.id);
    assert!(state.governance.iter().all(|r| {
        ![
            "permission_context",
            "runtime_resource",
            "orchestration_policy",
        ]
        .contains(&r.r#type.as_str())
    }));
    assert!(defaults.get("permission_context_ref").is_none());
    assert!(defaults.get("runtime_resources").is_none());
}
#[test]
fn explicit_profiles_replace_defaults_without_requiring_a_default_binding() {
    let directory = tempfile::tempdir().unwrap();
    let engine = engine(directory.path(), false);
    let state = engine.store.snapshot().unwrap();
    let before = state.governance.len();
    let selection = ExecutionPermissionSelection {
        sandbox_ref: "builtin-sandbox-docker:1".into(),
        harness: PermissionSelection::Profile {
            permission_revision_id: "builtin-permission-harness-read-only:1".into(),
        },
        sandbox: PermissionSelection::Profile {
            permission_revision_id: "builtin-permission-sandbox-full-access:1".into(),
        },
    };
    let lock =
        resolve_permission_lock(&state, "builtin-harness-codex-managed:1", &selection).unwrap();
    let permissions = compile_permission_profiles(&state, &lock, None).unwrap();
    validate_frozen_permissions(&state, &permissions).unwrap();
    assert!(permissions.scope.writable_paths.is_empty());
    assert_eq!(state.governance.len(), before);
}
#[test]
fn workflow_snapshot_contains_scheduling_and_selected_profiles() {
    let directory = tempfile::tempdir().unwrap();
    let engine = engine(directory.path(), true);
    let state = engine.store.snapshot().unwrap();
    let defaults = engine.configuration_defaults(None).unwrap();
    let snapshot = resolve_standard_sources(
        &state,
        &[defaults["task_workflow_ref"].as_str().unwrap().into()],
        &Scope::global(),
    )
    .unwrap();
    let workflow = snapshot
        .nodes
        .iter()
        .find(|n| n.kind == "workflow")
        .unwrap();
    assert_eq!(workflow.content["scheduling"]["max_concurrency"], 4);
    assert_eq!(
        workflow.content["scheduling"]["agent"]["max_turns_per_wakeup"],
        8
    );
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|n| n.kind == "permission_profile")
    );
    for kind in [
        "permission_context",
        "runtime_resource",
        "orchestration_policy",
    ] {
        assert!(
            SourceDefinition {
                r#type: kind.into(),
                identity: "removed".into(),
                revision: 1,
                name: "Removed".into(),
                scope: Scope::global(),
                refs: References::new(),
                data: json!({})
            }
            .into_record()
            .is_err()
        );
    }
}
#[test]
fn installation_version_and_sandbox_inputs_both_determine_composed_content() {
    let directory = tempfile::tempdir().unwrap();
    let engine = engine(directory.path(), false);
    let state = engine.store.snapshot().unwrap();
    let mut harness = state
        .configuration("builtin-harness-codex-managed:1")
        .unwrap()
        .clone();
    let mut sandbox = state
        .configuration("builtin-sandbox-docker:1")
        .unwrap()
        .clone();
    let original = compose_execution_recipe(&harness, &sandbox).unwrap();
    assert!(original.files[0].content.contains("@openai/codex@0.167.0"));
    harness.data["runtime_version"] = json!("0.167.1");
    let different = compose_execution_recipe(&harness, &sandbox).unwrap();
    assert_ne!(original.recipe_digest(), different.recipe_digest());
    assert_eq!(original.native_releases, different.native_releases);
    sandbox.data["build"]["packages"]
        .as_array_mut()
        .unwrap()
        .push(json!("git"));
    assert_ne!(
        different.recipe_digest(),
        compose_execution_recipe(&harness, &sandbox)
            .unwrap()
            .recipe_digest()
    );
    sandbox.data["build"]["base_image"] = json!("node:latest");
    assert!(compose_execution_recipe(&harness, &sandbox).is_err());
}

#[test]
#[ignore = "requires an explicit native Codex binary and a dedicated DEVDESK_RUNTIME_CACHE; no OAuth/model calls"]
fn native_login_asset_is_shared_and_recovered_without_downloading_or_sharing_credentials() {
    use devdesk_adapters::GlobalStore;
    use sha2::{Digest, Sha256};
    let binary = std::env::var("DEVDESK_TEST_CODEX_BINARY").expect("explicit native Codex binary");
    assert!(
        std::env::var_os("DEVDESK_RUNTIME_CACHE").is_some(),
        "dedicated cache required"
    );
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let setup = |path: &std::path::Path| {
        let store = Arc::new(GlobalStore::open(path).unwrap());
        let e = Engine::new(
            store.clone(),
            Arc::new(FileArtifacts::new(path.into())),
            runtime::adapters(),
        );
        e.bootstrap().unwrap();
        store
    };
    let a = setup(first.path());
    let b = setup(second.path());
    let (_, recipe) = a.default_execution_recipe(HarnessProvider::Codex).unwrap();
    let release =
        &recipe.native_releases[&format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH)];
    a.register_runtime(RuntimeRegistration {
        key: RuntimeKey {
            provider: HarnessProvider::Codex,
            version: recipe.version,
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
        },
        artifact: RuntimeArtifact::Executable {
            canonical_path: binary.clone(),
            sha256: format!("{:x}", Sha256::digest(std::fs::read(&binary).unwrap())),
        },
        source: release.url.clone(),
        license: "Apache-2.0".into(),
    })
    .unwrap();
    let events = std::sync::Mutex::new(Vec::new());
    let installed = a
        .prepare_codex_auth(|phase| events.lock().unwrap().push(phase.to_owned()))
        .unwrap();
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .any(|phase| phase.contains("复用已有"))
    );
    let shared = b
        .prepare_codex_auth(|phase| assert!(!phase.contains("下载")))
        .unwrap();
    assert_eq!(installed.artifact, shared.artifact);
    let conn = rusqlite::Connection::open(second.path().join("global.sqlite3")).unwrap();
    conn.execute("DELETE FROM runtime_installations", [])
        .unwrap();
    let recovered = b
        .prepare_codex_auth(|phase| assert!(!phase.contains("下载")))
        .unwrap();
    assert_eq!(installed.artifact, recovered.artifact);
    assert!(!second.path().join("credentials/codex/auth.json").exists());
    assert!(a.prepare_codex_auth_cancellable(|_| {}, &|| true).is_err());
}

#[test]
fn project_step_profile_can_restrict_a_global_harness_without_rebinding_it() {
    let directory = tempfile::tempdir().unwrap();
    let engine = engine(directory.path(), true);
    let mut state = engine.store.snapshot().unwrap();
    let defaults = engine.configuration_defaults(None).unwrap();
    let mut profile = state
        .configuration("builtin-permission-harness-read-only:1")
        .unwrap()
        .source();
    profile.identity = "local-read-only".into();
    profile.scope = Scope::project("local");
    state.governance.push(profile.into_record().unwrap());
    let mut workflow = state
        .configuration(defaults["task_workflow_ref"].as_str().unwrap())
        .unwrap()
        .source();
    workflow.identity = "local-workflow".into();
    workflow.scope = Scope::project("local");
    let step = workflow.data["steps"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|step| step["key"] == "tests")
        .unwrap();
    step["harness_permission_ref"] = json!("local-read-only:1");
    let workflow = workflow.into_record().unwrap();
    let bindings = serde_json::from_value(defaults["role_agent_bindings"].clone()).unwrap();
    let plan = engine
        .compile_configured_workflow(&state, &workflow, &Scope::project("local"), Some(&bindings))
        .unwrap();
    let lock = &plan.permission_locks[&format!("{}:tests", workflow.id)];
    assert_eq!(lock.harness.profile.id, "local-read-only:1");
    assert!(
        compile_permission_profiles(&state, lock, None)
            .unwrap()
            .scope
            .writable_paths
            .is_empty()
    );
    assert!(
        engine
            .compile_configured_workflow(
                &state,
                &workflow,
                &Scope::project("other"),
                Some(&bindings)
            )
            .is_err()
    );
}

#[test]
fn frozen_materialization_uses_its_sandbox_after_the_project_default_changes() {
    let directory = tempfile::tempdir().unwrap();
    let engine = engine(directory.path(), false);
    let state = engine.store.snapshot().unwrap();
    let harness = state
        .configuration("builtin-harness-codex-managed:1")
        .unwrap()
        .clone();
    let sandbox = state.configuration("builtin-sandbox-docker:1").unwrap();
    let expected = compose_execution_recipe(&harness, sandbox)
        .unwrap()
        .recipe_digest();
    let mut source = sandbox.source();
    source.identity = "later-sandbox".into();
    source.data["build"]["packages"] = json!(["ca-certificates", "bubblewrap", "git"]);
    let sandbox = source.into_record().unwrap();
    engine
        .store
        .transact(None, &mut |state| {
            state.governance.push(sandbox.clone());
            let source = state
                .governance
                .iter_mut()
                .find(|r| r.r#type == "template_catalog_state")
                .unwrap();
            source.data["defaults"]["sandbox_ref"] = json!(sandbox.id);
            source.content_hash = content_digest(&source.refs, &source.data);
            source.revision += 1;
            Ok(serde_json::Value::Null)
        })
        .unwrap();
    let resources = runtime::ResourceFixture {
        store: engine.store.as_ref(),
    };
    let result = engine
        .materialize_harness_in_sandbox(&resources, &harness, "builtin-sandbox-docker:1")
        .unwrap();
    assert_eq!(result.recipe_digest.as_deref(), Some(expected.as_str()));
    let state = engine.store.snapshot().unwrap();
    let receipt = state
        .governance
        .iter()
        .find(|r| r.r#type == "harness_materialization")
        .unwrap();
    assert_eq!(receipt.data["sandbox_ref"], "builtin-sandbox-docker:1");
}
