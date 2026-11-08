use devdesk_adapters::{GlobalStore, SqliteStore, permissions};
use devdesk_core::{Command, EngineeringStore, seed_configuration};
use devdesk_domain::*;
use serde_json::json;
fn definitions() -> Vec<Permission> {
    let mut state = EngineeringState::default();
    seed_configuration(&mut state);
    state.permissions
}
fn scope() -> ExecutionScope {
    ExecutionScope {
        work_type: WorkType::Discussion,
        writable_paths: vec![],
        read_only_paths: vec![],
    }
}
#[test]
fn native_defaults_and_explicit_network_restrictions_have_real_enforcement_owners() {
    for provider in [HarnessProvider::Codex, HarnessProvider::Dsh] {
        let p = permissions::resolve_execution_permissions(
            &definitions(),
            &PermissionConfiguration::defaults(false),
            provider,
            scope(),
            None,
            None,
            None,
        )
        .unwrap();
        assert!(!p.isolation.nested_sandbox);
        assert_eq!(p.isolation.credential_boundary, "container-trust");
        assert!(p.isolation.executable_tmp);
        assert_eq!(p.approval, ApprovalPolicy::Never);
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../permissions/backends/manifest.json"))
                .unwrap();
        for backend in [
            &p.enforcement.harness_backend,
            &p.enforcement.sandbox_backend,
        ] {
            assert!(
                manifest["backends"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|b| b["id"] == *backend)
            );
        }
        assert_eq!(
            permissions::codex_sandbox_policy(&p)["type"],
            "externalSandbox"
        );
    }
    let mut o = PermissionOverrides::default();
    o.harness.overrides.network = Some(NetworkPermission::Disabled);
    assert_eq!(
        permissions::resolve_execution_permissions(
            &definitions(),
            &PermissionConfiguration::defaults(false),
            HarnessProvider::Dsh,
            scope(),
            None,
            None,
            Some(&o)
        )
        .unwrap_err()
        .code,
        "permission_unsupported"
    );
    o.harness.overrides.filesystem = Some(FilesystemPermission::ReadOnly);
    o.harness.overrides.tools = Some(ToolPermission::Restricted {
        allow: vec!["bash".into(), "ask_user_question".into()],
        deny: vec![],
    });
    let p = permissions::resolve_execution_permissions(
        &definitions(),
        &PermissionConfiguration::defaults(false),
        HarnessProvider::Dsh,
        scope(),
        None,
        None,
        Some(&o),
    )
    .unwrap();
    assert!(p.isolation.nested_sandbox);
    o.harness.overrides.network = Some(NetworkPermission::Allowlist {
        destinations: vec!["example.com".into()],
    });
    assert_eq!(
        permissions::resolve_execution_permissions(
            &definitions(),
            &PermissionConfiguration::defaults(false),
            HarnessProvider::Dsh,
            scope(),
            None,
            None,
            Some(&o)
        )
        .unwrap_err()
        .code,
        "permission_unsupported"
    );
}
#[test]
fn dsh_rejects_scoped_writes_that_its_native_workspace_grant_would_broaden() {
    let mut scope = scope();
    scope.work_type = WorkType::Implementation;
    scope.writable_paths = vec![".".into()];
    let overrides = PermissionOverrides {
        harness: PermissionBinding {
            overrides: PermissionPatch {
                filesystem: Some(FilesystemPermission::Scoped {
                    rules: vec![
                        PathPermission {
                            path: "/".into(),
                            access: FileAccess::Read,
                        },
                        PathPermission {
                            path: "/work/src".into(),
                            access: FileAccess::ReadWrite,
                        },
                    ],
                }),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    for provider in [HarnessProvider::Dsh, HarnessProvider::Codex] {
        let plan = permissions::resolve_execution_permissions(
            &definitions(),
            &PermissionConfiguration::defaults(false),
            provider,
            scope.clone(),
            None,
            None,
            Some(&overrides),
        );
        if provider == HarnessProvider::Dsh {
            assert_eq!(plan.unwrap_err().code, "permission_unsupported");
        } else {
            assert!(plan.is_ok());
        }
    }
}
#[test]
fn legacy_codex_keeps_the_original_root_and_state_read_denials() {
    for writes in [vec![], vec![".".into()], vec!["tests/task".into()]] {
        let mut scope = scope();
        scope.writable_paths = writes;
        let plan = permissions::resolve_execution_permissions(
            &definitions(),
            &PermissionConfiguration::defaults(true),
            HarnessProvider::Codex,
            scope,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            plan.harness.policy.file_access("/state/codex/auth.json"),
            FileAccess::Deny
        );
        assert_eq!(
            plan.harness.policy.file_access("/etc/legacy-private"),
            FileAccess::Deny
        );
        let config = permissions::codex_config(&plan);
        assert!(config.contains("\":root\"=\"deny\""));
        assert!(config.contains("\"/state\"=\"deny\""));
        assert!(config.contains("\":minimal\"=\"read\""));
    }
}
#[test]
fn fresh_and_upgraded_global_defaults_are_distinct_and_bindings_are_cas_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let global = GlobalStore::open(root.path()).unwrap();
    let mut state = EngineeringState::default();
    seed_configuration(&mut state);
    global
        .transact(None, &mut |s| {
            s.permissions = state.permissions.clone();
            Ok(json!({}))
        })
        .unwrap();
    let old = global.permission_configuration().unwrap();
    let mut unsupported = old.clone();
    unsupported.sandbox.overrides.network = Some(NetworkPermission::Disabled);
    let rejected = Command {
        command_id: id(),
        target: Some("permission-configuration".into()),
        expected_revision: Some(old.revision),
        payload: json!(unsupported),
    };
    assert_eq!(
        global.bind_permissions(&rejected).unwrap_err().code,
        "permission_conflict"
    );
    assert_eq!(
        global.permission_configuration().unwrap().revision,
        old.revision
    );
    assert_eq!(
        old.harnesses["dsh"]
            .permission
            .as_ref()
            .unwrap()
            .revision_id,
        "permission-container-full-access-v1"
    );
    let mut next = old.clone();
    next.harnesses
        .insert("dsh".into(), PermissionBinding::preset("read-only"));
    let c = Command {
        command_id: id(),
        target: Some("permission-configuration".into()),
        expected_revision: Some(1),
        payload: json!(next),
    };
    let first = global.bind_permissions(&c).unwrap();
    assert_eq!(first, global.bind_permissions(&c).unwrap());
    assert_eq!(first["revision"], 2);
    let mut stale = c.clone();
    stale.command_id = id();
    assert_eq!(
        global.bind_permissions(&stale).unwrap_err().code,
        "revision_conflict"
    );
    drop(global);
    let conn = rusqlite::Connection::open(root.path().join("global.sqlite3")).unwrap();
    conn.execute_batch("DROP TABLE permissions; ALTER TABLE harnesses DROP COLUMN permission; ALTER TABLE sandboxes DROP COLUMN permission; DELETE FROM settings WHERE key='permission_configuration'; PRAGMA user_version=3;").unwrap();
    drop(conn);
    let upgraded = GlobalStore::open(root.path()).unwrap();
    assert_eq!(
        upgraded.permission_configuration().unwrap().harnesses["dsh"]
            .permission
            .as_ref()
            .unwrap()
            .revision_id,
        "permission-legacy-restricted-v1"
    );
}
#[test]
fn permission_revisions_round_trip_and_reject_storage_rewrites() {
    let root = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(root.path()).unwrap();
    store
        .transact(None, &mut |s| {
            seed_configuration(s);
            Ok(json!({}))
        })
        .unwrap();
    let saved = store.snapshot().unwrap().permissions;
    assert!(
        store
            .transact(None, &mut |s| {
                s.permissions[0].name = "rewritten".into();
                Ok(json!({}))
            })
            .is_err()
    );
    drop(store);
    assert_eq!(
        SqliteStore::open(root.path())
            .unwrap()
            .snapshot()
            .unwrap()
            .permissions,
        saved
    );
}

#[test]
#[ignore = "requires real Docker and the pinned Codex native sandbox; no model call"]
fn codex_compiled_profile_allows_scoped_writes_and_denies_other_worktree_writes() {
    use devdesk_adapters::docker::{ContainerPurpose, ContainerSpec, DockerSandbox};
    use std::{
        fs, thread,
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let state = root.path().join("state");
    fs::create_dir_all(work.join("allowed")).unwrap();
    fs::create_dir_all(state.join("codex")).unwrap();
    let overrides = PermissionOverrides {
        harness: PermissionBinding {
            overrides: PermissionPatch {
                filesystem: Some(FilesystemPermission::Scoped {
                    rules: vec![
                        PathPermission {
                            path: "/".into(),
                            access: FileAccess::Read,
                        },
                        PathPermission {
                            path: "/work/allowed".into(),
                            access: FileAccess::ReadWrite,
                        },
                    ],
                }),
                network: Some(NetworkPermission::Disabled),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let plan = permissions::resolve_execution_permissions(
        &definitions(),
        &PermissionConfiguration::defaults(false),
        HarnessProvider::Codex,
        scope(),
        None,
        None,
        Some(&overrides),
    )
    .unwrap();
    fs::write(
        state.join("codex/config.toml"),
        permissions::codex_config(&plan),
    )
    .unwrap();
    let image = std::process::Command::new("docker")
        .args([
            "image",
            "inspect",
            "devdesk/codex:0.167.0",
            "--format",
            "{{.Id}}",
        ])
        .output()
        .unwrap();
    assert!(image.status.success());
    let spec = ContainerSpec {
        permissions: Some(plan), purpose: ContainerPurpose::Harness, project_id: id(),
        session_id: id(), operation_id: id(), image_id: String::from_utf8(image.stdout).unwrap().trim().into(),
        worktree: work.clone(), state, writable: true, writable_subdirectories: vec![],
        read_only_subdirectories: vec![], network: true, nested_sandbox: true,
        command: vec!["codex".into(), "sandbox".into(), "--".into(), "node".into(), "-e".into(),
            "const fs=require('fs');fs.writeFileSync('/work/allowed/proof','native-scoped-ok');try{fs.writeFileSync('/work/prohibited','bad');process.exit(3)}catch{}".into()],
        credential_env_file: None, profile_file: None, runner_file: None, profile_target: None,
    };
    let sandbox = DockerSandbox::new(root.path()).unwrap();
    sandbox.create(&spec).unwrap();
    sandbox.start(&spec.operation_id).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while sandbox
        .inspect(&spec.operation_id)
        .unwrap()
        .is_some_and(|r| r.running)
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(100));
    }
    let logs = sandbox.logs(&spec.operation_id).unwrap();
    let exit = sandbox
        .inspect(&spec.operation_id)
        .unwrap()
        .unwrap()
        .exit_code;
    sandbox.stop(&spec.operation_id).unwrap();
    sandbox.remove(&spec.operation_id).unwrap();
    assert_eq!(exit, 0, "{logs}");
    assert_eq!(
        fs::read_to_string(work.join("allowed/proof")).unwrap(),
        "native-scoped-ok"
    );
    assert!(!work.join("prohibited").exists());
}

#[test]
#[ignore = "requires a real Docker daemon and pinned Node image"]
fn full_access_container_writes_and_exposes_only_its_declared_trust_scope() {
    use devdesk_adapters::docker::{ContainerPurpose, ContainerSpec, DockerSandbox};
    use std::{
        fs, thread,
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let state = root.path().join("state");
    fs::create_dir(&work).unwrap();
    fs::create_dir(&state).unwrap();
    fs::write(state.join("canary"), "FAKE_CONTAINER_PERMISSION_CANARY").unwrap();
    let output = std::process::Command::new("docker")
        .args([
            "image",
            "inspect",
            "devdesk/codex:0.167.0",
            "--format",
            "{{.Id}}",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let image = String::from_utf8(output.stdout).unwrap().trim().to_owned();
    let mut scope = scope();
    scope.work_type = WorkType::Implementation;
    scope.writable_paths = vec![".".into()];
    let plan = permissions::resolve_execution_permissions(
        &definitions(),
        &PermissionConfiguration::defaults(false),
        HarnessProvider::Codex,
        scope,
        None,
        None,
        None,
    )
    .unwrap();
    let spec=ContainerSpec {permissions:Some(plan),purpose:ContainerPurpose::Harness,project_id:id(),session_id:id(),operation_id:id(),image_id:image,worktree:work.clone(),state:state.clone(),writable:true,writable_subdirectories:vec![],read_only_subdirectories:vec![],network:true,command:vec!["sh".into(),"-c".into(),"test -r /state/canary && printf '#!/bin/sh\nprintf executable' > /tmp/permission-probe && chmod +x /tmp/permission-probe && /tmp/permission-probe > /work/proof && test ! -e /var/run/docker.sock".into()],nested_sandbox:false,credential_env_file:None,profile_file:None,runner_file:None,profile_target:None};
    let sandbox = DockerSandbox::new(root.path()).unwrap();
    let receipt = sandbox.create(&spec).unwrap();
    let inspect = std::process::Command::new("docker")
        .args(["inspect", &receipt.id])
        .output()
        .unwrap();
    let actual: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(actual[0]["HostConfig"]["ReadonlyRootfs"], false);
    assert_eq!(actual[0]["HostConfig"]["Privileged"], false);
    assert_eq!(actual[0]["HostConfig"]["NetworkMode"], "bridge");
    assert!(
        !actual[0]["HostConfig"]["SecurityOpt"]
            .to_string()
            .contains("devdesk-harness")
    );
    sandbox.start(&spec.operation_id).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while sandbox
        .inspect(&spec.operation_id)
        .unwrap()
        .is_some_and(|r| r.running)
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(50));
    }
    let logs = sandbox.logs(&spec.operation_id).unwrap();
    let exit = sandbox
        .inspect(&spec.operation_id)
        .unwrap()
        .unwrap()
        .exit_code;
    let proof = fs::read_to_string(work.join("proof"));
    sandbox.stop(&spec.operation_id).unwrap();
    sandbox.remove(&spec.operation_id).unwrap();
    assert_eq!(exit, 0, "{logs}");
    assert_eq!(proof.unwrap(), "executable");
}

#[test]
#[ignore = "requires actual Docker and the pinned DSH native SDK; no model call"]
fn dsh_restricted_native_executor_hides_state_environment_and_tool_network() {
    use devdesk_adapters::docker::{ContainerPurpose, ContainerSpec, DockerSandbox};
    use std::{
        fs, thread,
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let state = root.path().join("state");
    fs::create_dir(&work).unwrap();
    fs::create_dir(work.join("tests")).unwrap();
    fs::create_dir(&state).unwrap();
    fs::write(state.join("canary"), "FAKE_PERMISSION_CANARY").unwrap();
    let command = "node -e \"const fs=require('fs');if(fs.existsSync('/state/canary')||process.env.DEEPSEEK_API_KEY)process.exit(3);if(Object.values(require('os').networkInterfaces()).flat().some(i=>!i.internal))process.exit(5);fs.writeFileSync('/work/proof','restricted-native-ok');try{fs.writeFileSync('/work/tests/prohibited','bad');process.exit(6)}catch{}fetch('http://1.1.1.1',{signal:AbortSignal.timeout(1500)}).then(()=>process.exit(4)).catch(()=>console.log('NETWORK_DENIED'))\"";
    let module = format!(
        "import {{writeFileSync}} from 'node:fs';export const name='devdesk-permission-probe';export const inject=['shell'];export async function apply(ctx){{try{{const ex=await ctx.shell.execute(ctx.shell.resolve({{command:{},workdir:'/work',timeoutMs:8000}}));const result=await ex.result();writeFileSync('/state/probe-result.json',JSON.stringify(result));process.exit(0);}}catch(e){{writeFileSync('/state/probe-result.json',JSON.stringify({{error:String(e)}}));process.exit(1);}}}}",
        serde_json::to_string(command).unwrap()
    );
    fs::write(state.join("probe.mjs"), module).unwrap();
    let mut scope = scope();
    scope.work_type = WorkType::Implementation;
    scope.writable_paths = vec![".".into()];
    scope.read_only_paths = vec!["tests".into()];
    let plan = permissions::resolve_execution_permissions(
        &definitions(),
        &PermissionConfiguration::defaults(true),
        HarnessProvider::Dsh,
        scope,
        None,
        None,
        None,
    )
    .unwrap();
    let mut patch =
        permissions::dsh_profile(&plan, "Controlled credential-free policy test", "/work").unwrap();
    patch.push_str("- insert:\n  - id: devdesk-permission-probe\n    name: /state/probe.mjs\n");
    let profile = root.path().join("profile.yaml");
    fs::write(&profile, patch).unwrap();
    let runner = root.path().join("runner.sh");
    fs::write(
        &runner,
        include_bytes!("../../../permissions/backends/dsh/bwrap-runner.sh"),
    )
    .unwrap();
    let env = root.path().join("fake.env");
    fs::write(&env, "DEEPSEEK_API_KEY=FAKE_PERMISSION_CANARY_NOT_A_KEY\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&env, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let image = std::process::Command::new("docker")
        .args([
            "image",
            "inspect",
            "devdesk/dsh:0.2.2-rc.1",
            "--format",
            "{{.Id}}",
        ])
        .output()
        .unwrap();
    assert!(image.status.success());
    let spec = ContainerSpec {
        permissions: Some(plan),
        purpose: ContainerPurpose::Harness,
        project_id: id(),
        session_id: id(),
        operation_id: id(),
        image_id: String::from_utf8(image.stdout).unwrap().trim().into(),
        worktree: work.clone(),
        state: state.clone(),
        writable: true,
        writable_subdirectories: vec![],
        read_only_subdirectories: vec!["tests".into()],
        network: true,
        command: vec![
            "dsh".into(),
            "--profile".into(),
            "sdk".into(),
            "--patch".into(),
            "/run/devdesk/profile".into(),
        ],
        nested_sandbox: true,
        credential_env_file: Some(env),
        profile_file: Some(profile),
        runner_file: Some(runner),
        profile_target: None,
    };
    let sandbox = DockerSandbox::new(root.path()).unwrap();
    sandbox.create(&spec).unwrap();
    sandbox.start(&spec.operation_id).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while sandbox
        .inspect(&spec.operation_id)
        .unwrap()
        .is_some_and(|r| r.running)
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(100));
    }
    let logs = sandbox.logs(&spec.operation_id).unwrap();
    let result = fs::read_to_string(state.join("probe-result.json"));
    sandbox.stop(&spec.operation_id).unwrap();
    sandbox.remove(&spec.operation_id).unwrap();
    let value: serde_json::Value =
        serde_json::from_str(&result.unwrap_or_else(|e| panic!("{e}: {logs}"))).unwrap();
    assert_eq!(value["exitCode"], 0, "{value} {logs}");
    assert_eq!(
        fs::read_to_string(work.join("proof")).unwrap(),
        "restricted-native-ok"
    );
}
