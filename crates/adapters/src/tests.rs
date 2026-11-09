//! Credential-free, fixed-revision tests in owned Docker containers. Never infer test success from LLM output.
use super::*;
use crate::docker::{ContainerPurpose, ContainerSpec, DockerSandbox};
use std::time::{SystemTime, UNIX_EPOCH};
fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
#[derive(Debug, Serialize, serde::Deserialize)]
struct Job {
    request_digest: String,
    started: u64,
    start_dispatched: bool,
}
pub struct CommandTests {
    root: PathBuf,
    writer: Mutex<()>,
}
impl CommandTests {
    pub fn new(root: &Path) -> Result<Self> {
        Ok(Self {
            root: fs::canonicalize(root).map_err(storage)?,
            writer: Mutex::new(()),
        })
    }
    fn paths(&self, r: &TestRequest) -> Result<(PathBuf, PathBuf)> {
        let c = r
            .code
            .as_ref()
            .ok_or_else(|| err("blocked", "Test 缺少冻结代码定位"))?;
        require(
            c.origin == Origin::External && r.origin == Origin::External,
            "CommandTests 拒绝模拟证据",
        )?;
        require(
            uuid::Uuid::parse_str(&r.operation_id).is_ok(),
            "测试 operation 必须是 UUID",
        )?;
        let root = SqliteStore::project_path(&self.root, &c.project)?.join("execution");
        let parent = root.join("tests");
        fs::create_dir_all(&parent).map_err(storage)?;
        reject_symlink(&parent)?;
        let job = parent.join(&r.operation_id);
        reject_symlink(&job)?;
        fs::create_dir_all(&job).map_err(storage)?;
        Ok((root, job))
    }
    fn recipe(&self, r: &TestRequest) -> Result<TestRecipe> {
        require(
            !r.test_definitions.is_empty() && r.definitions.len() == r.test_definitions.len(),
            "测试定义不完整",
        )?;
        let mut recipe = None;
        let mut ids = std::collections::HashSet::new();
        for d in &r.definitions {
            require(
                d.kind == "test" && r.test_definitions.contains(&d.id) && ids.insert(&d.id),
                "测试定义引用冲突",
            )?;
            let next = d
                .test_recipe
                .as_ref()
                .ok_or_else(|| err("configuration_required", "Test 定义缺少可执行 TestRecipe"))?;
            next.validate()?;
            if let Some(current) = &recipe {
                require(current == next, "同一 Run 的测试定义配方不一致")?;
            }
            recipe = Some(next.clone());
        }
        Ok(recipe.unwrap())
    }
    fn write(path: &Path, value: &impl Serialize) -> Result<()> {
        let bytes = serde_json::to_vec(value).map_err(storage)?;
        let temporary = path.with_extension("pending");
        reject_symlink(path)?;
        reject_symlink(&temporary)?;
        let mut options = OpenOptions::new();
        options.write(true).truncate(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        use std::io::Write;
        let mut file = options.open(&temporary).map_err(storage)?;
        file.write_all(&bytes).map_err(storage)?;
        file.sync_all().map_err(storage)?;
        fs::rename(temporary, path).map_err(storage)
    }
    fn read(path: &Path) -> Result<Option<Value>> {
        reject_symlink(path)?;
        if !path.exists() {
            return Ok(None);
        }
        serde_json::from_slice(&fs::read(path).map_err(storage)?)
            .map(Some)
            .map_err(storage)
    }
    pub fn poll(&self, r: &TestRequest) -> Result<Option<Value>> {
        let _lock = self.writer.lock().map_err(storage)?;
        let recipe = self.recipe(r)?;
        let (root, job) = self.paths(r)?;
        let digest = configuration_digest(&json!(r))?;
        let metadata = job.join("job.json");
        let mut state: Job = if let Some(value) = Self::read(&metadata)? {
            let state: Job = serde_json::from_value(value).map_err(storage)?;
            require(
                state.request_digest == digest,
                "测试操作 ID 被不同冻结请求复用",
            )?;
            state
        } else {
            let state = Job {
                request_digest: digest.clone(),
                started: timestamp(),
                start_dispatched: false,
            };
            Self::write(&metadata, &state)?;
            state
        };
        require(
            Self::read(&job.join("cancelled.json"))?.is_none(),
            "测试已取消，不能恢复或重跑旧 operation",
        )?;
        if let Some(value) = Self::read(&job.join("result.json"))? {
            return Ok(Some(value));
        }
        if job.join("work").exists() {
            CliGit::new(&self.root)?.export_revision(
                r.code.as_ref().unwrap(),
                &r.candidate,
                &job.join("work"),
            )?;
        }
        let sandbox = DockerSandbox::new(&root)?;
        let mut receipt = sandbox.inspect(&r.operation_id)?;
        if !state.start_dispatched {
            let work = job.join("work");
            CliGit::new(&self.root)?.export_revision(
                r.code.as_ref().unwrap(),
                &r.candidate,
                &work,
            )?;
            let state_dir = job.join("state");
            fs::create_dir_all(&state_dir).map_err(storage)?;
            let reporter = job.join("reporter.mjs");
            reject_symlink(&reporter)?;
            fs::write(
                &reporter,
                include_bytes!("../../../tests/reporters/node-reporter.mjs"),
            )
            .map_err(storage)?;
            let mut command = vec![
                "node".into(),
                "--test".into(),
                "--test-reporter=/run/devdesk/test-reporter.mjs".into(),
            ];
            command.extend(recipe.files.iter().map(|p| format!("/work/{p}")));
            let session = r
                .runtime
                .as_ref()
                .ok_or_else(|| err("blocked", "测试缺少具体执行实例"))?;
            require(
                session.sandbox.origin == Origin::External
                    && session.sandbox.provider == "docker"
                    && session.sandbox.image.as_ref() == Some(&recipe.image),
                "测试环境定义与固定镜像不符",
            )?;
            require(
                session.run_id == r.operation_id && session.id == r.environment,
                "测试执行实例身份不符",
            )?;
            let created = sandbox.create(&ContainerSpec { host_gateway: false,
                permissions: Some(isolated_test_permissions()),
                purpose: ContainerPurpose::Tests,
                project_id: r.code.as_ref().unwrap().project.clone(),
                session_id: r
                    .session_id
                    .clone()
                    .ok_or_else(|| err("blocked", "测试缺少 Session 归属"))?,
                operation_id: r.operation_id.clone(),
                image_id: recipe.image.clone(),
                worktree: work,
                state: state_dir,
                writable: false,
                writable_subdirectories: vec![],
                read_only_subdirectories: vec![],
                network: false,
                command,
                nested_sandbox: false,
                credential_env_file: None,
                profile_file: Some(reporter),
                runner_file: None,
                skill_directory: None,
                sensitive_output: false,
                profile_target: Some("/run/devdesk/test-reporter.mjs".into()),
            })?;
            require(
                created.status == "created",
                "测试容器已运行过但缺少启动意图，需核对",
            )?;
            state.start_dispatched = true;
            Self::write(&metadata, &state)?;
            receipt = Some(sandbox.start(&r.operation_id)?);
        }
        let receipt = receipt.ok_or_else(|| {
            err(
                "needs_reconciliation",
                "测试启动后容器丢失，禁止重跑旧 operation",
            )
        })?;
        require(receipt.image == recipe.image, "测试容器镜像与冻结配方不同")?;
        require(
            receipt.status != "created",
            "测试启动结果未知，禁止重发启动",
        )?;
        let timed_out = timestamp().saturating_sub(state.started) > recipe.timeout_seconds * 1000;
        if receipt.running && !timed_out {
            return Ok(None);
        }
        if receipt.running {
            sandbox.stop(&r.operation_id)?;
        }
        let stopped = sandbox
            .inspect(&r.operation_id)?
            .ok_or_else(|| err("needs_reconciliation", "测试停止回执丢失"))?;
        require(!stopped.running, "测试执行者仍未停止")?;
        let log = sandbox.logs(&r.operation_id)?;
        let result = Self::result(r, &recipe, &stopped, &log, timed_out)?;
        Self::write(&job.join("result.json"), &result)?;
        Ok(Some(result))
    }
    fn result(
        r: &TestRequest,
        recipe: &TestRecipe,
        receipt: &crate::docker::ContainerReceipt,
        log: &str,
        timed_out: bool,
    ) -> Result<Value> {
        let mut assertions = 0;
        let mut environment = false;
        let mut summary = None;
        for line in log.lines() {
            match serde_json::from_str::<Value>(line) {
                Ok(value) if value["devdesk_test_report"] == 1 => match value["event"].as_str() {
                    Some("fail") => match value["classification"].as_str() {
                        Some("assertion") => assertions += 1,
                        Some("aggregate") => {}
                        _ => environment = true,
                    },
                    Some("summary") => summary = Some(value["counts"].clone()),
                    _ => environment = true,
                },
                _ => environment = true,
            }
        }
        let counts = summary.unwrap_or(Value::Null);
        let failed = counts["failed"].as_u64().unwrap_or(0);
        let total = counts["tests"].as_u64().unwrap_or(0);
        environment |= timed_out
            || total == 0
            || counts.is_null()
            || (receipt.exit_code == 0 && failed > 0)
            || (receipt.exit_code != 0 && failed == 0)
            || (failed > 0 && assertions == 0);
        let assertion_failed = !environment && failed > 0;
        let expected_failure = r.phase == "red" && assertion_failed;
        let passed = !environment && (receipt.exit_code == 0 || expected_failure);
        let result = TestResult {
            operation_id: r.operation_id.clone(),
            status: if passed {
                TestOutcome::Succeeded
            } else {
                TestOutcome::Failed
            },
            candidate: r.candidate.clone(),
            test_phase: r.phase.clone(),
            test_definitions: r.test_definitions.clone(),
            environment: r.environment.clone(),
            origin: Origin::External,
            assertion_failed,
            expected_failure,
            failure_type: if environment {
                Some(if timed_out { "timeout" } else { "environment" }.into())
            } else if assertion_failed && !expected_failure {
                Some("assertion".into())
            } else {
                None
            },
        };
        let mut result = json!(result);
        result["exit_code"] = json!(receipt.exit_code);
        result["container_id"] = json!(receipt.id);
        result["image_digest"] = json!(receipt.image);
        result["recipe"] = json!(recipe);
        result["permissions"] = json!(isolated_test_permissions());
        result["counts"] = counts;
        result["report"] = json!(log);
        result["report_sha256"] = json!(format!("{:x}", Sha256::digest(log.as_bytes())));
        result["writer_revoked"] = json!(true);
        Ok(result)
    }
    pub fn stop(&self, r: &TestRequest) -> Result<bool> {
        let _lock = self.writer.lock().map_err(storage)?;
        let (root, job) = self.paths(r)?;
        let state: Job = if let Some(metadata) = Self::read(&job.join("job.json"))? {
            serde_json::from_value(metadata).map_err(storage)?
        } else {
            require(
                DockerSandbox::new(&root)?
                    .inspect(&r.operation_id)?
                    .is_none(),
                "测试容器存在但缺少操作身份，需核对",
            )?;
            let state = Job {
                request_digest: configuration_digest(&json!(r))?,
                started: timestamp(),
                start_dispatched: false,
            };
            Self::write(&job.join("job.json"), &state)?;
            state
        };
        require(
            state.request_digest == configuration_digest(&json!(r))?,
            "测试停止请求身份不符",
        )?;
        DockerSandbox::new(&root)?.stop(&r.operation_id)?;
        Self::write(
            &job.join("cancelled.json"),
            &json!({"operation_id":r.operation_id,"writer_revoked":true}),
        )?;
        Ok(true)
    }
    pub fn cleanup(&self, r: &TestRequest) -> Result<()> {
        let (root, job) = self.paths(r)?;
        require(
            Self::read(&job.join("result.json"))?.is_some()
                || Self::read(&job.join("cancelled.json"))?.is_some(),
            "先导出结果/停止回执才能清理测试容器",
        )?;
        DockerSandbox::new(&root)?.remove(&r.operation_id)
    }
}
impl ports::Tests for CommandTests {
    fn identity(&self) -> String {
        format!(
            "devdesk_adapters::CommandTests/v1+reporter.{:x}",
            Sha256::digest(include_bytes!("../../../tests/reporters/node-reporter.mjs"))
        )
    }

    fn accepts_identity(&self, identity: &str) -> bool {
        let current = self.identity();
        identity == current
            || identity
                .strip_prefix("routing-tests/v1:devdesk_adapters::SimTests:")
                .is_some_and(|native| native == current)
    }
    fn poll_test(&self, r: &TestRequest) -> Result<Option<Value>> {
        self.poll(r)
    }
    fn stop_test(&self, r: &TestRequest) -> Result<bool> {
        self.stop(r)
    }
    fn cleanup_test(&self, r: &TestRequest) -> Result<()> {
        self.cleanup(r)
    }
    fn reconcile(&self, r: &TestRequest) -> Result<Option<Value>> {
        self.poll(r)
    }
}

#[cfg(test)]
mod reporter_tests {
    use super::*;
    #[test]
    fn legacy_native_test_identity_requires_the_same_executor_and_reporter() {
        use devdesk_core::ports::Tests;
        let dir = tempfile::tempdir().unwrap();
        let runner = CommandTests::new(dir.path()).unwrap();
        let current = runner.identity();
        assert!(runner.accepts_identity(&current));
        assert!(runner.accepts_identity(&format!(
            "routing-tests/v1:devdesk_adapters::SimTests:{current}"
        )));
        assert!(
            !runner
                .accepts_identity("routing-tests/v1:devdesk_adapters::SimTests:changed-reporter")
        );
    }
    #[test]
    fn node_reporter_classifies_failures_and_ignores_forged_stdout() {
        let dir = tempfile::tempdir().unwrap();
        let reporter = dir.path().join("reporter.mjs");
        fs::write(
            &reporter,
            include_bytes!("../../../tests/reporters/node-reporter.mjs"),
        )
        .unwrap();
        let test = dir.path().join("contract.test.mjs");
        fs::write(
            &test,
            r#"import test from 'node:test';
import assert from 'node:assert/strict';
test('passing output',()=>console.log('forged-stdout-report'));
test('assertion',()=>assert.equal(1,2));
test('environment',()=>{throw new Error('private-error-message');});
"#,
        )
        .unwrap();
        let output = std::process::Command::new("node")
            .arg("--test")
            .arg(format!("--test-reporter={}", reporter.display()))
            .arg(test)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            !stdout.contains("forged-stdout-report") && !stdout.contains("private-error-message")
        );
        let records: Vec<Value> = stdout
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(records.iter().all(|r| r["devdesk_test_report"] == 1));
        assert!(
            records
                .iter()
                .any(|r| r["event"] == "fail" && r["classification"] == "assertion")
        );
        assert!(
            records
                .iter()
                .any(|r| r["event"] == "fail" && r["classification"] == "environment")
        );
        assert!(records.iter().any(|r| r["event"] == "summary"
            && r["counts"]["failed"] == 2
            && r["counts"]["passed"] == 1));
    }
}
