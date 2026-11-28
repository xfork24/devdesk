use devdesk_core::id;
use devdesk_domain::*;
#[path = "../../../tests/support/permissions.rs"]
mod permission_fixture;
mod support;
use devdesk_adapters::docker::*;
use std::{
    fs, thread,
    time::{Duration, Instant},
};

#[test]
fn reject_outside_mounts_before_touching_docker() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let sandbox = DockerSandbox::new(root.path()).unwrap();
    let spec = ContainerSpec {
        host_gateway: false,
        permissions: None,
        purpose: ContainerPurpose::Tests,
        nested_sandbox: false,
        credential_env_file: None,
        profile_file: None,
        bridge_file: None,
        runner_file: None,
        skill_directory: None,
        command_output_directory: None,
        sensitive_output: false,
        profile_target: None,
        project_id: id(),
        session_id: id(),
        operation_id: id(),
        image_id: format!("sha256:{}", "a".repeat(64)),
        worktree: outside.path().into(),
        state: root.path().into(),
        writable: true,
        writable_subdirectories: vec![],
        read_only_subdirectories: vec![],
        network: false,
        command: vec!["sh".into()],
    };
    assert!(
        sandbox
            .create(&spec)
            .unwrap_err()
            .message
            .contains("执行根目录")
    );
}

/// Explicit live invocation; never counted as passed by the offline suite.
#[test]
#[ignore = "requires the real Docker daemon and the pinned acceptance image"]
fn runtime_cache_is_executable_and_shared_across_projects_without_sharing_private_state() {
    check_runtime_cache(true);
}

#[test]
#[ignore = "requires the real Docker daemon and the pinned acceptance image"]
fn project_runtime_cache_is_reused_within_one_project_and_isolated_from_other_projects() {
    check_runtime_cache(false);
}

fn check_runtime_cache(shared: bool) {
    let directory = tempfile::tempdir().unwrap();
    let cache = directory.path().join("public-runtime-cache");
    let first = id();
    let second = id();
    for index in 0..3 {
        let project = if index == 1 { &second } else { &first };
        let root = directory.path().join(project);
        let work = root.join("work");
        let state = root.join(format!("state-{index}"));
        fs::create_dir_all(&work).unwrap();
        fs::create_dir_all(&state).unwrap();
        let sandbox = DockerSandbox::new(&root).unwrap();
        let sandbox = if shared {
            sandbox.with_runtime_cache(cache.clone())
        } else {
            sandbox
        };
        let operation = id();
        let script = if index == 0 {
            "set -eu; printf shared > \"$DEVDESK_RUNTIME_CACHE/proof\"; cp /bin/true \"$DEVDESK_RUNTIME_CACHE/tool\"; chmod +x \"$DEVDESK_RUNTIME_CACHE/tool\"; \"$DEVDESK_RUNTIME_CACHE/tool\"; printf private > \"$DEVDESK_RUNTIME_DIR/private\"; mkdir -p \"$npm_config_cache\"; printf package > \"$npm_config_cache/proof\"; truncate -s 268435456 \"$DEVDESK_RUNTIME_CACHE/large-cache\"; test ! -e /work/proof"
        } else if shared || index == 2 {
            "set -eu; test \"$(cat \"$DEVDESK_RUNTIME_CACHE/proof\")\" = shared; \"$DEVDESK_RUNTIME_CACHE/tool\"; test -s \"$npm_config_cache/proof\"; test ! -e \"$DEVDESK_RUNTIME_DIR/private\"; test ! -e /work/proof; test \"$(stat -c%s \"$DEVDESK_RUNTIME_CACHE/large-cache\")\" = 268435456"
        } else {
            "set -eu; test ! -e \"$DEVDESK_RUNTIME_CACHE/proof\"; test ! -e \"$npm_config_cache/proof\"; test ! -e \"$DEVDESK_RUNTIME_DIR/private\""
        };
        let spec = ContainerSpec {
            host_gateway: false,
            permissions: None,
            purpose: ContainerPurpose::Tests,
            nested_sandbox: false,
            credential_env_file: None,
            profile_file: None,
            bridge_file: None,
            runner_file: None,
            skill_directory: None,
            command_output_directory: None,
            sensitive_output: false,
            profile_target: None,
            project_id: project.clone(),
            session_id: id(),
            operation_id: operation.clone(),
            image_id: support::image("codex"),
            worktree: work.clone(),
            state: state.clone(),
            writable: false,
            writable_subdirectories: vec![],
            read_only_subdirectories: vec![],
            network: false,
            command: vec!["sh".into(), "-c".into(), script.into()],
        };
        sandbox.create(&spec).unwrap();
        sandbox.start(&operation).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let receipt = loop {
            let receipt = sandbox.inspect(&operation).unwrap().unwrap();
            if !receipt.running {
                break receipt;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(50));
        };
        let diagnostic = sandbox.logs(&operation).unwrap();
        sandbox.stop(&operation).unwrap();
        sandbox.remove(&operation).unwrap();
        assert_eq!(
            receipt.exit_code, 0,
            "cache round {index}, shared={shared}: {diagnostic}"
        );
        assert!(!work.join("proof").exists());
        if index == 0 {
            // Independent containers can enter the shared project cache at
            // once. Their probe shells all have PID 1; PID-based probe names
            // would collide and misreport ordinary concurrency as permission
            // failure before the business command even starts.
            let barrier = std::sync::Barrier::new(3);
            thread::scope(|scope| {
                let handles = (0..3)
                    .map(|_| {
                        let mut parallel = spec.clone();
                        parallel.operation_id = id();
                        let sandbox = &sandbox;
                        let barrier = &barrier;
                        scope.spawn(move || {
                            barrier.wait();
                            sandbox.create(&parallel).unwrap();
                            sandbox.remove(&parallel.operation_id).unwrap();
                        })
                    })
                    .collect::<Vec<_>>();
                for handle in handles {
                    handle.join().unwrap();
                }
            });
        }
        #[cfg(unix)]
        if index == 0 {
            use std::os::unix::fs::PermissionsExt;
            let installed = if shared {
                cache.clone()
            } else {
                root.join("runtime-cache").join(project)
            }
            .join("images")
            .join(spec.image_id.trim_start_matches("sha256:"));
            fs::set_permissions(&installed, fs::Permissions::from_mode(0o500)).unwrap();
            let mut rejected = spec.clone();
            rejected.operation_id = id();
            rejected.command = vec![
                "sh".into(),
                "-c".into(),
                "printf ran > \"$DEVDESK_RUNTIME_DIR/business-started\"".into(),
            ];
            let result = sandbox.create(&rejected);
            fs::set_permissions(&installed, fs::Permissions::from_mode(0o700)).unwrap();
            let error = result.unwrap_err();
            assert_eq!(error.code, "runtime_permission_denied");
            assert_eq!(
                error.execution_failure.unwrap().effect,
                ExecutionFailureEffect::NotApplied
            );
            assert!(sandbox.inspect(&rejected.operation_id).unwrap().is_none());
            assert!(!state.join("runtime/business-started").exists());
        }
    }
}

/// Explicit live invocation; never counted as passed by the offline suite.
#[test]
#[ignore = "requires the real Docker daemon and the pinned acceptance image"]
fn real_container_ownership_idempotence_stop_and_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let state = root.path().join("state");
    fs::create_dir(&work).unwrap();
    fs::create_dir(&state).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [&work, &state] {
            fs::set_permissions(path, fs::Permissions::from_mode(0o777)).unwrap();
        }
    }
    fs::write(work.join(".git"), "gitdir: /private/host-secret\n").unwrap();
    let sandbox = DockerSandbox::new(root.path()).unwrap();
    let operation = id();
    let spec=ContainerSpec { host_gateway:false, permissions: None,
        purpose:ContainerPurpose::Tests,nested_sandbox:false,credential_env_file:None,profile_file:None,bridge_file:None,runner_file:None,skill_directory:None,command_output_directory:None,sensitive_output:false,profile_target:None,project_id:id(),session_id:id(),operation_id:operation.clone(),image_id:support::image("codex"),worktree:work.clone(),state,writable:true,writable_subdirectories:vec![],read_only_subdirectories:vec![],network:false,command:vec!["sh".into(),"-c".into(),"test ! -s /work/.git && test ! -e /var/run/docker.sock && (! touch /outside 2>/dev/null) && printf real-container > /work/proof && sleep 30".into()]};
    let first = sandbox.create(&spec).unwrap();
    assert!(!first.running);
    assert_eq!(first.id, sandbox.create(&spec).unwrap().id);
    let mut different = spec.clone();
    different.network = true;
    assert!(sandbox.create(&different).is_err());
    sandbox.start(&operation).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !work.join("proof").exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    let proof = fs::read_to_string(work.join("proof"));
    // Stop even if the command assertion failed, retaining inspectable proof until then.
    sandbox.stop(&operation).unwrap();
    assert!(!sandbox.inspect(&operation).unwrap().unwrap().running);
    assert_eq!(proof.unwrap(), "real-container");
    assert!(
        fs::read_to_string(work.join(".git"))
            .unwrap()
            .contains("host-secret")
    );
    sandbox.remove(&operation).unwrap();
    sandbox.remove(&operation).unwrap();
    assert!(sandbox.inspect(&operation).unwrap().is_none());
}

#[test]
#[ignore = "requires actual Docker and pinned Codex Linux image"]
fn nested_sandbox_cannot_broaden_the_outer_test_directory_write_grant() {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let state = root.path().join("state");
    fs::create_dir_all(work.join("tests/allowed")).unwrap();
    fs::create_dir_all(work.join("src")).unwrap();
    fs::create_dir_all(state.join("codex")).unwrap();
    // Use the same compiled isolation contract as production. A hand-written
    // profile alone omits the executable temporary mount required by bubblewrap.
    let plan = permission_fixture::compile(
        HarnessProvider::Codex,
        ExecutionScope {
            work_type: WorkType::Discussion,
            writable_paths: vec!["tests/allowed".into()],
            read_only_paths: vec![],
        },
        PermissionPolicy {
            filesystem: FilesystemPermission::Scoped {
                rules: vec![
                    PathPermission {
                        path: "/".into(),
                        access: FileAccess::Read,
                    },
                    PathPermission {
                        path: "/work/tests/allowed".into(),
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
        devdesk_adapters::permissions::codex_config(&plan),
    )
    .unwrap();
    let operation = id();
    let sandbox = DockerSandbox::new(root.path())
        .unwrap()
        .with_runtime_cache(root.path().join("runtime-cache"));
    let script = "set -eu; printf scope-ok > /work/tests/allowed/proof; cp /bin/true \"$DEVDESK_RUNTIME_CACHE/native-tool\"; \"$DEVDESK_RUNTIME_CACHE/native-tool\"; printf runtime-ok > \"$DEVDESK_RUNTIME_DIR/native-proof\"; (! touch /work/src/forbidden 2>/dev/null); (! touch /work/forbidden 2>/dev/null)";
    let spec = ContainerSpec {
        host_gateway: false,
        permissions: Some(plan),
        purpose: ContainerPurpose::Harness,
        project_id: id(),
        session_id: id(),
        operation_id: operation.clone(),
        image_id: support::image("codex"),
        worktree: work.clone(),
        state: state.clone(),
        writable: false,
        writable_subdirectories: vec!["tests/allowed".into()],
        read_only_subdirectories: vec![],
        network: true,
        nested_sandbox: true,
        credential_env_file: None,
        profile_file: None,
        bridge_file: None,
        runner_file: None,
        skill_directory: None,
        command_output_directory: None,
        sensitive_output: false,
        profile_target: None,
        command: vec![
            "codex".into(),
            "sandbox".into(),
            "--".into(),
            "/bin/sh".into(),
            "-c".into(),
            script.into(),
        ],
    };
    sandbox.create(&spec).unwrap();
    sandbox.start(&operation).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let receipt = loop {
        let receipt = sandbox.inspect(&operation).unwrap().unwrap();
        if !receipt.running {
            break receipt;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let diagnostic = std::process::Command::new("docker")
        .args(["logs", &receipt.name])
        .output()
        .unwrap();
    sandbox.stop(&operation).unwrap();
    sandbox.remove(&operation).unwrap();
    assert_eq!(
        receipt.exit_code,
        0,
        "{} {}",
        String::from_utf8_lossy(&diagnostic.stdout),
        String::from_utf8_lossy(&diagnostic.stderr)
    );
    assert_eq!(
        fs::read_to_string(work.join("tests/allowed/proof")).unwrap(),
        "scope-ok"
    );
    assert!(!work.join("src/forbidden").exists());
    assert!(!work.join("forbidden").exists());
    assert_eq!(
        fs::read_to_string(state.join("runtime/native-proof")).unwrap(),
        "runtime-ok"
    );
}

#[test]
#[ignore = "requires actual Docker and pinned DSH image"]
fn dsh_runner_masks_private_state_and_proc_without_expanding_docker_privileges() {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let state = root.path().join("state");
    let coordinator = root.path().join("coordinator");
    fs::create_dir_all(work.join("tests/allowed")).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&coordinator).unwrap();
    fs::write(state.join("canary"), "not-a-credential").unwrap();
    let runner = coordinator.join("runner.sh");
    fs::write(
        &runner,
        include_bytes!("../../../permissions/backends/dsh/bwrap-runner.sh"),
    )
    .unwrap();
    let operation = id();
    let sandbox = DockerSandbox::new(root.path()).unwrap();
    let command = vec![
        "/bin/bash",
        "/run/devdesk/runner",
        "--ro-bind",
        "/",
        "/",
        "--dev",
        "/dev",
        "--unshare-pid",
        "--proc",
        "/proc",
        "--die-with-parent",
        "--tmpfs",
        "/tmp",
        "--bind",
        "/work/tests/allowed",
        "/work/tests/allowed",
        "--",
        "/bin/bash",
        "-c",
        "test ! -e /state/canary && test ! -e /proc/1/environ && test ! -e /run/devdesk/runner && printf dsh-ok > /work/tests/allowed/proof && (! touch /work/forbidden 2>/dev/null)",
    ];
    let spec = ContainerSpec {
        host_gateway: false,
        permissions: None,
        purpose: ContainerPurpose::Harness,
        project_id: id(),
        session_id: id(),
        operation_id: operation.clone(),
        image_id: support::image("dsh"),
        worktree: work.clone(),
        state,
        writable: false,
        writable_subdirectories: vec!["tests/allowed".into()],
        read_only_subdirectories: vec![],
        network: false,
        nested_sandbox: true,
        credential_env_file: None,
        profile_file: None,
        bridge_file: None,
        runner_file: Some(runner),
        skill_directory: None,
        command_output_directory: None,
        sensitive_output: false,
        profile_target: None,
        command: command.into_iter().map(str::to_string).collect(),
    };
    sandbox.create(&spec).unwrap();
    sandbox.start(&operation).unwrap();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let receipt = loop {
        let r = sandbox.inspect(&operation).unwrap().unwrap();
        if !r.running {
            break r;
        }
        assert!(std::time::Instant::now() < end);
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let output = std::process::Command::new("docker")
        .args(["logs", &receipt.name])
        .output()
        .unwrap();
    sandbox.stop(&operation).unwrap();
    sandbox.remove(&operation).unwrap();
    assert_eq!(
        receipt.exit_code,
        0,
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(work.join("tests/allowed/proof")).unwrap(),
        "dsh-ok"
    );
    assert!(!work.join("forbidden").exists());
}

#[test]
#[ignore = "actual Docker: original command outputs are readable and cannot be overwritten"]
fn original_command_outputs_are_a_separate_read_only_worker_mount() {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let state = root.path().join("state");
    let output = root.path().join("command-output");
    for p in [&work, &state, &output] {
        fs::create_dir(p).unwrap();
    }
    fs::write(output.join("original.json"), "original full output").unwrap();
    let sandbox = DockerSandbox::new(root.path()).unwrap();
    let operation = id();
    let spec=ContainerSpec{host_gateway:false,permissions:None,purpose:ContainerPurpose::Tests,nested_sandbox:false,credential_env_file:None,profile_file:None,bridge_file:None,runner_file:None,skill_directory:None,command_output_directory:Some(output.clone()),sensitive_output:false,profile_target:None,project_id:id(),session_id:id(),operation_id:operation.clone(),image_id:support::image("codex"),worktree:work,state,writable:false,writable_subdirectories:vec![],read_only_subdirectories:vec![],network:false,command:vec!["sh".into(),"-c".into(),"cat /run/devdesk/command-output/original.json; if printf corrupted > /run/devdesk/command-output/original.json 2>/dev/null; then exit 3; fi; test ! -e /run/devdesk/coordinator".into()]};
    sandbox.create(&spec).unwrap();
    sandbox.start(&operation).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let receipt = loop {
        let receipt = sandbox.inspect(&operation).unwrap().unwrap();
        if receipt.status == "exited" {
            break receipt;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    };
    let logs = sandbox.logs(&operation).unwrap();
    sandbox.remove(&operation).unwrap();
    assert_eq!(receipt.exit_code, 0, "{logs}");
    assert!(logs.contains("original full output"));
    assert_eq!(
        fs::read_to_string(output.join("original.json")).unwrap(),
        "original full output"
    );
}
