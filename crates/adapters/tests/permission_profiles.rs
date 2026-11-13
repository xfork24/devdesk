#[path = "../../../tests/support/defaults.rs"]
mod default_fixture;
use default_fixture::*;
use devdesk_adapters::{permission_profiles::*, *};
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
use std::sync::Arc;
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
#[path = "../../../tests/support/schemas.rs"]
mod schemas;

fn global(path: &std::path::Path) -> (Arc<GlobalStore>, Engine) {
    let store = Arc::new(GlobalStore::open(path).unwrap());
    let engine = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(path.into())),
        runtime::adapters(),
    )
    .with_tool_executor(Arc::new(
        devdesk_adapters::integrations::NativeTools::new(path).unwrap(),
    ));
    engine.bootstrap().unwrap();
    (store, engine)
}
fn command(
    method: &str,
    payload: Value,
    target: Option<&str>,
    expected: Option<u64>,
) -> (String, Command) {
    (
        method.into(),
        Command {
            command_id: id(),
            target: target.map(str::to_owned),
            expected_revision: expected,
            payload,
        },
    )
}
fn execute(e: &Engine, c: (String, Command)) -> Result<Value> {
    let value = e.execute(&c.0, c.1)?;
    if value["type"] == "permission_profile" {
        let source: SourceDefinition = serde_json::from_value(value.clone()).unwrap();
        Ok(
            json!({"id":source_reference(&source.identity,source.revision)?,"identity":source.identity,"version":source.revision,"name":source.name,"policy":source.data["policy"],"target_kind":source.data["target_kind"],"source":value}),
        )
    } else {
        Ok(value)
    }
}
fn change(
    e: &Engine,
    kind: PermissionTargetKind,
    target: &str,
    profile: &str,
    expected: u64,
) -> Value {
    let s = e.store.snapshot().unwrap();
    let target = permission_target(&s, kind, target).unwrap();
    json!({"target":target,"permission_revision_id":profile,"expected_revision":expected,"enforcement_contract":permission_contract(&s,&target).unwrap()})
}
fn apply(e: &Engine, changes: Vec<Value>) -> Result<Value> {
    execute(
        e,
        command(
            "permission.bindings.apply",
            json!({"bindings":changes}),
            None,
            None,
        ),
    )
}
fn lock(e: &Engine, harness: &str) -> VersionedPermissionLock {
    resolve_permission_lock(
        &e.store.snapshot().unwrap(),
        harness,
        &ExecutionPermissionSelection {
            sandbox_ref: DEFAULT_PERMISSION_SANDBOX.into(),
            harness: PermissionSelection::FollowTarget,
            sandbox: PermissionSelection::FollowTarget,
        },
    )
    .unwrap()
}
fn bind_pair(e: &Engine, harness: &str, profile: &str) {
    apply(
        e,
        vec![
            change(e, PermissionTargetKind::Harness, harness, profile, 0),
            change(
                e,
                PermissionTargetKind::Sandbox,
                DEFAULT_PERMISSION_SANDBOX,
                "builtin-permission-sandbox-full-access:1",
                0,
            ),
        ],
    )
    .unwrap();
}
fn profile(e: &Engine, id: &str) -> PermissionProfile {
    e.store
        .snapshot()
        .unwrap()
        .configuration(id)
        .unwrap()
        .permission_profile()
        .unwrap()
}

#[test]
fn strict_profiles_share_canonical_hashes_and_reject_v1() {
    let dir = tempfile::tempdir().unwrap();
    let (store, e) = global(dir.path());
    let before = store
        .snapshot()
        .unwrap()
        .governance
        .iter()
        .filter(|r| r.r#type == "permission_profile")
        .map(|r| r.id.clone())
        .collect::<Vec<_>>();
    store
        .reload_permission_presets(&command("reload", json!({}), None, None).1)
        .unwrap();
    assert_eq!(
        before,
        store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .filter(|r| r.r#type == "permission_profile")
            .map(|r| r.id.clone())
            .collect::<Vec<_>>()
    );
    assert!(
        insert_permission_profiles(
            &mut EngineeringState::default(),
            &[json!({"schema_version":1})],
            "fixture"
        )
        .is_err()
    );
    let p = profile(&e, "builtin-permission-harness-read-only:1");
    let r = store
        .snapshot()
        .unwrap()
        .configuration(&p.id)
        .unwrap()
        .clone();
    assert_eq!(
        r.content_hash,
        content_digest(&r.refs, &permission_profile_data(&p))
    );
    schemas::SCHEMAS.assert_valid("permission-profile.schema.json", &json!(p));
    let mut bad = json!(p);
    bad["data"]["policy"]["tools"] = Value::Null;
    assert!(serde_json::from_value::<PermissionProfile>(bad.clone()).is_err());
    assert!(
        !schemas::SCHEMAS
            .validator("permission-profile.schema.json")
            .is_valid(&bad)
    );
    bad = json!(p);
    bad["data"]["policy"]["typo"] = json!(true);
    assert!(serde_json::from_value::<PermissionProfile>(bad).is_err());
    let mut bad = p.clone();
    bad.target_kind = PermissionTargetKind::Sandbox;
    assert_eq!(bad.normalize().unwrap_err().code, "permission_inapplicable");
}

#[test]
fn import_is_atomic_and_revision_export_round_trips_in_a_fresh_store() {
    let dir = tempfile::tempdir().unwrap();
    let (_, e) = global(dir.path());
    let payload = json!({"identity":"custom-review","name":"Custom review","target_kind":"harness","policy":profile(&e,"builtin-permission-harness-read-only:1").policy});
    let first = execute(
        &e,
        command("permission.profile.create", payload, None, None),
    )
    .unwrap();
    let revised = execute(
        &e,
        command(
            "permission.profile.revise",
            json!({"name":"Next","target_kind":"harness","policy":first["policy"]}),
            first["id"].as_str(),
            Some(1),
        ),
    )
    .unwrap();
    let export = permission_profile_export(
        &e.store.snapshot().unwrap(),
        revised["id"].as_str().unwrap(),
    )
    .unwrap();
    let other = tempfile::tempdir().unwrap();
    let (_, second) = global(other.path());
    execute(
        &second,
        command("permission.profile.import", export.clone(), None, None),
    )
    .unwrap();
    assert_eq!(
        permission_profile_export(
            &second.store.snapshot().unwrap(),
            revised["id"].as_str().unwrap()
        )
        .unwrap(),
        export
    );
    let mut valid = first["source"].clone();
    valid["identity"] = json!("atomic-new");
    let mut conflict = first["source"].clone();
    conflict["name"] = json!("conflict");
    assert_eq!(
        execute(
            &e,
            command(
                "permission.profile.import",
                json!({"profiles":[valid.clone(),conflict]}),
                None,
                None
            )
        )
        .unwrap_err()
        .code,
        "revision_conflict"
    );
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .configuration(&source_reference(valid["identity"].as_str().unwrap(), 1).unwrap())
            .is_err()
    );
    assert!(
        execute(
            &e,
            command(
                "permission.profile.revise",
                json!({"name":"branch","target_kind":"harness","policy":first["policy"]}),
                first["id"].as_str(),
                Some(1)
            )
        )
        .is_err()
    );
}

#[test]
fn runtime_directory_reload_is_idempotent_and_does_not_assign_targets() {
    let dir = tempfile::tempdir().unwrap();
    let (store, e) = global(dir.path());
    let root = dir
        .path()
        .join("resources/configuration/permissions/profiles");
    std::fs::create_dir_all(&root).unwrap();
    let mut p = profile(&e, "builtin-permission-harness-read-only:1");
    p.id = "runtime-added:1".into();
    p.identity = "runtime-added".into();
    std::fs::write(root.join("new.json"), serde_json::to_vec(&p).unwrap()).unwrap();
    store
        .select_permission_preset_source(&command("select", json!({"path":root}), None, None).1)
        .unwrap();
    let c = command("reload", json!({}), None, None).1;
    assert!(
        store.reload_permission_presets(&c).unwrap()["added"]
            .as_array()
            .unwrap()
            .contains(&json!(p.id))
    );
    assert_eq!(
        store.reload_permission_presets(&c).unwrap()["added"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .all(|r| r.r#type != "permission_assignment")
    );
    p.name = "changed same ID".into();
    std::fs::write(root.join("new.json"), serde_json::to_vec(&p).unwrap()).unwrap();
    assert_eq!(
        store
            .reload_permission_presets(&command("reload", json!({}), None, None).1)
            .unwrap_err()
            .code,
        "revision_conflict"
    );
}

#[test]
fn preset_source_is_single_persistent_and_recoverable_with_builtin_reload() {
    let dir = tempfile::tempdir().unwrap();
    let (store, e) = global(dir.path());
    let builtin = store.permission_preset_source().unwrap();
    assert!(builtin.is_builtin);
    assert_eq!(builtin.path, builtin.builtin_path);
    // The former automatic dataset source no longer participates in loading.
    let former = dir
        .path()
        .join("resources/configuration/permissions/profiles");
    std::fs::create_dir_all(&former).unwrap();
    std::fs::write(former.join("invalid.json"), "invalid").unwrap();
    store
        .reload_permission_presets(&command("reload", json!({}), None, None).1)
        .unwrap();

    let first = dir.path().join("团队 预设");
    let second = dir.path().join("个人预设");
    for (path, name) in [(&first, "team"), (&second, "personal")] {
        std::fs::create_dir(path).unwrap();
        let mut p = profile(&e, "builtin-permission-harness-read-only:1");
        p.id = format!("{name}:1");
        p.identity = name.into();
        std::fs::write(path.join("profile.json"), serde_json::to_vec(&p).unwrap()).unwrap();
    }
    let select_first = command("select", json!({"path":first}), None, None).1;
    store
        .select_permission_preset_source(&select_first)
        .unwrap();
    let loaded = store
        .reload_permission_presets(&command("reload", json!({}), None, None).1)
        .unwrap();
    assert_eq!(loaded["added"], json!(["team:1"]));
    assert!(
        store
            .snapshot()
            .unwrap()
            .configuration("personal:1")
            .is_err()
    );
    store
        .select_permission_preset_source(&command("select", json!({"path":second}), None, None).1)
        .unwrap();
    // An invalid former source must not break the current source's reload.
    std::fs::write(first.join("profile.json"), "invalid").unwrap();
    let loaded = store
        .reload_permission_presets(&command("reload", json!({}), None, None).1)
        .unwrap();
    assert_eq!(loaded["added"], json!(["personal:1"]));
    // Replaying an older selection must not revert a newer selection.
    store
        .select_permission_preset_source(&select_first)
        .unwrap();
    assert_eq!(
        store.permission_preset_source().unwrap().path,
        std::fs::canonicalize(&second).unwrap()
    );
    drop(e);
    drop(store);

    let (store, _) = global(dir.path());
    assert_eq!(
        store.permission_preset_source().unwrap().path,
        std::fs::canonicalize(&second).unwrap()
    );
    assert!(!store.permission_preset_source().unwrap().is_builtin);
    std::fs::remove_dir_all(&second).unwrap();
    assert!(
        store
            .reload_permission_presets(&command("reload", json!({}), None, None).1)
            .is_err()
    );
    assert!(!store.permission_preset_source().unwrap().is_builtin);
    let restore = command("reload", json!({"builtin":true}), None, None).1;
    store.reload_permission_presets(&restore).unwrap();
    assert!(store.permission_preset_source().unwrap().is_builtin);
    let state = store.snapshot().unwrap();
    assert!(state.configuration("team:1").is_ok());
    assert!(state.configuration("personal:1").is_ok());
    assert!(
        state
            .governance
            .iter()
            .all(|r| r.r#type != "permission_assignment")
    );
    assert!(
        store
            .select_permission_preset_source(
                &command("select", json!({"path":"relative"}), None, None).1
            )
            .is_err()
    );
    assert!(
        store
            .select_permission_preset_source(
                &command(
                    "select",
                    json!({"path":former.join("invalid.json")}),
                    None,
                    None
                )
                .1
            )
            .is_err()
    );
    assert!(
        store
            .reload_permission_presets(&command("reload", json!({"builtin":"true"}), None, None).1)
            .is_err()
    );
}

#[test]
fn exact_target_bindings_cas_batch_atomicity_pinning_and_frozen_results() {
    let dir = tempfile::tempdir().unwrap();
    let (_, e) = global(dir.path());
    let h = e
        .store
        .snapshot()
        .unwrap()
        .governance
        .iter()
        .find(|r| r.r#type == "harness")
        .unwrap()
        .id
        .clone();
    bind_pair(&e, &h, "builtin-permission-harness-full-access:1");
    let first = lock(&e, &h);
    let old = compile_permission_profiles(
        &e.store.snapshot().unwrap(),
        &first,
        Some(ExecutionScope {
            work_type: WorkType::Discussion,
            writable_paths: vec![],
            read_only_paths: vec![],
        }),
    )
    .unwrap();
    let c = command(
        "permission.bindings.apply",
        json!({"bindings":[change(&e,PermissionTargetKind::Harness,&h,"builtin-permission-harness-read-only:1",1)]}),
        None,
        None,
    );
    let result = execute(&e, c.clone()).unwrap();
    assert_eq!(execute(&e, c).unwrap(), result);
    assert_eq!(
        apply(
            &e,
            vec![change(
                &e,
                PermissionTargetKind::Harness,
                &h,
                "builtin-permission-harness-full-access:1",
                1
            )]
        )
        .unwrap_err()
        .code,
        "revision_conflict"
    );
    let next = lock(&e, &h);
    assert_ne!(next, first);
    let pinned = resolve_permission_lock(
        &e.store.snapshot().unwrap(),
        &h,
        &ExecutionPermissionSelection {
            sandbox_ref: DEFAULT_PERMISSION_SANDBOX.into(),
            harness: PermissionSelection::Pinned {
                binding_revision_id: first.harness.binding_revision_id.clone(),
            },
            sandbox: PermissionSelection::FollowTarget,
        },
    )
    .unwrap();
    assert_eq!(pinned, first);
    assert_eq!(old.binding_lock.as_ref(), Some(&first));
    let failed = apply(
        &e,
        vec![
            change(
                &e,
                PermissionTargetKind::Harness,
                &h,
                "builtin-permission-harness-full-access:1",
                2,
            ),
            change(
                &e,
                PermissionTargetKind::Sandbox,
                DEFAULT_PERMISSION_SANDBOX,
                "builtin-permission-sandbox-full-access:1",
                999,
            ),
        ],
    );
    assert!(failed.is_err());
    assert_eq!(lock(&e, &h), next);
    schemas::SCHEMAS.assert_valid(
        "permission-binding.schema.json",
        &json!(next.harness.binding),
    );
    schemas::SCHEMAS.assert_valid("rpc.schema.json#/$defs/ExecutionPermissions", &json!(old));
    let other=execute(&e,command("harness.create",json!({"name":"same upstream different definition","model_providers":["builtin-model-provider-openai:1"],"data":{"provider":"codex","runtime_version":devdesk_adapters::runtime_preparation::default_version(HarnessProvider::Codex),"capabilities":[]}}),None,None)).unwrap();
    assert!(
        resolve_bound_permission(
            &e.store.snapshot().unwrap(),
            permission_target(
                &e.store.snapshot().unwrap(),
                PermissionTargetKind::Harness,
                other["id"].as_str().unwrap()
            )
            .unwrap(),
            &PermissionSelection::Pinned {
                binding_revision_id: first.harness.binding_revision_id
            }
        )
        .is_err()
    );
}

#[test]
fn compiler_rejects_unenforceable_rules_and_checks_the_real_scope() {
    let dir = tempfile::tempdir().unwrap();
    let (_, e) = global(dir.path());
    let s = e.store.snapshot().unwrap();
    let h = s
        .governance
        .iter()
        .find(|r| r.r#type == "harness" && r.data["provider"] == "codex")
        .unwrap();
    let target = permission_target(&s, PermissionTargetKind::Harness, &h.id).unwrap();
    let mut p = profile(&e, "builtin-permission-harness-read-only:1");
    p.policy.tools = Some(ToolPermission::Restricted {
        allow: vec!["bash".into()],
        deny: vec![],
    });
    assert_eq!(
        preflight_profile(&s, &target, &p).unwrap_err().code,
        "permission_unsupported"
    );
    p.policy.tools = Some(ToolPermission::AllAvailable);
    p.policy.approval = Some(ApprovalPolicy::OnRequest);
    assert_eq!(
        preflight_profile(&s, &target, &p).unwrap_err().code,
        "permission_unsupported"
    );
    p.policy.approval = Some(ApprovalPolicy::Never);
    p.policy.network = NetworkPermission::Allowlist {
        destinations: vec!["example.com".into()],
    };
    assert_eq!(
        preflight_profile(&s, &target, &p).unwrap_err().code,
        "permission_unsupported"
    );
    bind_pair(&e, &h.id, "builtin-permission-harness-read-only:1");
    assert!(
        compile_permission_profiles(
            &e.store.snapshot().unwrap(),
            &lock(&e, &h.id),
            Some(ExecutionScope {
                work_type: WorkType::Workflow,
                writable_paths: vec!["src".into()],
                read_only_paths: vec![]
            })
        )
        .is_err()
    );
}

#[test]
fn defaults_use_v2_directly_and_restart_does_not_create_compatibility_records() {
    let dir = tempfile::tempdir().unwrap();
    let (store, e) = global(dir.path());
    let first = store.snapshot().unwrap();
    for r in first.governance.iter().filter(|r| r.r#type == "agent") {
        assert!(r.data.get("permissions").is_none());
        assert!(r.data.get("approval").is_none());
        assert!(
            r.data.get("permission_selection").is_none() && permission_selection(&first).is_ok()
        );
    }
    assert!(
        !first
            .governance
            .iter()
            .any(|r| r.r#type.starts_with("permission_migration"))
    );
    drop(e);
    drop(store);
    let (store, _) = global(dir.path());
    assert_eq!(
        json!(first.governance),
        json!(store.snapshot().unwrap().governance)
    );
}

fn prepared_discussion(e: &Engine) -> (Id, Id, RuntimeBinding) {
    let project = execute(
        e,
        command(
            "project.create",
            json!({"title":"permission switch"}),
            None,
            None,
        ),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let binding = RuntimeBinding {
        model_connection: None,
        model_profile_digest: None,
        agent_configuration: None,
        model_parameters: Default::default(),
        permissions: None,
        installation: RuntimeInstallation {
            recipe_digest: None,
            id: "fixture-runtime".into(),
            key: RuntimeKey {
                provider: HarnessProvider::Codex,
                version: "fixture".into(),
                os: "linux".into(),
                arch: "aarch64".into(),
            },
            artifact: RuntimeArtifact::DockerImage {
                image_id: format!("sha256:{}", "a".repeat(64)),
            },
            source: "test".into(),
            license: "test".into(),
        },
        adapter_version: "fixture".into(),
        model: "fixture".into(),
        tool_policy: ToolPolicy::ReadOnly,
        writable_paths: vec![],
        read_only_paths: vec![],
        work_type: WorkType::Discussion,
        configuration_digest: "a".repeat(64),
        credential_ref: None,
    };
    let revision = e.store.snapshot().unwrap().projects[0].revision;
    let sid = execute(
        e,
        command(
            "session.open",
            json!({"agent":"builtin-agent-sol-high-analyst:4","binding":binding}),
            Some(&project),
            Some(revision),
        ),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (project, sid, binding)
}

#[test]
fn successor_switch_accepts_input_once_preserves_history_and_recovers_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::new(
        Arc::new(SqliteStore::open(dir.path()).unwrap()),
        Arc::new(FileArtifacts::new(dir.path().into())),
        runtime::adapters(),
    )
    .with_tool_executor(Arc::new(
        devdesk_adapters::integrations::NativeTools::new(dir.path()).unwrap(),
    ));
    e.bootstrap().unwrap();
    let (_, sid, mut next) = prepared_discussion(&e);
    next.configuration_digest = "b".repeat(64);
    let c = command(
        "conversation.send",
        json!({"input":"Keep the context","client_message_id":"switch-message"}),
        Some(&sid),
        Some(1),
    )
    .1;
    let result = e
        .send_with_permission_binding(c.clone(), next.clone())
        .unwrap();
    assert_ne!(result["effective_session_id"], sid);
    let s = e.store.snapshot().unwrap();
    assert_eq!(s.turns.len(), 1);
    assert_eq!(s.sessions.len(), 2);
    assert_eq!(
        s.sessions
            .iter()
            .find(|v| v.id == sid)
            .unwrap()
            .runtime_binding
            .as_ref()
            .unwrap()
            .configuration_digest,
        "a".repeat(64)
    );
    let successor = s
        .sessions
        .iter()
        .find(|v| {
            Some(&v.id)
                == result["effective_session_id"]
                    .as_str()
                    .map(|s| s.to_owned())
                    .as_ref()
        })
        .unwrap();
    assert_eq!(successor.predecessor.as_deref(), Some(sid.as_str()));
    assert!(successor.snapshot_id.is_some());
    assert!(successor.external_session.is_none());
    assert_eq!(
        e.send_with_permission_binding(c.clone(), next.clone())
            .unwrap(),
        result
    );
    drop(e);
    let restarted = Engine::new(
        Arc::new(SqliteStore::open(dir.path()).unwrap()),
        Arc::new(FileArtifacts::new(dir.path().into())),
        runtime::adapters(),
    )
    .with_tool_executor(Arc::new(
        devdesk_adapters::integrations::NativeTools::new(dir.path()).unwrap(),
    ));
    restarted.bootstrap().unwrap();
    assert_eq!(
        restarted.send_with_permission_binding(c, next).unwrap(),
        result
    );
    assert_eq!(restarted.store.snapshot().unwrap().turns.len(), 1);
}

#[test]
fn queued_input_prevents_permission_switch_and_cannot_be_reinterpreted() {
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::new(
        Arc::new(SqliteStore::open(dir.path()).unwrap()),
        Arc::new(FileArtifacts::new(dir.path().into())),
        runtime::adapters(),
    )
    .with_tool_executor(Arc::new(
        devdesk_adapters::integrations::NativeTools::new(dir.path()).unwrap(),
    ));
    e.bootstrap().unwrap();
    let (_, sid, mut next) = prepared_discussion(&e);
    execute(
        &e,
        command(
            "conversation.send",
            json!({"input":"accepted under old permissions","client_message_id":"old"}),
            Some(&sid),
            Some(1),
        ),
    )
    .unwrap();
    let s = e.store.snapshot().unwrap();
    let revision = s.sessions.iter().find(|v| v.id == sid).unwrap().revision;
    next.configuration_digest = "b".repeat(64);
    let error = e
        .send_with_permission_binding(
            command(
                "conversation.send",
                json!({"input":"new","client_message_id":"new"}),
                Some(&sid),
                Some(revision),
            )
            .1,
            next,
        )
        .unwrap_err();
    assert_eq!(error.code, "permission_switch_pending");
    assert_eq!(e.store.snapshot().unwrap().sessions.len(), 1);
    assert_eq!(e.store.snapshot().unwrap().turns.len(), 1);
}

#[test]
fn frozen_feature_pins_followed_bindings_before_the_target_default_changes() {
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::new(
        Arc::new(SqliteStore::open(dir.path()).unwrap()),
        Arc::new(FileArtifacts::new(dir.path().into())),
        runtime::adapters(),
    )
    .with_tool_executor(Arc::new(
        devdesk_adapters::integrations::NativeTools::new(dir.path()).unwrap(),
    ));
    e.bootstrap().unwrap();
    let project = execute(
        &e,
        command(
            "project.create",
            json!({"title":"Frozen permissions"}),
            None,
            None,
        ),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    bind_pair(
        &e,
        DEFAULT_HARNESS_ID,
        "builtin-permission-harness-full-access:1",
    );
    let defaults = e.configuration_defaults(Some(&project)).unwrap();
    let mut agents = defaults["role_agent_bindings"].clone();
    let role_key = e
        .store
        .snapshot()
        .unwrap()
        .configuration(&default_role_id("analyst"))
        .unwrap()
        .id
        .clone();
    let source = agents[&role_key].as_str().unwrap();
    let record = e
        .store
        .snapshot()
        .unwrap()
        .configuration(source)
        .unwrap()
        .clone();
    let mut payload = json!({"name":"Followed analyst","scope":"global","data":record.data});
    for (key, value) in &record.refs {
        payload[key] = value.clone();
    }
    let a = execute(&e, command("agent.create", payload, None, None)).unwrap();
    agents[&role_key] = a["id"].clone();
    let revision = e.store.snapshot().unwrap().projects[0].revision;
    execute(
        &e,
        command(
            "project.configuration.update",
            json!({"workflow_ref":defaults["workflow_ref"],"role_agent_bindings":agents}),
            Some(&project),
            Some(revision),
        ),
    )
    .unwrap();
    let feature = execute(
        &e,
        command(
            "feature.create",
            json!({"project_id":project,"title":"Frozen","requirement":"Keep permissions fixed"}),
            None,
            None,
        ),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let frozen = frozen_configured_permission(
        &e.store.snapshot().unwrap(),
        &feature,
        a["id"].as_str().unwrap(),
    )
    .unwrap();
    apply(
        &e,
        vec![change(
            &e,
            PermissionTargetKind::Harness,
            DEFAULT_HARNESS_ID,
            "builtin-permission-harness-read-only:1",
            1,
        )],
    )
    .unwrap();
    assert_ne!(frozen, lock(&e, DEFAULT_HARNESS_ID));
    assert_eq!(
        frozen_configured_permission(
            &e.store.snapshot().unwrap(),
            &feature,
            a["id"].as_str().unwrap()
        )
        .unwrap(),
        frozen
    );
}

#[test]
fn uncertain_stop_retains_the_intent_and_retry_uses_the_original_binding() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct StopOnce(AtomicUsize);
    impl ports::Harness for StopOnce {
        fn stop_session(&self, _: &Session, e: &RuntimeExecution) -> Result<ports::WorkerStop> {
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(err("needs_reconciliation", "lost stop receipt"));
            }
            Ok(ports::WorkerStop {
                operation_id: e.operation_id.clone(),
                writer_revoked: true,
                mode: "fixture-stop".into(),
                requires_successor: true,
            })
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let mut adapters = runtime::adapters();
    adapters.harness = Arc::new(StopOnce(AtomicUsize::new(0)));
    let e = Engine::new(
        Arc::new(SqliteStore::open(dir.path()).unwrap()),
        Arc::new(FileArtifacts::new(dir.path().into())),
        adapters,
    );
    e.bootstrap().unwrap();
    let (_, sid, mut next) = prepared_discussion(&e);
    next.configuration_digest = "b".repeat(64);
    e.store
        .transact(None, &mut |s| {
            s.executions.push(RuntimeExecution {
                permissions: None,
                kind: RuntimeExecutionKind::Harness,
                id: id(),
                session_id: sid.clone(),

                operation_id: id(),
                container_id: None,
                image_digest: format!("sha256:{}", "a".repeat(64)),
                status: Status::Prepared,
                writer_revoked: false,
                started: None,
                finished: None,
                protocol_artifact: None,
            });
            Ok(Value::Null)
        })
        .unwrap();
    let c = command(
        "conversation.send",
        json!({"input":"one original input","client_message_id":"fault"}),
        Some(&sid),
        Some(1),
    )
    .1;
    assert_eq!(
        e.send_with_permission_binding(c.clone(), next.clone())
            .unwrap_err()
            .code,
        "needs_reconciliation"
    );
    let s = e.store.snapshot().unwrap();
    assert!(s.turns.is_empty());
    assert_eq!(s.sessions.len(), 1);
    assert_eq!(
        s.configuration(&permission_switch_id(&c.command_id))
            .unwrap()
            .data["status"],
        "needs_reconciliation"
    );
    next.configuration_digest = "c".repeat(64);
    let result = e.send_with_permission_binding(c, next).unwrap();
    let s = e.store.snapshot().unwrap();
    assert_eq!(s.turns.len(), 1);
    assert!(s.executions.iter().all(|e| e.writer_revoked));
    assert_eq!(
        s.sessions
            .iter()
            .find(|v| v.id == result["effective_session_id"].as_str().unwrap())
            .unwrap()
            .runtime_binding
            .as_ref()
            .unwrap()
            .configuration_digest,
        "b".repeat(64)
    );
}

#[test]
#[ignore = "requires actual Docker and pinned Codex/DSH images; no model calls"]
fn v2_binding_compiles_into_real_codex_and_dsh_file_and_network_boundaries() {
    use devdesk_adapters::docker::{ContainerPurpose, ContainerSpec, DockerSandbox};
    use std::{
        fs, thread,
        time::{Duration, Instant},
    };
    for provider in ["codex", "dsh"] {
        let root = tempfile::tempdir().unwrap();
        let (_, e) = global(root.path());
        let harness=execute(&e,command("harness.create",json!({"name":provider,"model_providers":[],"data":{"provider":provider,"runtime_version":if provider=="codex" {devdesk_adapters::runtime_preparation::default_version(HarnessProvider::Codex)}else{devdesk_adapters::runtime_preparation::default_version(HarnessProvider::Dsh)},"capabilities":[]}}),None,None)).unwrap();
        let h = harness["id"].as_str().unwrap();
        let p=execute(&e,command("permission.profile.create",json!({"identity":format!("native-{provider}"),"name":"Native v2 restrictions","target_kind":"harness","policy":{
            "filesystem":{"mode":"scoped","rules":[{"path":"/","access":"read"},{"path":"/work","access":"read_write"},{"path":"/work/tests","access":"read"}]},
            "network":{"mode":"disabled"},"tools":if provider=="dsh" {json!({"mode":"restricted","allow":["bash","ask_user_question"],"deny":[]})}else{json!({"mode":"all_available"})},"approval":"never"
        }}),None,None)).unwrap();
        bind_pair(&e, h, p["id"].as_str().unwrap());
        let plan = compile_permission_profiles(
            &e.store.snapshot().unwrap(),
            &lock(&e, h),
            Some(ExecutionScope {
                work_type: WorkType::Workflow,
                writable_paths: vec![".".into()],
                read_only_paths: vec!["tests".into()],
            }),
        )
        .unwrap();
        assert!(plan.binding_lock.is_some());
        let work = root.path().join("work");
        let state = root.path().join("state");
        fs::create_dir_all(work.join("tests")).unwrap();
        fs::create_dir_all(state.join("codex")).unwrap();
        let js = "const fs=require('fs');fs.writeFileSync('/work/proof','v2-native-ok');try{fs.writeFileSync('/work/tests/prohibited','bad');process.exit(3)}catch{}let denied=false;try{if(Object.values(require('os').networkInterfaces()).flat().some(i=>!i.internal))process.exit(4);denied=true;}catch(e){if(e.info?.errno===1)denied=true;else throw e;}if(!denied)process.exit(5);console.log('V2_FILE_AND_NETWORK_BOUNDARY_OK')";
        let mut profile_file = None;
        let mut runner_file = None;
        let mut credential_env_file = None;
        let native_command = if provider == "codex" {
            fs::write(
                state.join("codex/config.toml"),
                permissions::codex_config(&plan),
            )
            .unwrap();
            vec![
                "codex".into(),
                "sandbox".into(),
                "--".into(),
                "node".into(),
                "-e".into(),
                js.into(),
            ]
        } else {
            let shell = format!("node -e {}", serde_json::to_string(js).unwrap());
            let module = format!(
                "import {{writeFileSync}} from 'node:fs';export const name='v2-probe';export const inject=['shell'];export async function apply(ctx){{try{{const ex=await ctx.shell.execute(ctx.shell.resolve({{command:{},workdir:'/work',timeoutMs:8000}}));const result=await ex.result();writeFileSync('/state/probe-result.json',JSON.stringify(result));process.exit(0);}}catch(e){{writeFileSync('/state/probe-result.json',JSON.stringify({{error:String(e)}}));process.exit(1);}}}}",
                serde_json::to_string(&shell).unwrap()
            );
            fs::write(state.join("probe.mjs"), module).unwrap();
            let mut patch =
                permissions::dsh_profile(&plan, "Credential-free v2 boundary probe", "/work")
                    .unwrap();
            patch.push_str("- insert:\n  - id: v2-probe\n    name: /state/probe.mjs\n");
            let profile = root.path().join("profile.yaml");
            fs::write(&profile, patch).unwrap();
            profile_file = Some(profile);
            let runner = root.path().join("runner.sh");
            fs::write(
                &runner,
                include_bytes!("../../../permissions/backends/dsh/bwrap-runner.sh"),
            )
            .unwrap();
            runner_file = Some(runner);
            let env = root.path().join("fake.env");
            fs::write(&env, "DEEPSEEK_API_KEY=FAKE_PERMISSION_CANARY_NOT_A_KEY\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&env, fs::Permissions::from_mode(0o600)).unwrap();
            }
            credential_env_file = Some(env);
            vec![
                "dsh".into(),
                "--profile".into(),
                "sdk".into(),
                "--patch".into(),
                "/run/devdesk/profile".into(),
            ]
        };
        let reference = if provider == "codex" {
            "devdesk/codex:0.167.0"
        } else {
            "devdesk/dsh:0.2.2-rc.1"
        };
        let output = std::process::Command::new("docker")
            .args(["image", "inspect", reference, "--format", "{{.Id}}"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let spec = ContainerSpec {
            host_gateway: false,
            permissions: Some(plan),
            purpose: ContainerPurpose::Harness,
            project_id: id(),
            session_id: id(),
            operation_id: id(),
            image_id: String::from_utf8(output.stdout).unwrap().trim().into(),
            worktree: work.clone(),
            state: state.clone(),
            writable: true,
            writable_subdirectories: vec![],
            read_only_subdirectories: vec!["tests".into()],
            network: true,
            command: native_command,
            nested_sandbox: true,
            credential_env_file,
            profile_file,
            runner_file,
            skill_directory: None,
            sensitive_output: false,
            profile_target: None,
        };
        let docker = DockerSandbox::new(root.path()).unwrap();
        docker.create(&spec).unwrap();
        docker.start(&spec.operation_id).unwrap();
        let deadline = Instant::now() + Duration::from_secs(25);
        while docker
            .inspect(&spec.operation_id)
            .unwrap()
            .is_some_and(|r| r.running)
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(100));
        }
        let receipt = docker.inspect(&spec.operation_id).unwrap().unwrap();
        let output = std::process::Command::new("docker")
            .args(["logs", &receipt.name])
            .output()
            .unwrap();
        let logs = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let exit = receipt.exit_code;
        docker.stop(&spec.operation_id).unwrap();
        docker.remove(&spec.operation_id).unwrap();
        assert_eq!(exit, 0, "{provider}: {logs}");
        if provider == "dsh" {
            let result: Value =
                serde_json::from_slice(&fs::read(state.join("probe-result.json")).unwrap())
                    .unwrap();
            assert_eq!(result["exitCode"], 0, "{result} {logs}");
        }
        assert_eq!(
            fs::read_to_string(work.join("proof")).unwrap(),
            "v2-native-ok"
        );
        assert!(!work.join("tests/prohibited").exists());
    }
}

/// Real subprocess exits leave SQLite to recover its own committed/rolled-back transactions.
#[test]
fn permission_switch_survives_process_exit_at_durable_boundaries() {
    use std::path::{Path, PathBuf};
    struct CrashStore {
        inner: Arc<SqliteStore>,
        boundary: String,
    }
    impl EngineeringStore for CrashStore {
        fn snapshot(&self) -> Result<EngineeringState> {
            self.inner.snapshot()
        }
        fn root(&self) -> String {
            self.inner.root()
        }
        fn replay(&self, id: &str, fingerprint: &str) -> Result<Option<Value>> {
            self.inner.replay(id, fingerprint)
        }
        fn transact(
            &self,
            key: Option<(&str, &str)>,
            action: &mut dyn FnMut(&mut EngineeringState) -> Result<Value>,
        ) -> Result<Value> {
            let result = self.inner.transact(key, &mut |state| {
                let value = action(state)?;
                if self.boundary == "before_successor_commit"
                    && state
                        .governance
                        .iter()
                        .any(|r| r.r#type == "permission_switch" && r.data["status"] == "adopted")
                {
                    std::process::exit(86);
                }
                Ok(value)
            })?;
            let state = self.inner.snapshot()?;
            if let Some(intent) = state
                .governance
                .iter()
                .find(|r| r.r#type == "permission_switch")
            {
                let stopped = intent.data["stop_receipts"]
                    .as_object()
                    .is_some_and(|v| !v.is_empty());
                if (self.boundary == "intent_committed" && intent.data["status"] == "prepared")
                    || (self.boundary == "stop_committed" && stopped)
                    || (self.boundary == "input_committed" && intent.data["status"] == "adopted")
                {
                    std::process::exit(86);
                }
            }
            Ok(result)
        }
    }
    struct Stopper {
        root: PathBuf,
        crash: bool,
    }
    impl ports::Harness for Stopper {
        fn stop_session(
            &self,
            _: &Session,
            execution: &RuntimeExecution,
        ) -> Result<ports::WorkerStop> {
            // The same operation can be reconciled after its receipt was lost.
            std::fs::write(self.root.join("stopped-operation"), &execution.operation_id).unwrap();
            if self.crash {
                std::process::exit(86);
            }
            Ok(ports::WorkerStop {
                operation_id: execution.operation_id.clone(),
                writer_revoked: true,
                mode: "fixture-stop".into(),
                requires_successor: true,
            })
        }
    }
    let adapters = |root: &Path, crash: bool| {
        let mut adapters = runtime::adapters();
        adapters.harness = Arc::new(Stopper {
            root: root.into(),
            crash,
        });
        adapters
    };
    if let Ok(root) = std::env::var("DEVDESK_SWITCH_CRASH_ROOT") {
        let root = PathBuf::from(root);
        let boundary = std::env::var("DEVDESK_SWITCH_CRASH_BOUNDARY").unwrap();
        let value: Value =
            serde_json::from_slice(&std::fs::read(root.join("request.json")).unwrap()).unwrap();
        let store = Arc::new(CrashStore {
            inner: Arc::new(SqliteStore::open(&root).unwrap()),
            boundary: boundary.clone(),
        });
        let engine = Engine::new(
            store,
            Arc::new(FileArtifacts::new(root.clone())),
            adapters(&root, boundary == "stop_receipt_lost"),
        );
        engine
            .send_with_permission_binding(
                serde_json::from_value(value["command"].clone()).unwrap(),
                serde_json::from_value(value["binding"].clone()).unwrap(),
            )
            .unwrap();
        panic!("crash boundary was not reached");
    }
    for boundary in [
        "intent_committed",
        "stop_receipt_lost",
        "stop_committed",
        "before_successor_commit",
        "input_committed",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let engine = Engine::new(
            Arc::new(SqliteStore::open(root).unwrap()),
            Arc::new(FileArtifacts::new(root.into())),
            adapters(root, false),
        );
        engine.bootstrap().unwrap();
        let (_, sid, mut binding) = prepared_discussion(&engine);
        binding.configuration_digest = "b".repeat(64);
        engine
            .store
            .transact(None, &mut |state| {
                state.executions.push(RuntimeExecution {
                    permissions: None,
                    kind: RuntimeExecutionKind::Harness,
                    id: id(),
                    session_id: sid.clone(),

                    operation_id: id(),
                    container_id: None,
                    image_digest: format!("sha256:{}", "a".repeat(64)),
                    status: Status::Prepared,
                    writer_revoked: false,
                    started: None,
                    finished: None,
                    protocol_artifact: None,
                });
                Ok(Value::Null)
            })
            .unwrap();
        let request = command(
            "conversation.send",
            json!({"input":"one crash-safe input","client_message_id":"crash-message"}),
            Some(&sid),
            Some(1),
        )
        .1;
        std::fs::write(
            root.join("request.json"),
            serde_json::to_vec(&json!({"command":request,"binding":binding})).unwrap(),
        )
        .unwrap();
        drop(engine);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "permission_switch_survives_process_exit_at_durable_boundaries",
                "--nocapture",
            ])
            .env("DEVDESK_SWITCH_CRASH_ROOT", root)
            .env("DEVDESK_SWITCH_CRASH_BOUNDARY", boundary)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(86),
            "{boundary}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let engine = Engine::new(
            Arc::new(SqliteStore::open(root).unwrap()),
            Arc::new(FileArtifacts::new(root.into())),
            adapters(root, false),
        );
        engine.bootstrap().unwrap();
        // A new default cannot replace the original durable switching intent.
        binding.configuration_digest = "c".repeat(64);
        let result = engine
            .send_with_permission_binding(request.clone(), binding.clone())
            .unwrap();
        assert_eq!(
            engine
                .send_with_permission_binding(request, binding)
                .unwrap(),
            result
        );
        let state = engine.store.snapshot().unwrap();
        assert_eq!(state.sessions.len(), 2, "{boundary}");
        assert_eq!(state.turns.len(), 1, "{boundary}");
        assert!(
            state.executions.iter().all(|e| e.writer_revoked),
            "{boundary}"
        );
        let successor = state
            .sessions
            .iter()
            .find(|s| s.id == result["effective_session_id"].as_str().unwrap())
            .unwrap();
        assert_eq!(successor.predecessor.as_deref(), Some(sid.as_str()));
        assert_eq!(
            successor
                .runtime_binding
                .as_ref()
                .unwrap()
                .configuration_digest,
            "b".repeat(64)
        );
        assert_eq!(
            state
                .sessions
                .iter()
                .find(|s| s.id == sid)
                .unwrap()
                .runtime_binding
                .as_ref()
                .unwrap()
                .configuration_digest,
            "a".repeat(64)
        );
    }
}

fn frozen_configured_permission(
    state: &EngineeringState,
    feature: &str,
    agent: &str,
) -> Result<VersionedPermissionLock> {
    let instance = state
        .governance
        .iter()
        .find(|r| r.r#type == "workflow_instance" && r.data["owner_id"] == feature)
        .ok_or_else(|| err("not_found", "流程不存在"))?;
    let plan: ConfiguredWorkflowPlan =
        serde_json::from_value(instance.data["plan"].clone()).unwrap();
    plan.permission_locks
        .get(agent)
        .cloned()
        .ok_or_else(|| err("not_found", "权限未冻结"))
}

#[test]
fn profile_revisions_preserve_the_source_owner_scope() {
    let directory = tempfile::tempdir().unwrap();
    let (_, engine) = global(directory.path());
    let mut source = engine
        .store
        .snapshot()
        .unwrap()
        .configuration("builtin-permission-harness-read-only:1")
        .unwrap()
        .source();
    source.identity = "project-owned-policy".into();
    source.scope = Scope::project(id());
    engine
        .execute(
            "permission.profile.import",
            Command {
                command_id: id(),
                target: None,
                expected_revision: None,
                payload: json!({"profiles":[source]}),
            },
        )
        .unwrap();
    let revised = engine.execute("permission.profile.revise", Command { command_id:id(), target:Some("project-owned-policy:1".into()), expected_revision:Some(1), payload:json!({"name":"Same owner revised policy","target_kind":source.data["target_kind"],"policy":source.data["policy"]}) }).unwrap();
    assert_eq!(revised["scope"], json!(source.scope));
    source.revision = 3;
    source.scope = Scope::global();
    assert!(
        engine
            .execute(
                "permission.profile.import",
                Command {
                    command_id: id(),
                    target: None,
                    expected_revision: None,
                    payload: json!({"profiles":[source]})
                }
            )
            .is_err()
    );
}
