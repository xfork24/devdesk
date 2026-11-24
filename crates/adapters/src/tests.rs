//! Credential-free, fixed-revision tests in owned Docker containers. Never infer test success from LLM output.
use super::*;
use crate::docker::{ContainerPurpose, ContainerSpec, DockerSandbox};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const NODE_REPORTER_DIGEST: &str =
    "4da4a17d61d5f3aa2baced84a43a7fbcc719babb8fc8e8ee02fd6c22d22fdafb";
const PROJECT_REPORTER_DIGEST: &str =
    "9538ce6eb3cabe73f2238cff30dbf961c2f2a0b4b66b70768d143f5df49cfe84";

#[cfg(any(feature = "engineering-defaults", feature = "test-fixtures", test))]
fn legacy_reporter(runner: &TestRunner) -> Result<&'static [u8]> {
    Ok(if *runner == TestRunner::NodeTest {
        include_bytes!("../../../configurations/software-development/runtime/node-reporter.mjs")
    } else {
        include_bytes!("../../../configurations/software-development/runtime/project-tests.py")
    })
}

#[cfg(not(any(feature = "engineering-defaults", feature = "test-fixtures", test)))]
fn legacy_reporter(_runner: &TestRunner) -> Result<&'static [u8]> {
    Err(err(
        "configuration_required",
        "请选择提供该测试运行器报告器的领域包",
    ))
}
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
    cells: Mutex<std::collections::BTreeMap<String, std::sync::Weak<Mutex<()>>>>,
}
impl CommandTests {
    fn command(recipe: &TestRecipe) -> Result<Vec<String>> {
        if recipe.runner == TestRunner::NodeTest {
            require(
                recipe.preparation.is_empty(),
                "Node 旧合同不支持准备入口；请使用项目 command 配方",
            )?;
            let mut command = vec![
                "node".into(),
                "--test".into(),
                "--test-reporter=/run/devdesk/test-reporter.mjs".into(),
            ];
            command.extend(recipe.files.iter().map(|p| format!("/work/{p}")));
            Ok(command)
        } else {
            Ok(vec![
                "python3".into(),
                "-I".into(),
                "/run/devdesk/test-reporter.mjs".into(),
                serde_json::to_string(recipe).map_err(storage)?,
            ])
        }
    }
    pub fn new(root: &Path) -> Result<Self> {
        Ok(Self {
            root: fs::canonicalize(root).map_err(storage)?,
            cells: Default::default(),
        })
    }
    fn paths(&self, r: &TestRequest) -> Result<(PathBuf, PathBuf)> {
        self.locations(r, true)
    }
    fn locations(&self, r: &TestRequest, create: bool) -> Result<(PathBuf, PathBuf)> {
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
        if create {
            fs::create_dir_all(&parent).map_err(storage)?;
        }
        reject_symlink(&parent)?;
        let job = parent.join(&r.operation_id);
        reject_symlink(&job)?;
        if create {
            fs::create_dir_all(&job).map_err(storage)?;
        }
        Ok((root, job))
    }
    fn cell(&self, operation: &str) -> Result<Arc<Mutex<()>>> {
        let mut cells = self.cells.lock().map_err(storage)?;
        cells.retain(|_, cell| cell.strong_count() > 0);
        if let Some(cell) = cells.get(operation).and_then(std::sync::Weak::upgrade) {
            return Ok(cell);
        }
        let cell = Arc::new(Mutex::new(()));
        cells.insert(operation.into(), Arc::downgrade(&cell));
        Ok(cell)
    }
    fn recipe(&self, r: &TestRequest) -> Result<TestRecipe> {
        let recipe = r
            .recipe
            .as_ref()
            .ok_or_else(|| err("configuration_required", "测试工具需要明确执行配方"))?;
        recipe.validate()?;
        Ok(recipe.clone())
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
        let cell = self.cell(&r.operation_id)?;
        let _lock = cell.lock().map_err(storage)?;
        self.poll_locked(r, true)
    }
    fn poll_locked(&self, r: &TestRequest, allow_start: bool) -> Result<Option<Value>> {
        self.poll_observed_locked(r, allow_start, &mut None)
    }
    fn poll_observed_locked(
        &self,
        r: &TestRequest,
        allow_start: bool,
        observation: &mut Option<ports::TestExecutionObservation>,
    ) -> Result<Option<Value>> {
        let recipe = self.recipe(r)?;
        require(
            !r.expect_assertion_failure || recipe.runner != TestRunner::Command,
            "通用命令退出码不能证明有效业务 Red；请选择结构化独立测试执行器",
        )?;
        let (root, job) = self.locations(r, allow_start)?;
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
            if !allow_start {
                *observation = Some(ports::TestExecutionObservation::NotStarted);
                return Ok(None);
            }
            let state = Job {
                request_digest: digest.clone(),
                started: timestamp(),
                start_dispatched: false,
            };
            Self::write(&metadata, &state)?;
            state
        };
        if let Some(value) = Self::read(&job.join("result.json"))? {
            return Ok(Some(value));
        }
        if allow_start {
            require(
                Self::read(&job.join("cancelled.json"))?.is_none(),
                "测试已取消，不能恢复或重跑旧 operation",
            )?;
        }
        if allow_start && job.join("work").exists() {
            CliGit::new(&self.root)?.export_revision(
                r.code.as_ref().unwrap(),
                &r.candidate,
                &job.join("work"),
            )?;
        }
        let sandbox = DockerSandbox::for_dataset(&root, &self.root)?;
        let mut receipt = sandbox.inspect(&r.operation_id)?;
        if !state.start_dispatched {
            if !allow_start {
                *observation = Some(ports::TestExecutionObservation::NotStarted);
                return Ok(None);
            }
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
            let content = if let Some(source) = &r.reporter {
                require(
                    source.content.len() <= 64 * 1024
                        && format!("{:x}", Sha256::digest(source.content.as_bytes()))
                            == source.digest,
                    "测试报告器与冻结摘要不符",
                )?;
                source.content.as_bytes()
            } else {
                legacy_reporter(&recipe.runner)?
            };
            fs::write(&reporter, content).map_err(storage)?;
            let command = Self::command(&recipe)?;
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
                session.operation_id == r.operation_id && session.id == r.environment,
                "测试执行实例身份不符",
            )?;
            let created = sandbox.create(&ContainerSpec {
                host_gateway: false,
                permissions: Some(isolated_test_permissions()),
                purpose: ContainerPurpose::Tests,
                project_id: r.code.as_ref().unwrap().project.clone(),
                session_id: r
                    .session_id
                    .clone()
                    .unwrap_or_else(|| r.operation_id.clone()),
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
                command_output_directory: None,
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
        if !allow_start && receipt.is_none() && Self::read(&job.join("cancelled.json"))?.is_some() {
            *observation = Some(ports::TestExecutionObservation::Stopped);
            return Ok(None);
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
        // A delayed Host poll is not test execution time. Even an already exited
        // container must still satisfy the budget using its own Docker clocks.
        let timed_out = receipt.runtime_millis(timestamp())? > recipe.timeout_seconds * 1000;
        if receipt.running && !timed_out {
            *observation = Some(ports::TestExecutionObservation::Running { timed_out });
            return Ok(None);
        }
        let stopped = if receipt.running {
            if !allow_start {
                *observation = Some(ports::TestExecutionObservation::Running { timed_out });
                return Ok(None);
            }
            sandbox.stop(&r.operation_id)?;
            sandbox
                .inspect(&r.operation_id)?
                .ok_or_else(|| err("needs_reconciliation", "测试停止回执丢失"))?
        } else {
            // This poll already obtained the stopped identity and Docker timestamps.
            // Reuse that proof through log export rather than doing two more CLI inspects.
            receipt
        };
        require(!stopped.running, "测试执行者仍未停止")?;
        let (log, stderr) = sandbox.logs_for_stopped(&r.operation_id, &stopped)?;
        let mut result = Self::result(r, &recipe, &stopped, &log, timed_out)?;
        let work = job.join("work").canonicalize().map_err(storage)?;
        let mut files = Vec::new();
        let mut gaps = Vec::new();
        for path in &recipe.files {
            let file = work.join(path);
            if !file.exists() {
                gaps.push(json!({"path":path,"reason":"missing in frozen revision"}));
                continue;
            }
            reject_symlink(&file)?;
            require(
                file.canonicalize().map_err(storage)?.starts_with(&work),
                "测试证据文件越界",
            )?;
            let bytes = fs::read(&file).map_err(storage)?;
            let Ok(content) = String::from_utf8(bytes.clone()) else {
                gaps.push(json!({"path":path,"reason":"non-UTF-8 source","sha256":format!("{:x}",Sha256::digest(&bytes))}));
                continue;
            };
            files.push(json!({"path":path,"sha256":format!("{:x}",Sha256::digest(content.as_bytes())),"content":content}));
        }
        let command = Self::command(&recipe)?;
        result["execution"] = json!({"command":serde_json::to_string(&command).map_err(storage)?,"cwd":"/work","started_at":format!("unix_ms:{}",stopped.started_at.ok_or_else(||err("needs_reconciliation","测试缺少实际开始时间"))?),"finished_at":format!("unix_ms:{}",stopped.finished_at.ok_or_else(||err("needs_reconciliation","测试缺少实际结束时间"))?),"exit_code":stopped.exit_code,"stdout":log,"stderr":stderr,"files":files});
        result["execution"]["recipe_digest"] = json!(configuration_digest(&json!(recipe))?);
        result["execution"]["termination"] =
            json!({"timed_out":timed_out,"cancelled":false,"writer_revoked":true});
        if !gaps.is_empty() {
            result["execution_file_gaps"] = json!(gaps);
        }
        if allow_start {
            Self::write(&job.join("result.json"), &result)?;
        }
        Ok(Some(result))
    }
    fn inspect(&self, r: &TestRequest) -> Result<ports::TestExecutionObservation> {
        let cell = self.cell(&r.operation_id)?;
        let _lock = cell.lock().map_err(storage)?;
        let mut observation = None;
        if let Some(result) = self.poll_observed_locked(r, false, &mut observation)? {
            return Ok(ports::TestExecutionObservation::Completed { result });
        }
        // Preserve the single physical observation. A second Docker inspect
        // could see running -> exited and misclassify that valid transition as
        // Unknown; the next read will export the original stopped result.
        observation.ok_or_else(|| err("adapter_contract", "测试观察缺少明确状态"))
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
                    Some("diagnostic")
                        if value["source"].as_str().is_some_and(|source| {
                            ["test:stdout", "test:stderr", "test:diagnostic"].contains(&source)
                        }) => {}
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
        let expected_failure = r.expect_assertion_failure && assertion_failed;
        let passed = !environment && (receipt.exit_code == 0 || expected_failure);
        let result = TestResult {
            operation_id: r.operation_id.clone(),
            status: if passed {
                TestOutcome::Succeeded
            } else {
                TestOutcome::Failed
            },
            candidate: r.candidate.clone(),
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
        let cell = self.cell(&r.operation_id)?;
        let _lock = cell.lock().map_err(storage)?;
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
        // Persist the physical no-start fence before asking Docker to stop.
        Self::write(
            &job.join("cancelled.json"),
            &json!({"operation_id":r.operation_id,"phase":"requested"}),
        )?;
        std::fs::File::open(&job)
            .and_then(|f| f.sync_all())
            .map_err(storage)?;
        let sandbox = DockerSandbox::new(&root)?;
        sandbox.stop(&r.operation_id)?;
        require(
            sandbox.inspect(&r.operation_id)?.is_none_or(|r| !r.running),
            "原测试 worker 尚未停止",
        )?;
        Self::write(
            &job.join("cancelled.json"),
            &json!({"operation_id":r.operation_id,"phase":"proved","writer_revoked":true}),
        )?;
        Ok(true)
    }
    pub fn cleanup(&self, r: &TestRequest) -> Result<()> {
        let cell = self.cell(&r.operation_id)?;
        let _lock = cell.lock().map_err(storage)?;
        let (root, job) = self.paths(r)?;
        let state: Job = serde_json::from_value(
            Self::read(&job.join("job.json"))?
                .ok_or_else(|| err("needs_reconciliation", "清理缺少原测试身份"))?,
        )
        .map_err(storage)?;
        require(
            state.request_digest == configuration_digest(&json!(r))?,
            "测试清理不能替换原请求",
        )?;
        require(
            Self::read(&job.join("result.json"))?.is_some()
                || Self::read(&job.join("cancelled.json"))?.is_some()
                || self.poll_locked(r, false)?.is_some(),
            "先导出结果/停止回执才能清理测试容器",
        )?;
        let sandbox = DockerSandbox::new(&root)?;
        require(
            sandbox
                .inspect(&r.operation_id)?
                .is_none_or(|receipt| !receipt.running),
            "测试清理前必须取得停止证明",
        )?;
        sandbox.remove(&r.operation_id)?;
        // The platform grants disposal after retaining the original result or
        // stop proof. Keep job identity and any local result for replay.
        if let Err(error) = crate::storage_maintenance::retire_test(&job) {
            eprintln!("DevDesk test scratch cleanup: {}", error.message);
        }
        Ok(())
    }
}
impl ports::Tests for CommandTests {
    fn identity(&self) -> String {
        format!(
            "devdesk_adapters::CommandTests/v3+node.{NODE_REPORTER_DIGEST}+project.{PROJECT_REPORTER_DIGEST}",
        )
    }

    fn accepts_identity(&self, identity: &str) -> bool {
        let current = self.identity();
        identity == current
            || identity
                == format!("devdesk_adapters::CommandTests/v2+reporter.{NODE_REPORTER_DIGEST}")
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
        self.inspect(r).map(|observation| match observation {
            ports::TestExecutionObservation::Completed { result } => Some(result),
            _ => None,
        })
    }
    fn inspect_test(&self, r: &TestRequest) -> Result<ports::TestExecutionObservation> {
        self.inspect(r)
    }
    fn inspect_test_cleanup(&self, r: &TestRequest) -> Result<bool> {
        let cell = self.cell(&r.operation_id)?;
        let _lock = cell.lock().map_err(storage)?;
        let (root, job) = self.locations(r, false)?;
        let state: Job = serde_json::from_value(
            Self::read(&job.join("job.json"))?
                .ok_or_else(|| err("needs_reconciliation", "测试清理核对缺少原身份"))?,
        )
        .map_err(storage)?;
        require(
            state.request_digest == configuration_digest(&json!(r))?,
            "测试清理核对改变了原请求",
        )?;
        Ok(DockerSandbox::new(&root)?
            .inspect(&r.operation_id)?
            .is_none())
    }
}

#[cfg(test)]
mod reporter_tests {
    use super::*;
    #[test]
    fn test_identity_requires_the_current_executor_and_reporter() {
        use devdesk_core::ports::Tests;
        let dir = tempfile::tempdir().unwrap();
        let runner = CommandTests::new(dir.path()).unwrap();
        let current = runner.identity();
        assert!(runner.accepts_identity(&current));
        assert!(!runner.accepts_identity(&format!(
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
        fs::write(&reporter, legacy_reporter(&TestRunner::NodeTest).unwrap()).unwrap();
        let test = dir.path().join("contract.test.mjs");
        fs::write(
            &test,
            r#"import test from 'node:test';
import assert from 'node:assert/strict';
test('passing output',()=>console.log('forged-stdout-report',JSON.stringify({devdesk_test_report:1,event:'fail',classification:'assertion'})));
test('assertion',()=>assert.equal(1,2));
test('environment',()=>{throw new Error('fixture failure detail');});
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
        assert!(stdout.contains("fixture failure detail"));
        let records: Vec<Value> = stdout
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(records.iter().all(|r| r["devdesk_test_report"] == 1));
        assert!(records.iter().any(|r| {
            r["event"] == "diagnostic"
                && r["source"] == "test:stdout"
                && r["diagnostic"]["message"]
                    .as_str()
                    .is_some_and(|s| s.contains("forged-stdout-report"))
        }));
        assert_eq!(records.iter().filter(|r| r["event"] == "fail").count(), 2);
        assert!(records.iter().any(|r| {
            r["event"] == "fail"
                && r["classification"] == "assertion"
                && r["diagnostic"]["details"]["error"]["cause"]["code"] == "ERR_ASSERTION"
                && r["diagnostic"]["details"]["error"]["cause"]["message"]
                    .as_str()
                    .is_some_and(|s| s.contains("1 !== 2"))
                && r["diagnostic"]["details"]["error"]["cause"]["stack"]
                    .as_str()
                    .is_some_and(|s| s.contains("contract.test.mjs"))
        }));
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
