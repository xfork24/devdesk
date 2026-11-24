use devdesk_adapters::CliGit;
use devdesk_core::{id, ports::Git};
use devdesk_domain::*;
use serde_json::json;
use std::{fs, path::Path, process::Command};

// Exercise the exact historical runtime with no Web build dependency.
const SOFTWARE_DOMAIN_FIXTURE: &[u8] =
    include_bytes!("../../../tests/fixtures/domain-packages/software-development-0.3.0.zip");

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
fn repository_fixture(
    files: &[(&str, &str)],
) -> (tempfile::TempDir, Id, std::path::PathBuf, String) {
    let root = tempfile::tempdir().unwrap();
    let project = id();
    let (repo, head) = seed_repository(root.path(), &project, files);
    (root, project, repo, head)
}

fn seed_repository(
    root: &Path,
    project: &str,
    files: &[(&str, &str)],
) -> (std::path::PathBuf, String) {
    let repo = root.join("projects").join(project).join("repository");
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
    (repo, head)
}

fn discover(files: &[(&str, &str)]) -> ProjectTestDiscovery {
    let (root, project, _, head) = repository_fixture(files);
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

#[test]
#[ignore = "explicit actual Docker domain worker and isolated Git repositories"]
fn software_domain_worker_matches_legacy_test_discovery() {
    use devdesk_adapters::domain_packages::DomainPackages;

    let temporary = tempfile::tempdir().unwrap();
    devdesk_adapters::seed_factory_configuration(&mut EngineeringState::default());
    let packages = DomainPackages::open(temporary.path()).unwrap();
    let installed = packages.install(SOFTWARE_DOMAIN_FIXTURE).unwrap();
    let digest = installed["digest"].as_str().unwrap();
    let epoch = packages
        .activate(digest, 0, |_| Ok(()), |_| Ok(()))
        .unwrap();
    let cases: &[&[(&str, &str)]] = &[
        &[],
        &[("package.json", r#"{"scripts":{"test":"node --test"}}"#)],
        &[
            ("Cargo.toml", "[package]\nname = 'example'"),
            ("src/lib.rs", "#[test] fn works() {}"),
        ],
        &[
            ("go.mod", "module devdesk.example/probe"),
            ("app_test.go", "package probe"),
        ],
        &[("pytest.ini", "[pytest]")],
        &[("tests/test_x.py", "def test_example(): pass")],
        &[(
            ".devdesk/tests.json",
            r#"{"language":"Custom","independent_runner":"node_test","regression":null}"#,
        )],
    ];
    for files in cases {
        let (root, project, repo, head) = repository_fixture(files);
        let baseline = CliGit::new(root.path())
            .unwrap()
            .discover_project_tests(&project, "main")
            .unwrap();
        let result = packages
            .invoke(
                epoch,
                "software-development.tests.discover",
                json!({"path":repo,"revision":head}),
            )
            .unwrap_or_else(|error| panic!("software domain failed on {files:?}: {error}"));
        let actual: ProjectTestDiscovery = serde_json::from_value(result).unwrap();
        actual.configuration.validate().unwrap();
        assert_eq!(json!(actual), json!(baseline), "{files:?}");
    }
}

#[cfg(feature = "test-fixtures")]
#[test]
#[ignore = "explicit actual Docker domain worker and private local Git worktrees"]
fn software_domain_branch_is_frozen_before_git_writes_and_inspected_without_resend() {
    use devdesk_adapters::{FileArtifacts, SqliteStore, domain_packages::DomainPackages};
    use devdesk_core::{
        Command as EngineCommand, Engine, EngineeringStore, concurrency::ResourceKey,
        ports::DomainReader,
    };
    use serde_json::Value;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct PackageReader {
        packages: DomainPackages,
    }
    impl DomainReader for PackageReader {
        fn read(&self, project: &str, handler: &str, input: Value) -> Result<Value> {
            self.packages.read_for_project(project, handler, input)
        }
        fn worktree_branch(
            &self,
            project: &str,
            owner: &str,
            kind: WorktreeKind,
        ) -> Result<Option<String>> {
            self.packages.worktree_branch(project, owner, kind)
        }
        fn project_test_discovery(
            &self,
            project: &str,
            mainline: &str,
        ) -> Result<Option<ProjectTestDiscovery>> {
            self.packages.project_test_discovery(project, mainline)
        }
    }

    struct LostReplyGit {
        actual: CliGit,
        writes: AtomicUsize,
        reads: AtomicUsize,
    }
    impl Git for LostReplyGit {
        fn repository_operation(&self, op: &RepositoryOperation) -> Result<RepositoryReceipt> {
            self.writes.fetch_add(1, Ordering::Relaxed);
            let result = self.actual.repository_operation(op)?;
            if matches!(
                op.action,
                RepositoryAction::CreateWorktree {
                    worktree_kind: WorktreeKind::Feature,
                    ..
                }
            ) {
                return Err(err(
                    "transport_interrupted",
                    "lost reply after successful Git write",
                )
                .technical_unknown());
            }
            Ok(result)
        }
        fn inspect_repository_operation(
            &self,
            op: &RepositoryOperation,
        ) -> Result<Option<RepositoryReceipt>> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            self.actual.inspect_repository_operation(op)
        }
        fn candidate(
            &self,
            context: &GitContext,
            source: &str,
            target: &str,
            op: &str,
        ) -> Result<String> {
            self.actual.candidate(context, source, target, op)
        }
        fn advance(
            &self,
            context: &GitContext,
            actual: &str,
            expected: &str,
            candidate: &str,
            op: &str,
        ) -> Result<String> {
            self.actual
                .advance(context, actual, expected, candidate, op)
        }
    }

    let root = tempfile::tempdir().unwrap();
    let project = id();
    let store = Arc::new(SqliteStore::open_project(root.path(), &project).unwrap());
    let (repository, revision) = seed_repository(
        root.path(),
        &project,
        &[
            ("README", "Worktree base"),
            ("package.json", r#"{"scripts":{"test":"node --test"}}"#),
        ],
    );
    let other_branch = git(&repository, &["rev-parse", "HEAD"]);
    let git_effects = Arc::new(LostReplyGit {
        actual: CliGit::new(root.path()).unwrap(),
        writes: AtomicUsize::new(0),
        reads: AtomicUsize::new(0),
    });
    let mut adapters = devdesk_adapters::execution_adapters(root.path()).unwrap();
    adapters.git = git_effects.clone();
    let mut engine = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(store.root().into())),
        adapters,
    );
    engine.bootstrap().unwrap();
    engine
        .execute(
            "project.create",
            EngineCommand {
                command_id: id(),
                target: None,
                expected_revision: None,
                payload: json!({"title":"Physical package worktree"}),
            },
        )
        .unwrap();
    let packages = DomainPackages::open(root.path()).unwrap();
    let installed = packages.install(SOFTWARE_DOMAIN_FIXTURE).unwrap();
    let digest = installed["digest"].as_str().unwrap();
    packages
        .activate(digest, 0, |_| Ok(()), |_| Ok(()))
        .unwrap();
    packages.bind_project(&project, digest).unwrap();
    engine.domain_reader = Some(Arc::new(PackageReader { packages }));
    let discovery = engine.discover_project_tests().unwrap();
    assert_eq!(discovery.revision.as_deref(), Some(revision.as_str()));
    assert_eq!(
        discovery
            .configuration
            .regression
            .unwrap()
            .command
            .unwrap()
            .argv,
        ["npm", "test"]
    );

    let feature = id();
    let task = id();
    store
        .seed_fixture_state(None, &mut |state| {
            state.features.push(Feature {
                id: feature.clone(),
                project_id: project.clone(),
                title: "Private feature".into(),
                status: Status::Draft,
                revision: 1,
                input_version: 1,
                predecessor: None,
                source_materials: vec![],
            });
            state.tasks.push(Task {
                id: task.clone(),
                feature_id: feature.clone(),
                title: "Private task".into(),
                status: Status::Draft,
                revision: 1,
                inherited_snapshot: None,
                dependencies: vec![],
                worktree: String::new(),
                scope: Default::default(),
            });
            for (owner, kind) in [
                (&feature, WorktreeKind::Feature),
                (&task, WorktreeKind::Task),
            ] {
                state.repository_operations.push(RepositoryOperation {
                    id: id(),
                    project_id: project.clone(),
                    action: RepositoryAction::CreateWorktree {
                        owner: owner.clone(),
                        worktree_kind: kind,
                        base: revision.clone(),
                        branch: None,
                    },
                    status: RepositoryOperationStatus::Prepared,
                    receipt: None,
                    reason: String::new(),
                    revision: 1,
                });
            }
            Ok(Value::Null)
        })
        .unwrap();
    for _ in 0..6 {
        engine.tick_repositories().unwrap();
    }
    let pending = store
        .snapshot()
        .unwrap()
        .repository_operations
        .into_iter()
        .find(|op| op.status == RepositoryOperationStatus::NeedsReconciliation)
        .expect("the applied Git effect lost its reply");
    let busy = ResourceKey::new("domain_project_busy", &project);
    assert_eq!(
        store.read_resources(std::slice::from_ref(&busy)).unwrap()[0].value,
        Some(json!({"active":true}))
    );
    assert_eq!(git_effects.writes.load(Ordering::Relaxed), 2);
    assert_eq!(git_effects.reads.load(Ordering::Relaxed), 0);
    engine
        .execute(
            "repository.reconcile",
            EngineCommand {
                command_id: id(),
                target: Some(pending.id),
                expected_revision: Some(pending.revision),
                payload: json!({}),
            },
        )
        .unwrap();
    for _ in 0..3 {
        engine.tick_repositories().unwrap();
    }
    assert_eq!(git_effects.writes.load(Ordering::Relaxed), 2);
    assert_eq!(git_effects.reads.load(Ordering::Relaxed), 1);
    assert_eq!(
        store.read_resources(&[busy]).unwrap()[0].value,
        Some(json!({"active":false}))
    );
    let stored = store.snapshot().unwrap();
    assert_eq!(stored.repository_operations.len(), 2);
    assert_eq!(stored.repository_worktrees.len(), 2);
    let git_adapter = CliGit::new(root.path()).unwrap();
    for operation in &stored.repository_operations {
        assert_eq!(
            operation.status,
            RepositoryOperationStatus::Completed,
            "{}",
            operation.reason
        );
        let RepositoryAction::CreateWorktree {
            branch: Some(branch),
            owner,
            ..
        } = &operation.action
        else {
            panic!("package branch was not persisted with the physical grant");
        };
        let kind = if owner == &feature {
            "features"
        } else {
            "tasks"
        };
        assert_eq!(branch, &format!("devdesk/{kind}/{owner}"));
        let RepositoryReceipt::Worktree { worktree } = git_adapter
            .inspect_repository_operation(operation)
            .unwrap()
            .expect("original physical receipt")
        else {
            panic!("unexpected Git receipt");
        };
        assert_eq!(worktree.branch, *branch);
        assert_eq!(
            git(Path::new(&worktree.path), &["rev-parse", "HEAD"]),
            revision
        );
    }
    assert_eq!(git(&repository, &["rev-parse", "HEAD"]), other_branch);
}
