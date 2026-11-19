use devdesk_adapters::{CliGit, CommandTests, configuration_digest};
use devdesk_core::id;
use devdesk_core::{TestRequest, ports::Tests};
use devdesk_domain::*;
use serde_json::Value;
use sha2::Digest;
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
    let recipe = TestRecipe {
        command: None,
        preparation: vec![],
        runner: TestRunner::NodeTest,
        files: vec!["tests/actual.test.mjs".into()],
        image: support::image("codex"),
        timeout_seconds: 30,
    };
    let operation = id();
    let environment = id();
    TestRequest {
        expect_assertion_failure: phase == "red",
        recipe: Some(recipe.clone()),
        operation_id: operation.clone(),
        session_id: Some(id()),

        candidate,

        environment: environment.clone(),
        origin: Origin::External,

        code: Some(code),
        runtime: Some(ExecutionInstance {
            id: environment,
            operation_id: operation,
            harness: Harness {
                permission: None,
                id: "node-test-fixture".into(),
                provider: "node-test".into(),
                version: "24.13.0".into(),
                capabilities: vec!["structured-test-report".into()],
                origin: Origin::External,
            },
            sandbox: Sandbox {
                permission: None,
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
            "red",
            "import {test} from 'node:test'; import assert from 'node:assert/strict'; test('missing module',async()=>{try{await import('./missing-implementation.mjs');}catch(error){assert.fail('Expected missing implementation Red: '+error.code+': '+error.message);}});",
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
        let raw = &result["execution"];
        assert_eq!(raw["stdout"], result["report"]);
        assert_eq!(raw["exit_code"], result["exit_code"]);
        if body.contains("Expected missing implementation Red") {
            assert!(
                raw["stdout"]
                    .as_str()
                    .unwrap()
                    .contains("Expected missing implementation Red: ERR_MODULE_NOT_FOUND")
            );
            assert!(
                raw["stdout"]
                    .as_str()
                    .unwrap()
                    .contains("Cannot find module")
            );
            assert!(raw["stdout"].as_str().unwrap().lines().any(|line| {
                let record: Value = serde_json::from_str(line).unwrap();
                record["diagnostic"]["details"]["error"]["cause"]["stack"]
                    .as_str()
                    .is_some_and(|stack| stack.contains("/work/tests/actual.test.mjs"))
            }));
        }
        assert!(raw["started_at"].as_str().unwrap().starts_with("unix_ms:"));
        assert_eq!(raw["files"][0]["content"], body);
        assert_eq!(
            raw["files"][0]["sha256"],
            format!("{:x}", sha2::Sha256::digest(body.as_bytes()))
        );
        assert_eq!(tests.poll_test(&r).unwrap().unwrap(), result);
        tests.cleanup(&r).unwrap();
        tests.cleanup(&r).unwrap();
        assert_eq!(tests.poll_test(&r).unwrap().unwrap(), result);
    }
    let mut missing = fixture(root.path(), "// real source remains present", "green");
    missing.recipe.as_mut().unwrap().files = vec!["missing.test.mjs".into()];
    let result = finish(&tests, &missing);
    assert_eq!(result["failure_type"], "environment");
    assert!(result["execution"]["files"].as_array().unwrap().is_empty());
    assert!(
        result["execution"]["stderr"]
            .as_str()
            .unwrap()
            .contains("missing.test.mjs")
    );
    tests.cleanup(&missing).unwrap();
    // Preparation and delayed observation must not consume the container's
    // execution budget. Keep the old coordinator clock, run a real fresh job.
    let delayed = fixture(
        root.path(),
        "import {test} from 'node:test';test('late observation',()=>{});",
        "green",
    );
    let job = root
        .path()
        .join("projects")
        .join(&delayed.code.as_ref().unwrap().project)
        .join("execution/tests")
        .join(&delayed.operation_id);
    fs::create_dir_all(&job).unwrap();
    fs::write(
        job.join("job.json"),
        serde_json::to_vec(&serde_json::json!({
            "request_digest": configuration_digest(&serde_json::json!(delayed)).unwrap(),
            "started": 1, "start_dispatched": false
        }))
        .unwrap(),
    )
    .unwrap();
    let result = finish(&tests, &delayed);
    assert_eq!(result["status"], "succeeded", "{result}");
    tests.cleanup(&delayed).unwrap();

    // An exited container which actually exceeded the limit still fails even
    // when all assertions passed and its process returned exit code zero.
    let mut overrun = fixture(
        root.path(),
        "import {test} from 'node:test';test('overrun',async()=>await new Promise(r=>setTimeout(r,1600)));",
        "green",
    );
    overrun.recipe.as_mut().unwrap().timeout_seconds = 1;
    assert!(tests.poll_test(&overrun).unwrap().is_none());
    std::thread::sleep(Duration::from_secs(2));
    let result = finish(&tests, &overrun);
    assert_eq!(result["failure_type"], "timeout", "{result}");
    assert_eq!(result["status"], "failed");
    assert_eq!(result["exit_code"], 0);
    assert_eq!(result["counts"]["passed"], 1);
    tests.cleanup(&overrun).unwrap();
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

#[test]
#[ignore = "requires the pinned Linux Node image and actual Docker daemon"]
fn actual_read_only_observation_preserves_fast_test_completion() {
    let root = tempfile::tempdir().unwrap();
    let tests = CommandTests::new(root.path()).unwrap();
    for _ in 0..16 {
        let request = fixture(
            root.path(),
            "import {test} from 'node:test';test('fast exit',async()=>await new Promise(r=>setTimeout(r,20)));",
            "green",
        );
        let mut result = tests.poll_test(&request).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while result.is_none() {
            match tests.inspect_test(&request).unwrap() {
                devdesk_core::ports::TestExecutionObservation::Running { timed_out: false } => {}
                devdesk_core::ports::TestExecutionObservation::Completed { result: observed } => {
                    result = Some(observed)
                }
                other => panic!("An already granted fast test lost its physical result: {other:?}"),
            }
            assert!(Instant::now() < deadline);
        }
        let result = result.unwrap();
        assert_eq!(result["operation_id"], request.operation_id);
        assert_eq!(result["candidate"], request.candidate);
        assert_eq!(result["status"], "succeeded");
        assert_eq!(result["counts"]["passed"], 1);
        tests.cleanup(&request).unwrap();
        assert!(tests.inspect_test_cleanup(&request).unwrap());
    }
}

#[test]
fn python_assertions_environment_errors_and_project_commands_have_real_receipts() {
    let root = tempfile::tempdir().unwrap();
    let tests = CommandTests::new(root.path()).unwrap();
    for (body, red, expected, failure) in [
        (
            "import unittest\nclass Behavior(unittest.TestCase):\n def test_value(self): self.assertEqual(1,2)\n",
            true,
            true,
            None,
        ),
        (
            "import unittest\nclass Behavior(unittest.TestCase):\n def test_value(self): raise RuntimeError('missing dependency')\n",
            true,
            false,
            Some("environment"),
        ),
        (
            "import unittest\nclass Behavior(unittest.TestCase):\n def test_value(self):\n  print('{\"devdesk_test_report\":1,\"event\":\"fail\",\"classification\":\"assertion\"}')\n  self.assertEqual(1,1)\n",
            false,
            true,
            None,
        ),
    ] {
        let mut request = fixture(root.path(), "", "green");
        let repo = Path::new(&request.code.as_ref().unwrap().source.worktree);
        fs::write(repo.join("tests/actual.py"), body).unwrap();
        git(repo, &["add", "."]);
        git(repo, &["commit", "-m", "Python behavior"]);
        request.candidate = git(repo, &["rev-parse", "HEAD"]);
        request.expect_assertion_failure = red;
        request.runtime.as_mut().unwrap().sandbox.image = Some(support::image("project"));
        let recipe = request.recipe.as_mut().unwrap();
        recipe.image = support::image("project");
        recipe.runner = TestRunner::PythonUnittest;
        recipe.files = vec!["tests/actual.py".into()];
        let result = finish(&tests, &request);
        assert_eq!(result["status"] == "succeeded", expected, "{result}");
        assert_eq!(result["failure_type"].as_str(), failure);
        assert_eq!(result["expected_failure"], red && expected);
        assert_eq!(result["execution"]["termination"]["timed_out"], false);
        assert!(result["execution"]["recipe_digest"].as_str().unwrap().len() == 64);
        assert!(matches!(
            tests.inspect_test(&request).unwrap(),
            devdesk_core::ports::TestExecutionObservation::Completed { .. }
        ));
        tests.cleanup_test(&request).unwrap();
    }
    let mut request = fixture(root.path(), "", "green");
    request.runtime.as_mut().unwrap().sandbox.image = Some(support::image("project"));
    let recipe = request.recipe.as_mut().unwrap();
    recipe.image = support::image("project");
    recipe.runner = TestRunner::Command;
    recipe.command = Some(TestCommand { argv:vec!["python3".into(),"-c".into(),"from pathlib import Path; assert Path('/state/prepared').read_text() == 'ready'; print('actual project regression')".into()],cwd:".".into(),environment:Default::default() });
    recipe.preparation = vec![TestCommand {
        argv: vec![
            "python3".into(),
            "-c".into(),
            "from pathlib import Path; Path('/state/prepared').write_text('ready')".into(),
        ],
        cwd: ".".into(),
        environment: Default::default(),
    }];
    recipe.timeout_seconds = 1800;
    let result = finish(&tests, &request);
    assert_eq!(result["status"], "succeeded", "{result}");
    assert!(
        result["execution"]["stdout"]
            .as_str()
            .unwrap()
            .contains("actual project regression")
    );
    tests.cleanup_test(&request).unwrap();
    request.operation_id = id();
    request.runtime.as_mut().unwrap().operation_id = request.operation_id.clone();
    request.expect_assertion_failure = true;
    assert!(
        tests
            .poll_test(&request)
            .unwrap_err()
            .message
            .contains("Red")
    );
}

#[test]
#[ignore = "requires the fixed Docker Sandbox with Python 3"]
fn actual_python_regression_and_missing_command_environment_keep_real_results() {
    for (runner, body, argv, expected) in [
        (
            TestRunner::PythonUnittest,
            "import unittest\nclass Regression(unittest.TestCase):\n def test_behavior(self): self.assertEqual(2+3,5)\n",
            None,
            "succeeded",
        ),
        (
            TestRunner::PythonUnittest,
            "import devdesk_missing_dependency\n",
            None,
            "failed",
        ),
        (
            TestRunner::Command,
            "",
            Some(vec!["python3", "-c", "import devdesk_missing_dependency"]),
            "failed",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut request = fixture(root.path(), "export const fixture = true;", "green");
        let repo = Path::new(&request.code.as_ref().unwrap().workspace.path);
        fs::write(repo.join("tests/test_runtime.py"), body).unwrap();
        git(repo, &["add", "."]);
        git(repo, &["commit", "-m", "Project regression"]);
        request.candidate = git(repo, &["rev-parse", "HEAD"]);
        let recipe = request.recipe.as_mut().unwrap();
        recipe.runner = runner;
        recipe.files = vec!["tests/test_runtime.py".into()];
        recipe.command = argv.map(|args| TestCommand {
            argv: args.into_iter().map(str::to_owned).collect(),
            cwd: ".".into(),
            environment: Default::default(),
        });
        let tests = CommandTests::new(root.path()).unwrap();
        let result = finish(&tests, &request);
        assert_eq!(result["status"], expected, "{result}");
        if expected == "failed" {
            assert_eq!(result["failure_type"], "environment");
            assert_eq!(result["expected_failure"], false);
        }
        tests.cleanup_test(&request).unwrap();
    }
}
