mod support;
use devdesk_adapters::docker::*;
use devdesk_domain::id;
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
        purpose: ContainerPurpose::Tests,
        nested_sandbox: false,
        credential_env_file: None,
        profile_file: None,
        runner_file: None,
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
    let spec=ContainerSpec {
        purpose:ContainerPurpose::Tests,nested_sandbox:false,credential_env_file:None,profile_file:None,runner_file:None,profile_target:None,project_id:id(),session_id:id(),operation_id:operation.clone(),image_id:support::image("codex"),worktree:work.clone(),state,writable:true,writable_subdirectories:vec![],read_only_subdirectories:vec![],network:false,command:vec!["sh".into(),"-c".into(),"test ! -s /work/.git && test ! -e /var/run/docker.sock && (! touch /outside 2>/dev/null) && printf real-container > /work/proof && sleep 30".into()]};
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
    fs::write(state.join("codex/config.toml"),"default_permissions=\"devdesk\"\n[permissions.devdesk.filesystem]\n\":root\"=\"deny\"\n\":minimal\"=\"read\"\n\"/work\"=\"read\"\n\"/work/tests/allowed\"=\"write\"\n\"/state\"=\"deny\"\n[permissions.devdesk.network]\nenabled=false\n").unwrap();
    let operation = id();
    let sandbox = DockerSandbox::new(root.path()).unwrap();
    let spec=ContainerSpec{purpose:ContainerPurpose::Harness,project_id:id(),session_id:id(),operation_id:operation.clone(),image_id:support::image("codex"),worktree:work.clone(),state,writable:false,writable_subdirectories:vec!["tests/allowed".into()],read_only_subdirectories:vec![],network:false,nested_sandbox:true,credential_env_file:None,profile_file:None,runner_file:None,profile_target:None,command:vec!["codex".into(),"sandbox".into(),"--".into(),"/bin/sh".into(),"-c".into(),"printf scope-ok > /work/tests/allowed/proof && (! touch /work/src/forbidden 2>/dev/null) && (! touch /work/forbidden 2>/dev/null)".into()]};
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
        include_bytes!("../../../containers/dsh/devdesk-policy/bwrap.sh"),
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
        runner_file: Some(runner),
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
