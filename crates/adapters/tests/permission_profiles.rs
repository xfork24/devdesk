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
    );
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
    e.execute(&c.0, c.1)
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
        &AgentPermissionSelection {
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
                "permission-sandbox-full-access-v2-r1",
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
        .decode()
        .unwrap()
}

#[test]
fn strict_profiles_share_canonical_hashes_with_files_and_preserve_v1() {
    let dir = tempfile::tempdir().unwrap();
    let (store, e) = global(dir.path());
    let before = store.snapshot().unwrap().permissions;
    store
        .reload_permission_presets(&command("reload", json!({}), None, None).1)
        .unwrap();
    assert_eq!(before, store.snapshot().unwrap().permissions);
    let p = profile(&e, "permission-harness-read-only-v2-r1");
    let r = store
        .snapshot()
        .unwrap()
        .configuration(&p.id)
        .unwrap()
        .clone();
    assert_eq!(r.content_hash, permission_content_hash(&p));
    schemas::SCHEMAS.assert_valid("permission-profile.schema.json", &json!(p));
    let mut bad = json!(p);
    bad["policy"]["tools"] = Value::Null;
    assert!(serde_json::from_value::<PermissionProfile>(bad.clone()).is_err());
    assert!(
        !schemas::SCHEMAS
            .validator("permission-profile.schema.json")
            .is_valid(&bad)
    );
    bad = json!(p);
    bad["policy"]["typo"] = json!(true);
    assert!(serde_json::from_value::<PermissionProfile>(bad).is_err());
    let mut bad = p.clone();
    bad.target_kind = PermissionTargetKind::Sandbox;
    assert_eq!(bad.normalize().unwrap_err().code, "permission_inapplicable");
}

#[test]
fn import_is_atomic_and_revision_export_round_trips_in_a_fresh_store() {
    let dir = tempfile::tempdir().unwrap();
    let (_, e) = global(dir.path());
    let payload = json!({"identity":"custom-review","name":"Custom review","target_kind":"harness","policy":profile(&e,"permission-harness-read-only-v2-r1").policy});
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
    let mut valid = first.clone();
    valid["id"] = json!(id());
    valid["identity"] = json!("atomic-new");
    let mut conflict = first.clone();
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
            .configuration(valid["id"].as_str().unwrap())
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
    let root = dir.path().join("permissions/presets");
    std::fs::create_dir_all(&root).unwrap();
    let mut p = profile(&e, "permission-harness-read-only-v2-r1");
    p.id = "runtime-added".into();
    p.identity = "runtime-added".into();
    std::fs::write(root.join("new.json"), serde_json::to_vec(&p).unwrap()).unwrap();
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
            .all(|r| r.kind != "permission_assignment")
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
fn exact_target_bindings_cas_batch_atomicity_pinning_and_frozen_results() {
    let dir = tempfile::tempdir().unwrap();
    let (_, e) = global(dir.path());
    let h = e
        .store
        .snapshot()
        .unwrap()
        .governance
        .iter()
        .find(|r| r.kind == "harness")
        .unwrap()
        .id
        .clone();
    bind_pair(&e, &h, "permission-harness-full-access-v2-r1");
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
        json!({"bindings":[change(&e,PermissionTargetKind::Harness,&h,"permission-harness-read-only-v2-r1",1)]}),
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
                "permission-harness-full-access-v2-r1",
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
        &AgentPermissionSelection {
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
                "permission-harness-full-access-v2-r1",
                2,
            ),
            change(
                &e,
                PermissionTargetKind::Sandbox,
                DEFAULT_PERMISSION_SANDBOX,
                "permission-sandbox-full-access-v2-r1",
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
    let other=execute(&e,command("harness.create",json!({"name":"same upstream different definition","data":{"provider":"codex","runtime_version":"0.167.0","capabilities":[],"model_providers":["openai"]}}),None,None)).unwrap();
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
        .find(|r| r.kind == "harness" && r.data["provider"] == "codex")
        .unwrap();
    let target = permission_target(&s, PermissionTargetKind::Harness, &h.id).unwrap();
    let mut p = profile(&e, "permission-harness-read-only-v2-r1");
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
    bind_pair(&e, &h.id, "permission-harness-read-only-v2-r1");
    assert!(
        compile_permission_profiles(
            &e.store.snapshot().unwrap(),
            &lock(&e, &h.id),
            Some(ExecutionScope {
                work_type: WorkType::Implementation,
                writable_paths: vec!["src".into()],
                read_only_paths: vec![]
            })
        )
        .is_err()
    );
}

#[test]
fn compatibility_conversion_keeps_immutable_agents_and_is_restart_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let (store, e) = global(dir.path());
    let before = store.snapshot().unwrap();
    store.migrate_permission_selections().unwrap();
    let first = store.snapshot().unwrap();
    assert!(
        first
            .governance
            .iter()
            .any(|r| r.kind == "permission_migration" && r.data["status"] == "converted_fixed")
    );
    for r in before.governance.iter().filter(|r| r.kind == "agent") {
        assert_eq!(json!(r), json!(first.configuration(&r.id).unwrap()));
    }
    store.migrate_permission_selections().unwrap();
    assert_eq!(
        json!(first.governance),
        json!(store.snapshot().unwrap().governance)
    );
    drop(e);
    drop(store);
    let (reopened, _) = global(dir.path());
    reopened.migrate_permission_selections().unwrap();
    assert_eq!(
        json!(first.governance),
        json!(reopened.snapshot().unwrap().governance)
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
        agent_configuration: None,
        permissions: None,
        installation: RuntimeInstallation {
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
            json!({"agent":"builtin-agent-analyst-v1","binding":binding}),
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
    );
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
    );
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
    );
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
    );
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
        "permission-harness-full-access-v2-r1",
    );
    let defaults = e.configuration_defaults(Some(&project)).unwrap();
    let mut agents = defaults["role_agent_bindings"].clone();
    let role_key = e
        .store
        .snapshot()
        .unwrap()
        .configuration(&default_role_id("analyst"))
        .unwrap()
        .definition_id
        .clone();
    let source = agents[&role_key].as_str().unwrap();
    let mut data = e
        .store
        .snapshot()
        .unwrap()
        .configuration(source)
        .unwrap()
        .data
        .clone();
    data["permission_selection"] = json!(AgentPermissionSelection {
        sandbox_ref: DEFAULT_PERMISSION_SANDBOX.into(),
        harness: PermissionSelection::FollowTarget,
        sandbox: PermissionSelection::FollowTarget
    });
    let a = execute(
        &e,
        command(
            "agent.create",
            json!({"name":"Followed analyst","data":data}),
            None,
            None,
        ),
    )
    .unwrap();
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
    let frozen = frozen_permission_lock(
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
            "permission-harness-read-only-v2-r1",
            1,
        )],
    )
    .unwrap();
    assert_ne!(frozen, lock(&e, DEFAULT_HARNESS_ID));
    assert_eq!(
        frozen_permission_lock(
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
                run_id: None,
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
        let harness=execute(&e,command("harness.create",json!({"name":provider,"data":{"provider":provider,"runtime_version":if provider=="codex" {"0.167.0"}else{"0.2.2-rc.1"},"model_providers":[],"capabilities":[]}}),None,None)).unwrap();
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
                work_type: WorkType::Implementation,
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
