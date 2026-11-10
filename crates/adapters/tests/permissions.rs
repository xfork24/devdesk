use devdesk_adapters::permissions;
use devdesk_domain::*;
#[path = "../../../tests/support/permissions.rs"]
mod permission_fixture;
fn scope() -> ExecutionScope {
    ExecutionScope {
        work_type: WorkType::Discussion,
        writable_paths: vec![],
        read_only_paths: vec![],
    }
}
#[test]
fn full_access_and_restricted_profiles_use_native_enforcement() {
    for provider in [HarnessProvider::Codex, HarnessProvider::Dsh] {
        let p = permission_fixture::compile(provider, scope(), PermissionPolicy::full_access())
            .unwrap();
        assert!(p.binding_lock.is_some());
        assert!(!p.isolation.nested_sandbox);
        assert_eq!(p.isolation.credential_boundary, "container-trust");
        assert!(p.isolation.executable_tmp);
        assert_eq!(p.enforcement.compiler_version, "permission-compiler/v2");
    }
    let mut policy = PermissionPolicy::full_access();
    policy.network = NetworkPermission::Disabled;
    assert_eq!(
        permission_fixture::compile(HarnessProvider::Dsh, scope(), policy.clone())
            .unwrap_err()
            .code,
        "permission_unsupported"
    );
    policy.filesystem = FilesystemPermission::ReadOnly;
    policy.tools = ToolPermission::Restricted {
        allow: vec!["bash".into(), "ask_user_question".into()],
        deny: vec![],
    };
    assert!(
        permission_fixture::compile(HarnessProvider::Dsh, scope(), policy)
            .unwrap()
            .isolation
            .nested_sandbox
    );
}
#[test]
fn unsupported_grants_are_rejected_before_execution() {
    let mut policy = PermissionPolicy::full_access();
    policy.filesystem = FilesystemPermission::Scoped {
        rules: vec![PathPermission {
            path: "/state".into(),
            access: FileAccess::Deny,
        }],
    };
    assert!(permission_fixture::compile(HarnessProvider::Codex, scope(), policy).is_err());
    let mut policy = PermissionPolicy::full_access();
    policy.network = NetworkPermission::Allowlist {
        destinations: vec!["example.com".into()],
    };
    assert!(permission_fixture::compile(HarnessProvider::Codex, scope(), policy).is_err());
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
    let plan = permission_fixture::compile(
        HarnessProvider::Codex,
        scope(),
        PermissionPolicy {
            filesystem: FilesystemPermission::Scoped {
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
            },
            network: NetworkPermission::Disabled,
            tools: ToolPermission::AllAvailable,
        },
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
    let spec = ContainerSpec { host_gateway: false,
        permissions: Some(plan), purpose: ContainerPurpose::Harness, project_id: id(),
        session_id: id(), operation_id: id(), image_id: String::from_utf8(image.stdout).unwrap().trim().into(),
        worktree: work.clone(), state, writable: true, writable_subdirectories: vec![],
        read_only_subdirectories: vec![], network: true, nested_sandbox: true,
        command: vec!["codex".into(), "sandbox".into(), "--".into(), "node".into(), "-e".into(),
            "const fs=require('fs');fs.writeFileSync('/work/allowed/proof','native-scoped-ok');try{fs.writeFileSync('/work/prohibited','bad');process.exit(3)}catch{}".into()],
        credential_env_file: None, profile_file: None, runner_file: None, skill_directory: None, sensitive_output: false, profile_target: None, };
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
    let plan = permission_fixture::compile(
        HarnessProvider::Codex,
        scope,
        PermissionPolicy::full_access(),
    )
    .unwrap();
    let spec=ContainerSpec {host_gateway:false, permissions:Some(plan),purpose:ContainerPurpose::Harness,project_id:id(),session_id:id(),operation_id:id(),image_id:image,worktree:work.clone(),state:state.clone(),writable:true,writable_subdirectories:vec![],read_only_subdirectories:vec![],network:true,command:vec!["sh".into(),"-c".into(),"test -r /state/canary && printf '#!/bin/sh\nprintf executable' > /tmp/permission-probe && chmod +x /tmp/permission-probe && /tmp/permission-probe > /work/proof && test ! -e /var/run/docker.sock".into()],nested_sandbox:false,credential_env_file:None,profile_file:None,runner_file:None,skill_directory:None,sensitive_output:false,profile_target:None};
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
