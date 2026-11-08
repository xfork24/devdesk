use devdesk_adapters::{CliGit, CommandTests};
use devdesk_core::{TestRequest, ports::Tests};
use devdesk_domain::*;
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
mod support;
fn git(p: &Path, args: &[&str]) -> String {
    let r = Command::new("git")
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@localhost",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-C",
        ])
        .arg(p)
        .args(args)
        .output()
        .unwrap();
    assert!(r.status.success());
    String::from_utf8(r.stdout).unwrap().trim().into()
}
fn git_fixture(root: &Path, body: &str) -> (GitContext, String) {
    let project = id();
    let repo = root.join("projects").join(&project).join("repository");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::create_dir(repo.join("tests")).unwrap();
    fs::write(repo.join("tests/actual.test.mjs"), body).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "Frozen test fixture"]);
    let candidate = git(&repo, &["rev-parse", "HEAD"]);
    let code = GitContext {
        project: project.clone(),
        workspace: Workspace {
            repository: "fixture/local".into(),
            path: fs::canonicalize(&repo).unwrap().to_string_lossy().into(),
            mainline: "main".into(),
        },
        source: CodeLocation {
            owner: id(),
            branch: "main".into(),
            worktree: repo.to_string_lossy().into(),
        },
        target: CodeLocation {
            owner: project,
            branch: "main".into(),
            worktree: repo.to_string_lossy().into(),
        },
        origin: Origin::External,
    };
    (code, candidate)
}
fn fixture(root: &Path, body: &str, phase: &str) -> TestRequest {
    let (code, candidate) = git_fixture(root, body);
    let document = Document {
        id: id(),
        project_id: None,
        feature_id: Some(id()),
        task_id: Some(code.source.owner.clone()),
        kind: "test".into(),
        title: "Actual test fixture".into(),
        body: "Node assert fixture".into(),
        version: 1,
        supersedes: None,
        requirement_id: None,
        design_id: None,
        origin: Origin::Human,
        acceptance: vec![],
        verifies: vec![],
        artifact_id: None,
        test_recipe: Some(TestRecipe {
            runner: TestRunner::NodeTest,
            files: vec!["tests/actual.test.mjs".into()],
            image: support::image("codex"),
            timeout_seconds: 30,
        }),
    };
    let operation = id();
    let environment = id();
    TestRequest {
        operation_id: operation.clone(),
        session_id: Some(id()),
        phase: phase.into(),
        candidate,
        test_definitions: vec![document.id.clone()],
        environment: environment.clone(),
        origin: Origin::External,
        definitions: vec![document],
        code: Some(code),
        runtime: Some(ExecutionInstance {
            id: environment,
            run_id: operation,
            harness: Harness {
                id: "node-test-fixture".into(),
                provider: "node-test".into(),
                version: "24.13.0".into(),
                capabilities: vec!["structured-test-report".into()],
                origin: Origin::External,
            },
            sandbox: Sandbox {
                id: "docker-fixture".into(),
                provider: "docker".into(),
                version: "fixture".into(),
                platform: support::platform("codex"),
                image: Some(support::image("codex")),
                network: false,
                origin: Origin::External,
            },
            external_session: None,
            external_environment: None,
            status: Status::Prepared,
            started: None,
            finished: None,
        }),
    }
}
fn finish(tests: &CommandTests, r: &TestRequest) -> Value {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if let Some(result) = tests.poll_test(r).unwrap() {
            return result;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(100));
    }
}
#[test]
fn export_rejects_content_drift_without_touching_host_files_outside_the_execution_root() {
    let root = tempfile::tempdir().unwrap();
    let (code, candidate) = git_fixture(root.path(), "export const x=1;");
    let path = root
        .path()
        .join("projects")
        .join(&code.project)
        .join("execution/tests/fixture");
    fs::create_dir_all(&path).unwrap();
    let a = CliGit::new(root.path()).unwrap();
    let path = fs::canonicalize(path).unwrap().join("work");
    a.export_revision(&code, &candidate, &path).unwrap();
    a.export_revision(&code, &candidate, &path).unwrap();
    fs::write(path.join("tests/actual.test.mjs"), "changed").unwrap();
    assert!(a.export_revision(&code, &candidate, &path).is_err());
    assert!(
        a.export_revision(&code, &candidate, &root.path().join("outside"))
            .is_err()
    );
}
#[test]
#[ignore = "requires the pinned Linux Node image and actual Docker daemon"]
fn actual_node_red_green_environment_failure_and_stopped_receipts() {
    let root = tempfile::tempdir().unwrap();
    let tests = CommandTests::new(root.path()).unwrap();
    for (phase, body, status, expected, failure) in [
        (
            "red",
            "import {test} from 'node:test'; import assert from 'node:assert/strict'; test('red',()=>assert.equal(2+2,5));",
            "succeeded",
            true,
            None,
        ),
        (
            "green",
            "import {test} from 'node:test'; import assert from 'node:assert/strict'; test('green',()=>assert.equal(2+3,5));",
            "succeeded",
            false,
            None,
        ),
        (
            "red",
            "import './missing-implementation.mjs';",
            "failed",
            false,
            Some("environment"),
        ),
        (
            "green",
            "this is invalid syntax !",
            "failed",
            false,
            Some("environment"),
        ),
    ] {
        let r = fixture(root.path(), body, phase);
        let result = finish(&tests, &r);
        assert_eq!(result["status"], status, "{result}");
        assert_eq!(result["expected_failure"], expected, "{result}");
        assert_eq!(result["failure_type"].as_str(), failure, "{result}");
        assert_eq!(result["candidate"], r.candidate);
        assert_eq!(result["image_digest"], support::image("codex"));
        assert_eq!(result["writer_revoked"], true);
        assert_eq!(tests.poll_test(&r).unwrap().unwrap(), result);
        tests.cleanup(&r).unwrap();
        tests.cleanup(&r).unwrap();
        assert_eq!(tests.poll_test(&r).unwrap().unwrap(), result);
    }
    let r = fixture(
        root.path(),
        "import {test} from 'node:test';test('wait',async()=>await new Promise(r=>setTimeout(r,30000)));",
        "green",
    );
    assert!(tests.poll_test(&r).unwrap().is_none());
    assert!(tests.stop_test(&r).unwrap());
    assert!(tests.poll_test(&r).is_err());
    tests.cleanup(&r).unwrap();
}
