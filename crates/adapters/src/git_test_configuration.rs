//! Read bounded, tracked manifests from the selected mainline. Never execute project scripts here.
use super::*;
use std::collections::BTreeMap;

impl CliGit {
    pub(crate) fn registered_revision(
        &self,
        project: &str,
        branch: &str,
    ) -> Result<Option<(PathBuf, String)>> {
        let location = self.project(project)?.join("repository");
        reject_symlink(&location)?;
        if !location.exists() {
            return Ok(None);
        }
        let repo = self.repository(project)?;
        self.output(&repo, &["check-ref-format", "--branch", branch])?;
        let reference = format!("refs/heads/{branch}");
        Ok(self
            .existing_ref(&repo, &reference)?
            .map(|revision| (repo, revision)))
    }

    pub(super) fn discover_tests(
        &self,
        project: &str,
        mainline: &str,
    ) -> Result<ProjectTestDiscovery> {
        let mut result = ProjectTestDiscovery::default();
        let Some((repo, revision)) = self.registered_revision(project, mainline)? else {
            return Ok(result);
        };
        result.revision = Some(revision.clone());
        let tree = self.output(&repo, &["ls-tree", "-r", "-z", "--full-tree", &revision])?;
        let entries: BTreeMap<_, _> = tree
            .split('\0')
            .filter_map(|entry| {
                let (metadata, path) = entry.split_once('\t')?;
                let mut metadata = metadata.split_whitespace();
                let mode = metadata.next()?;
                let kind = metadata.next()?;
                let oid = metadata.next()?;
                (matches!(mode, "100644" | "100755") && kind == "blob" && test_relative_path(path))
                    .then(|| (path.to_string(), oid.to_string()))
            })
            .collect();
        let mut manifests = BTreeMap::new();
        for path in [
            ".devdesk/tests.json",
            "package.json",
            "pyproject.toml",
            "pytest.ini",
            "setup.cfg",
            "Cargo.toml",
            "go.mod",
        ] {
            let Some(oid) = entries.get(path) else {
                continue;
            };
            let size = self
                .output(&repo, &["cat-file", "-s", oid])?
                .parse::<usize>()
                .map_err(storage)?;
            if size > 256 * 1024 {
                result
                    .notes
                    .push(format!("{path} 超出识别大小限制，使用默认测试入口。"));
                continue;
            }
            manifests.insert(
                path.to_string(),
                self.output(&repo, &["cat-file", "blob", oid])?,
            );
        }
        let rust_tests = if manifests.contains_key("Cargo.toml") {
            let found = self.git(
                &repo,
                &[
                    "grep",
                    "-q",
                    "-F",
                    "-e",
                    "#[test",
                    "-e",
                    "#[tokio::test",
                    "-e",
                    "#[async_std::test",
                    &revision,
                    "--",
                    "*.rs",
                ],
                None,
            )?;
            require(matches!(found.code, 0 | 1), "无法读取 Rust 测试声明")?;
            found.code == 0
        } else {
            false
        };
        detect(&manifests, &entries, rust_tests, &mut result)?;
        Ok(result)
    }
}

fn command(result: &mut ProjectTestDiscovery, file: &str, language: &str, argv: &[&str]) {
    result.source = "repository".into();
    result.files = vec![file.into()];
    result.notes.clear();
    result.configuration.language = language.into();
    result.configuration.regression = Some(TestRecipe {
        runner: TestRunner::Command,
        files: vec![file.into()],
        command: Some(TestCommand {
            argv: argv.iter().map(|s| (*s).into()).collect(),
            cwd: ".".into(),
            environment: Default::default(),
        }),
        image: String::new(),
        timeout_seconds: 1800,
        preparation: vec![],
    });
}

fn detect(
    manifests: &BTreeMap<String, String>,
    entries: &BTreeMap<String, String>,
    rust_tests: bool,
    result: &mut ProjectTestDiscovery,
) -> Result<()> {
    if let Some(body) = manifests.get(".devdesk/tests.json") {
        match serde_json::from_str::<ProjectTestConfiguration>(body).and_then(|configuration| {
            configuration.validate().map_err(serde::de::Error::custom)?;
            Ok(configuration)
        }) {
            Ok(mut configuration) => {
                // Runtime identity belongs to the selected Sandbox, not a machine-specific repository file.
                if let Some(regression) = configuration.regression.as_mut() {
                    regression.image.clear();
                }
                result.configuration = configuration;
                result.source = "repository".into();
                result.files = vec![".devdesk/tests.json".into()];
                result.notes.clear();
                return Ok(());
            }
            Err(error) => {
                return Err(err(
                    "invalid_configuration",
                    format!(".devdesk/tests.json 格式无效：{error}"),
                ));
            }
        }
    }
    if let Some(body) = manifests.get("package.json") {
        match serde_json::from_str::<Value>(body) {
            Ok(package) => {
                if package["scripts"]["test"].as_str().is_some_and(|script| {
                    !script.trim().is_empty() && !script.contains("no test specified")
                }) {
                    command(
                        result,
                        "package.json",
                        "JavaScript / TypeScript / Node.js",
                        &["npm", "test"],
                    );
                    result
                        .notes
                        .push("项目依赖需在运行环境中准备；识别过程不会执行仓库脚本。".into());
                    return Ok(());
                }
            }
            Err(_) => result
                .notes
                .push("package.json 格式无效，使用可识别的其他入口。".into()),
        }
    }
    if let Some(cargo) = manifests.get("Cargo.toml") {
        result.configuration.language = "Rust".into();
        if !rust_tests
            && !cargo.contains("[[test]]")
            && !entries
                .keys()
                .any(|p| p.ends_with(".rs") && p.split('/').any(|part| part == "tests"))
        {
            return Ok(());
        }
        command(
            result,
            "Cargo.toml",
            "Rust",
            &["cargo", "test", "--workspace"],
        );
        result
            .notes
            .push("运行环境需要 Rust 工具链与项目依赖。".into());
        return Ok(());
    }
    if manifests.contains_key("go.mod") {
        result.configuration.language = "Go".into();
        if !entries.keys().any(|p| p.ends_with("_test.go")) {
            return Ok(());
        }
        command(result, "go.mod", "Go", &["go", "test", "./..."]);
        result
            .notes
            .push("运行环境需要 Go 工具链与项目依赖。".into());
        return Ok(());
    }
    let python = manifests.contains_key("pytest.ini")
        || manifests.contains_key("pyproject.toml")
        || manifests.contains_key("setup.cfg")
        || entries.keys().any(|p| p.ends_with(".py"));
    if python {
        result.configuration.language = "Python 3".into();
        result.configuration.independent_runner = TestRunner::PythonUnittest;
        let pytest = manifests.contains_key("pytest.ini")
            || manifests
                .get("pyproject.toml")
                .is_some_and(|s| s.contains("[tool.pytest.ini_options]"))
            || manifests
                .get("setup.cfg")
                .is_some_and(|s| s.contains("[tool:pytest]"));
        if pytest {
            let file = ["pytest.ini", "pyproject.toml", "setup.cfg"]
                .into_iter()
                .find(|p| manifests.contains_key(*p))
                .unwrap();
            command(result, file, "Python 3", &["python3", "-m", "pytest"]);
            result.notes.push("运行环境需要 pytest 与项目依赖。".into());
            return Ok(());
        }
        let files: Vec<_> = entries
            .keys()
            .filter(|p| {
                let name = p.rsplit('/').next().unwrap_or(p);
                name.starts_with("test")
                    && name.ends_with(".py")
                    && (p.starts_with("tests/") || !p.contains('/'))
            })
            .take(100)
            .cloned()
            .collect();
        if !files.is_empty() {
            result.configuration.regression = Some(TestRecipe {
                runner: TestRunner::PythonUnittest,
                files: files.clone(),
                image: String::new(),
                timeout_seconds: 1800,
                command: None,
                preparation: vec![],
            });
            result.source = "repository".into();
            result.files = files;
            result.notes = vec!["自动发现 Python unittest 测试文件。".into()];
        }
    }
    Ok(())
}
