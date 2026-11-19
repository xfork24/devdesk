use devdesk_adapters::CliGit;
use devdesk_core::{id, ports::Git};
use devdesk_domain::*;
use serde_json::json;
use std::{fs, path::Path, process::Command};

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
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
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn discover(files: &[(&str, &str)]) -> ProjectTestDiscovery {
    let root = tempfile::tempdir().unwrap();
    let project = id();
    let repo = root
        .path()
        .join("projects")
        .join(&project)
        .join("repository");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    for (file, content) in files {
        let path = repo.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "--allow-empty", "-m", "Test manifests"]);
    let head = git(&repo, &["rev-parse", "HEAD"]);
    // Discovery must use the selected branch, not uncommitted files or another HEAD.
    git(&repo, &["checkout", "-b", "other"]);
    fs::write(
        repo.join("package.json"),
        r#"{"scripts":{"test":"exit 99"}}"#,
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "Unselected branch"]);
    let detection = CliGit::new(root.path())
        .unwrap()
        .discover_project_tests(&project, "main")
        .unwrap();
    detection.configuration.validate().unwrap();
    assert_eq!(detection.revision.as_deref(), Some(head.as_str()));
    assert!(
        detection
            .configuration
            .regression
            .as_ref()
            .is_none_or(|r| r.image.is_empty())
    );
    detection
}
#[test]
fn missing_repository_and_empty_repository_have_executable_default_recipes() {
    let root = tempfile::tempdir().unwrap();
    let adapter = CliGit::new(root.path()).unwrap();
    let detection = adapter.discover_project_tests(&id(), "main").unwrap();
    detection.configuration.validate().unwrap();
    assert_eq!(detection.source, "unconfigured");
    let empty = discover(&[]);
    assert_eq!(empty.configuration, ProjectTestConfiguration::default());
    assert!(empty.configuration.regression.is_none());
}
#[test]
fn package_test_script_is_read_without_running_it_and_placeholders_fall_back() {
    let result = discover(&[(
        "package.json",
        r#"{"scripts":{"test":"echo do-not-execute && node --test"}}"#,
    )]);
    assert_eq!(result.source, "repository");
    assert_eq!(result.files, ["package.json"]);
    assert_eq!(
        result
            .configuration
            .regression
            .unwrap()
            .command
            .unwrap()
            .argv,
        ["npm", "test"]
    );
    for content in [
        r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#,
        "{broken",
    ] {
        assert_eq!(
            discover(&[("package.json", content)]).source,
            "unconfigured"
        );
    }
}
#[test]
fn python_configuration_selects_unittest_or_existing_pytest() {
    let fallback = discover(&[("pyproject.toml", "[project]\nname = 'fixture'\n")]);
    assert_eq!(
        fallback.configuration.independent_runner,
        TestRunner::PythonUnittest
    );
    assert_eq!(
        fallback.configuration.independent_runner,
        TestRunner::PythonUnittest
    );
    let suite = discover(&[("tests/test_behavior.py", "import unittest")]);
    assert_eq!(suite.source, "repository");
    assert_eq!(
        suite.configuration.regression.unwrap().files,
        ["tests/test_behavior.py"]
    );
    let pytest = discover(&[(
        "pyproject.toml",
        "[tool.pytest.ini_options]\ntestpaths = ['tests']",
    )]);
    assert_eq!(
        pytest
            .configuration
            .regression
            .unwrap()
            .command
            .unwrap()
            .argv,
        ["python3", "-m", "pytest"]
    );
    assert_eq!(
        pytest.configuration.independent_runner,
        TestRunner::PythonUnittest
    );
}
#[test]
fn explicit_repository_configuration_overrides_conventions_without_machine_specific_images() {
    let configuration = ProjectTestConfiguration {
        regression: Some(TestRecipe {
            runner: TestRunner::NodeTest,
            files: vec!["tests/example.test.mjs".into()],
            image: format!("sha256:{}", "a".repeat(64)),
            timeout_seconds: 60,
            command: None,
            preparation: vec![],
        }),
        ..ProjectTestConfiguration::default()
    };
    let body = serde_json::to_string(&configuration).unwrap();
    let result = discover(&[
        (".devdesk/tests.json", &body),
        ("package.json", r#"{"scripts":{"test":"node --test"}}"#),
    ]);
    assert_eq!(result.files, [".devdesk/tests.json"]);
    assert_eq!(
        result
            .configuration
            .regression
            .as_ref()
            .unwrap()
            .timeout_seconds,
        60
    );
    assert_eq!(json!(result.configuration)["regression"].get("image"), None);
}
#[test]
fn common_compiled_project_manifests_supply_real_test_commands() {
    let inline = discover(&[
        ("Cargo.toml", "[package]\nname='inline-tests'"),
        ("src/lib.rs", "#[test] fn behavior() { assert_eq!(2+3,5); }"),
    ]);
    assert!(inline.configuration.regression.is_some());
    assert!(
        discover(&[("Cargo.toml", "[package]\nname=\"empty\"")])
            .configuration
            .regression
            .is_none()
    );
    assert!(
        discover(&[("go.mod", "module example.test/app")])
            .configuration
            .regression
            .is_none()
    );
    let rust = discover(&[
        ("Cargo.toml", "[workspace]\nmembers = []"),
        ("tests/example.rs", "#[test] fn works() {}"),
    ]);
    assert_eq!(
        rust.configuration.regression.unwrap().command.unwrap().argv,
        ["cargo", "test", "--workspace"]
    );
    assert!(!rust.notes.is_empty());
    let go = discover(&[
        ("go.mod", "module example.test/app"),
        ("app_test.go", "package app"),
    ]);
    assert_eq!(
        go.configuration.regression.unwrap().command.unwrap().argv,
        ["go", "test", "./..."]
    );
}
