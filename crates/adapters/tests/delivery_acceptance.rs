//! Explicit local acceptance using the production Git and Docker test adapters.
//! No model, credentials, remote repository, or user dataset is used.
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
use devdesk_adapters::{
    CliGit, CommandTests, FileArtifacts, SqliteStore, integrations::NativeTools,
};
use devdesk_core::{
    Command, Engine,
    ports::{Git, TestRequest, Tests, ToolExecutor},
};
use devdesk_domain::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    process::Command as Process,
    sync::Arc,
    time::{Duration, Instant},
};

fn run(path: &Path, executable: &str, args: &[&str]) -> String {
    let out = Process::new(executable)
        .current_dir(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
fn git(path: &Path, args: &[&str]) -> String {
    let mut flags = vec![
        "-c",
        "user.name=Delivery Acceptance",
        "-c",
        "user.email=acceptance@localhost",
        "-c",
        "commit.gpgSign=false",
        "-c",
        "core.hooksPath=/dev/null",
    ];
    flags.extend_from_slice(args);
    run(path, "git", &flags)
}
fn test_request(
    context: &GitContext,
    revision: &str,
    image: &str,
    expect_red: bool,
) -> TestRequest {
    let operation_id = id();
    let environment = id();
    TestRequest {
        operation_id: operation_id.clone(),
        session_id: None,
        candidate: revision.into(),
        environment: environment.clone(),
        origin: Origin::External,
        code: Some(context.clone()),
        expect_assertion_failure: expect_red,
        recipe: Some(TestRecipe {
            runner: TestRunner::NodeTest,
            files: vec!["tests/add.test.cjs".into()],
            image: image.into(),
            timeout_seconds: 30,
        }),
        runtime: Some(ExecutionInstance {
            id: environment,
            operation_id,
            harness: Harness {
                id: "local-node-test".into(),
                provider: "node-test".into(),
                version: "24".into(),
                capabilities: vec!["node_test".into()],
                origin: Origin::External,
                permission: None,
            },
            sandbox: Sandbox {
                id: "local-docker".into(),
                provider: "docker".into(),
                version: "local-verified-daemon".into(),
                platform: "linux".into(),
                image: Some(image.into()),
                network: false,
                origin: Origin::External,
                permission: None,
            },
            external_session: None,
            external_environment: None,
            status: Status::Running,
            started: None,
            finished: None,
        }),
    }
}
fn execute(tests: &CommandTests, request: &TestRequest) -> Value {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(result) = tests.poll_test(request).unwrap() {
            return result;
        }
        assert!(
            Instant::now() < deadline,
            "actual Docker test did not complete"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "manual acceptance: actual Docker and an already available pinned Node image, no network"]
fn actual_docker_red_green_candidate_reports_and_git_diff_match_their_specs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let image = std::env::var("DEVDESK_ACCEPTANCE_NODE_IMAGE")
        .expect("provide a locally verified sha256 Docker image ID");
    let actual = run(
        root,
        "docker",
        &["image", "inspect", &image, "--format", "{{.Id}}"],
    );
    assert_eq!(actual, image);
    let engine = Engine::new(
        Arc::new(SqliteStore::open(root).unwrap()),
        Arc::new(FileArtifacts::new(root.into())),
        runtime::adapters(),
    );
    engine.bootstrap().unwrap();
    let project = engine
        .execute(
            "project.create",
            Command {
                command_id: id(),
                target: None,
                expected_revision: None,
                payload: json!({"title":"Local Delivery acceptance"}),
            },
        )
        .unwrap();
    let project = project["id"].as_str().unwrap();
    let repo = root.join("projects").join(project).join("repository");
    std::fs::create_dir_all(repo.join("tests")).unwrap();
    git(&repo, &["init", "-b", "main"]);
    std::fs::write(repo.join("add.cjs"), "exports.add = (a, b) => 0;\n").unwrap();
    std::fs::write(repo.join("tests/add.test.cjs"),"const {test}=require('node:test'); const assert=require('node:assert/strict'); const {add}=require('../add.cjs'); test('AC-1 adds values',()=>{assert.equal(add(2,3),5);assert.equal(add(-2,3),1);assert.equal(add(0,0),0)});\n").unwrap();
    git(&repo, &["add", "."]);
    git(
        &repo,
        &["commit", "-m", "freeze independent test and Red baseline"],
    );
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let adapter = CliGit::new(root).unwrap();
    let target = adapter
        .prepare_worktree(project, &id(), "features", &base, &id())
        .unwrap();
    let source = adapter
        .prepare_worktree(project, &id(), "tasks", &base, &id())
        .unwrap();
    let context = GitContext {
        project: project.into(),
        workspace: Workspace {
            repository: "acceptance/local".into(),
            path: repo.canonicalize().unwrap().to_string_lossy().into(),
            mainline: "main".into(),
        },
        source: CodeLocation {
            owner: source.owner,
            branch: source.branch,
            worktree: source.path,
        },
        target: CodeLocation {
            owner: target.owner,
            branch: target.branch,
            worktree: target.path,
        },
        origin: Origin::External,
    };
    let tests = CommandTests::new(root).unwrap();
    let red_request = test_request(&context, &base, &image, true);
    let red = execute(&tests, &red_request);
    assert_eq!(red["expected_failure"], true);
    assert_eq!(red["assertion_failed"], true);
    assert_ne!(red["execution"]["exit_code"], 0);
    std::fs::write(
        Path::new(&context.source.worktree).join("add.cjs"),
        "exports.add = (a, b) => a + b;\n",
    )
    .unwrap();
    let green_revision = adapter.snapshot_worktree(&context, &base, &id()).unwrap();
    let green_request = test_request(&context, &green_revision, &image, false);
    let green = execute(&tests, &green_request);
    assert_eq!(green["status"], "succeeded");
    assert_eq!(green["execution"]["exit_code"], 0);
    let candidate = adapter
        .candidate(&context, &green_revision, &base, &id())
        .unwrap();
    let candidate_request = test_request(&context, &candidate, &image, false);
    let candidate_tests = execute(&tests, &candidate_request);
    assert_eq!(candidate_tests["status"], "succeeded");
    assert_eq!(candidate_tests["candidate"], candidate);
    let state = engine.store.snapshot().unwrap();
    let validator = NativeTools::new(root).unwrap();
    let report: DeliveryTypeDefinition = state
        .configuration("builtin-delivery-test-report:2")
        .unwrap()
        .decode()
        .unwrap();
    let doc: DeliveryTypeDefinition = state
        .configuration("builtin-delivery-test-recipe:2")
        .unwrap()
        .decode()
        .unwrap();
    let test_doc = json!({"scope":"AC-1 local addition","cases":[{"id":"AC-1","acceptance":["AC-1"],"expected":"2+3=5; -2+3=1; 0+0=0","files":["tests/add.test.cjs"]}],"recipe":{"runner":"node_test","files":["tests/add.test.cjs"],"timeout_seconds":30}});
    validator
        .validate_arguments(doc.content_schema.as_ref().unwrap(), &test_doc)
        .unwrap();
    let mut artifacts = vec![];
    for value in [&red, &green, &candidate_tests] {
        validator
            .validate_arguments(report.content_schema.as_ref().unwrap(), value)
            .unwrap();
        assert_eq!(value["origin"], "external");
        assert!(!value["execution"]["stdout"].as_str().unwrap().is_empty());
        let artifact = engine
            .artifacts
            .seal_scoped(
                &MaterialOwner::Project { id: project.into() },
                "delivery-acceptance",
                &serde_json::to_string(value).unwrap(),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&engine.artifacts.read(&artifact).unwrap()).unwrap(),
            *value
        );
        artifacts.push(artifact);
    }
    let patch = adapter.diff(&context, &base, &candidate).unwrap();
    assert!(patch.contains("+exports.add = (a, b) => a + b;"));
    assert_eq!(
        patch.trim(),
        git(
            &repo,
            &[
                "diff",
                "--binary",
                "--full-index",
                "--find-renames",
                &base,
                &candidate,
                "--"
            ]
        )
    );
    let diff = engine
        .artifacts
        .seal_scoped(
            &MaterialOwner::Project { id: project.into() },
            "delivery-acceptance",
            &patch,
        )
        .unwrap();
    assert_eq!(engine.artifacts.read(&diff).unwrap(), patch.as_bytes());
    assert_eq!(
        diff.sha256,
        format!("{:x}", Sha256::digest(patch.as_bytes()))
    );
    // Idempotent polling after persistence returns the same actual receipt.
    assert_eq!(
        tests.poll_test(&green_request).unwrap(),
        Some(green.clone())
    );
    for request in [&red_request, &green_request, &candidate_request] {
        tests.cleanup_test(request).unwrap();
    }
    println!(
        "Actual Docker: Red assertion failure, Green and candidate pass; 3 Test Reports validated; independent Test Doc and fixed-object Diff validated and persisted; no remote publish."
    );
}
