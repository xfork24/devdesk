//! Docker resource service. Agents receive an isolated work directory, never the daemon socket.
use super::*;
use crate::process::{Output, capture};
use std::time::Duration;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerPurpose {
    Harness,
    #[default]
    Tests,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct ContainerSpec {
    #[serde(default)]
    pub purpose: ContainerPurpose,
    pub project_id: Id,
    pub session_id: Id,
    /// Stable operation identity is also the container idempotency key.
    pub operation_id: Id,
    pub image_id: String,
    pub worktree: PathBuf,
    pub state: PathBuf,
    pub writable: bool,
    #[serde(default)]
    pub writable_subdirectories: Vec<String>,
    #[serde(default)]
    pub read_only_subdirectories: Vec<String>,
    pub network: bool,
    pub command: Vec<String>,
    #[serde(default)]
    pub nested_sandbox: bool,
    #[serde(default)]
    pub credential_env_file: Option<PathBuf>,
    #[serde(default)]
    pub profile_file: Option<PathBuf>,
    #[serde(default)]
    pub runner_file: Option<PathBuf>,
    #[serde(default)]
    pub profile_target: Option<String>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct ContainerReceipt {
    pub name: String,
    pub id: String,
    pub image: String,
    pub running: bool,
    pub status: String,
    pub exit_code: i64,
    pub specification_digest: String,
}

pub struct DockerSandbox {
    root: PathBuf,
    user: String,
}

impl DockerSandbox {
    fn checked_file(
        &self,
        path: &Path,
        work: &Path,
        state: &Path,
        secret: bool,
    ) -> Result<PathBuf> {
        reject_symlink(path)?;
        let path = fs::canonicalize(path).map_err(storage)?;
        require(
            path.starts_with(&self.root)
                && path.is_file()
                && !path.starts_with(work)
                && !path.starts_with(state),
            "配置与凭据文件必须位于项目协调器目录内，且不能位于 Agent 可写目录",
        )?;
        require(
            !path.to_string_lossy().contains([',', '\n', '\r']),
            "配置文件路径无效",
        )?;
        #[cfg(unix)]
        if secret {
            use std::os::unix::fs::PermissionsExt;
            require(
                fs::metadata(&path).map_err(storage)?.permissions().mode() & 0o077 == 0,
                "凭据文件必须为私有权限",
            )?;
        }
        Ok(path)
    }
    fn nested_policy(&self) -> Result<PathBuf> {
        let policy = include_bytes!("../../../containers/security/nested-sandbox.json");
        let digest = format!("{:x}", Sha256::digest(policy));
        let path = self.root.join(format!(".seccomp-{digest}.json"));
        reject_symlink(&path)?;
        if !path.exists() {
            let mut options = OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(mut file) => {
                    use std::io::Write;
                    file.write_all(policy).map_err(storage)?;
                    file.sync_all().map_err(storage)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(storage(error)),
            }
        }
        require(
            fs::read(&path).map_err(storage)? == policy,
            "沙箱 syscall 配置发生漂移",
        )?;
        Ok(path)
    }
    /// Root is the coordinator-owned project execution directory, not a user-selected arbitrary mount.
    pub fn new(root: &Path) -> Result<Self> {
        #[cfg(unix)]
        let user = {
            use std::os::unix::fs::MetadataExt;
            let meta = fs::metadata(root).map_err(storage)?;
            if meta.uid() == 0 {
                "1000:1000".to_string()
            } else {
                format!("{}:{}", meta.uid(), meta.gid())
            }
        };
        #[cfg(not(unix))]
        let user = "1000:1000".to_string();
        Ok(Self {
            root: fs::canonicalize(root).map_err(storage)?,
            user,
        })
    }
    fn call(&self, args: Vec<String>) -> Result<Output> {
        capture("docker", &args, Duration::from_secs(30))
    }
    fn name(operation: &str) -> Result<String> {
        let id = uuid::Uuid::parse_str(operation)
            .map_err(|_| err("invalid_params", "容器操作 ID 必须是 UUID"))?;
        Ok(format!("devdesk-{}", id.simple()))
    }
    fn checked_path(&self, path: &Path) -> Result<PathBuf> {
        let path = fs::canonicalize(path).map_err(storage)?;
        require(
            path.starts_with(&self.root) && path != self.root && path.is_dir(),
            "容器目录必须位于项目执行根目录内",
        )?;
        require(
            !path.to_string_lossy().contains([',', '\n', '\r']),
            "容器挂载路径包含不支持的字符",
        )?;
        Ok(path)
    }
    fn inspect_owned(&self, name: &str) -> Result<Option<(ContainerReceipt, Value)>> {
        let output = self.call(vec!["container".into(), "inspect".into(), name.into()])?;
        if output.code != 0 {
            // Differentiate an absent object from an unreachable daemon. Never infer deletion from a transport failure.
            let info = self.call(vec![
                "info".into(),
                "--format".into(),
                "{{.ServerVersion}}".into(),
            ])?;
            require(info.code == 0, "Docker daemon 不可用；不能确定容器状态")?;
            let ls = self.call(vec![
                "container".into(),
                "ls".into(),
                "--all".into(),
                "--filter".into(),
                format!("name=^/{name}$"),
                "--format".into(),
                "{{.ID}}".into(),
            ])?;
            require(ls.code == 0 && ls.stdout.is_empty(), "容器存在但无法核对")?;
            return Ok(None);
        }
        require(!output.truncated, "容器元数据超出限制")?;
        let value: Value = serde_json::from_slice(&output.stdout).map_err(storage)?;
        let row = &value[0];
        require(
            row["Config"]["Labels"]["devdesk.root"] == self.root.to_string_lossy().as_ref(),
            "容器不属于当前项目执行根目录",
        )?;
        let digest = row["Config"]["Labels"]["devdesk.spec"]
            .as_str()
            .ok_or_else(|| err("blocked", "容器缺少规格摘要"))?;
        Ok(Some((
            ContainerReceipt {
                name: name.into(),
                id: row["Id"].as_str().unwrap_or_default().into(),
                image: row["Image"].as_str().unwrap_or_default().into(),
                running: row["State"]["Running"].as_bool().unwrap_or(false),
                status: row["State"]["Status"].as_str().unwrap_or("unknown").into(),
                exit_code: row["State"]["ExitCode"].as_i64().unwrap_or(-1),
                specification_digest: digest.into(),
            },
            row.clone(),
        )))
    }
    pub fn inspect(&self, operation_id: &str) -> Result<Option<ContainerReceipt>> {
        Ok(self
            .inspect_owned(&Self::name(operation_id)?)?
            .map(|(r, _)| r))
    }
    /// Deterministic names allow reconciliation if creation succeeded before its receipt was persisted.
    pub fn create(&self, spec: &ContainerSpec) -> Result<ContainerReceipt> {
        let name = Self::name(&spec.operation_id)?;
        for id in [&spec.project_id, &spec.session_id] {
            require(uuid::Uuid::parse_str(id).is_ok(), "无效容器归属 ID")?;
        }
        let work = self.checked_path(&spec.worktree)?;
        let state = self.checked_path(&spec.state)?;
        if spec.purpose == ContainerPurpose::Tests {
            require(
                !spec.network
                    && spec.credential_env_file.is_none()
                    && spec.runner_file.is_none()
                    && !spec.nested_sandbox,
                "测试容器不得带模型凭据、网络或 Harness namespace 策略",
            )?;
        }
        let credential = spec
            .credential_env_file
            .as_ref()
            .map(|p| self.checked_file(p, &work, &state, true))
            .transpose()?;
        let runner = spec
            .runner_file
            .as_ref()
            .map(|p| self.checked_file(p, &work, &state, false))
            .transpose()?;
        if let Some(path) = &runner {
            require(
                spec.purpose == ContainerPurpose::Harness
                    && spec.nested_sandbox
                    && fs::read(path).map_err(storage)?
                        == include_bytes!("../../../containers/dsh/devdesk-policy/bwrap.sh"),
                "执行包装器不符合固定沙箱策略",
            )?;
        }
        let profile = spec
            .profile_file
            .as_ref()
            .map(|p| self.checked_file(p, &work, &state, false))
            .transpose()?;
        require(
            !work.starts_with(&state) && !state.starts_with(&work),
            "工作目录与会话状态目录必须隔离",
        )?;
        require(
            spec.image_id.starts_with("sha256:")
                && spec.image_id.len() == 71
                && spec.image_id[7..]
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "必须使用固定 Docker image ID",
        )?;
        require(
            !spec.command.is_empty() && spec.command.len() <= 128,
            "容器需要明确命令",
        )?;
        let mut canonical = spec.clone();
        canonical.worktree = work.clone();
        canonical.state = state.clone();
        canonical.credential_env_file = credential.clone();
        canonical.profile_file = profile.clone();
        canonical.runner_file = runner.clone();
        let digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&json!({"spec":canonical,"profile_digest":profile.as_ref().map(|p|fs::read(p).map(|data|format!("{:x}",Sha256::digest(data)))).transpose().map_err(storage)?,"nested_policy":if spec.nested_sandbox {Some(format!("{:x}",Sha256::digest(include_bytes!("../../../containers/security/nested-sandbox.json"))))} else {None}})).map_err(storage)?)
        );
        if let Some((receipt, _)) = self.inspect_owned(&name)? {
            require(
                receipt.specification_digest == digest,
                "操作 ID 被不同容器规格复用",
            )?;
            return Ok(receipt);
        }
        let mut args: Vec<String> = vec![
            "create",
            "--interactive",
            "--name",
            &name,
            "--read-only",
            "--user",
            &self.user,
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--pids-limit=256",
            "--memory=1g",
            "--cpus=2",
            "--tmpfs",
            "/tmp:rw,noexec,nosuid,size=128m",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        if spec.nested_sandbox {
            args.extend([
                "--security-opt".into(),
                format!("seccomp={}", self.nested_policy()?.display()),
            ]);
        }
        if let Some(file) = credential {
            args.extend(["--env-file".into(), file.to_string_lossy().into_owned()]);
        }
        let profile_target = spec
            .profile_target
            .as_deref()
            .unwrap_or("/run/devdesk/profile");
        require(
            ["/run/devdesk/profile", "/run/devdesk/test-reporter.mjs"].contains(&profile_target),
            "配置挂载目标无效",
        )?;
        if let Some(file) = runner {
            args.extend([
                "--mount".into(),
                format!(
                    "type=bind,source={},target=/run/devdesk/runner,readonly",
                    file.display()
                ),
            ]);
        }
        if let Some(file) = profile {
            args.extend([
                "--mount".into(),
                format!(
                    "type=bind,source={},target={profile_target},readonly",
                    file.display()
                ),
            ]);
        }
        if spec.purpose == ContainerPurpose::Harness {
            let home = state.join("home");
            reject_symlink(&home)?;
            fs::create_dir_all(home).map_err(storage)?;
            args.extend(["--env".into(), "HOME=/state/home".into()]);
        } else {
            args.extend(["--env".into(), "HOME=/tmp".into()]);
        }
        if spec.purpose == ContainerPurpose::Tests {
            for name in [
                "OPENAI_API_KEY",
                "DEEPSEEK_API_KEY",
                "GH_TOKEN",
                "GITHUB_TOKEN",
            ] {
                args.extend(["--env".into(), format!("{name}=")]);
            }
        }
        args.extend([
            "--label".into(),
            format!("devdesk.root={}", self.root.display()),
            "--label".into(),
            format!("devdesk.spec={digest}"),
            "--label".into(),
            format!("devdesk.project={}", spec.project_id),
            "--label".into(),
            format!("devdesk.session={}", spec.session_id),
            "--network".into(),
            if spec.network { "bridge" } else { "none" }.into(),
            "--mount".into(),
            format!(
                "type=bind,source={},target=/work{}",
                work.display(),
                if spec.writable { "" } else { ",readonly" }
            ),
            "--mount".into(),
            format!("type=bind,source={},target=/state", state.display()),
            "--workdir=/work".into(),
        ]);
        require(
            !spec.writable || spec.writable_subdirectories.is_empty(),
            "完整写权限不应叠加子目录授权",
        )?;
        for relative in &spec.writable_subdirectories {
            require(
                spec.purpose == ContainerPurpose::Harness
                    && relative.starts_with("tests/")
                    && !relative.contains(['\\', '\n', '\r', ','])
                    && relative
                        .split('/')
                        .all(|v| !v.is_empty() && v != "." && v != ".."),
                "测试写路径不合法",
            )?;
            let source = work.join(relative);
            reject_symlink(&source)?;
            let source = fs::canonicalize(&source).map_err(storage)?;
            require(
                source.starts_with(&work) && source.is_dir(),
                "测试写路径不在所属 worktree 内",
            )?;
            args.extend([
                "--mount".into(),
                format!(
                    "type=bind,source={},target=/work/{relative}",
                    source.display()
                ),
            ]);
        }
        for relative in &spec.read_only_subdirectories {
            require(
                spec.purpose == ContainerPurpose::Harness && relative == "tests",
                "只读覆盖仅允许冻结测试目录",
            )?;
            let path = work.join(relative);
            reject_symlink(&path)?;
            let path = fs::canonicalize(&path).map_err(storage)?;
            require(
                path.starts_with(&work) && path.is_dir(),
                "冻结测试目录不在所属 worktree",
            )?;
            args.extend([
                "--mount".into(),
                format!(
                    "type=bind,source={},target=/work/{relative},readonly",
                    path.display()
                ),
            ]);
        }
        // Git metadata and repository-local runtime config never enter the managed agent view.
        // Existing targets also let the inner sandbox mount its protected paths under a read-only work root.
        for leaf in [".git", ".codex", ".agents"] {
            let target = work.join(leaf);
            reject_symlink(&target)?;
            if !target.exists() && spec.purpose == ContainerPurpose::Harness {
                fs::create_dir(&target).map_err(storage)?;
            }
            if target.exists() {
                let hidden = state.join(format!("hidden-{}", leaf.trim_start_matches('.')));
                reject_symlink(&hidden)?;
                if target.is_dir() {
                    fs::create_dir_all(&hidden).map_err(storage)?;
                } else {
                    fs::write(&hidden, "").map_err(storage)?;
                }
                args.extend([
                    "--mount".into(),
                    format!(
                        "type=bind,source={},target=/work/{leaf},readonly",
                        hidden.display()
                    ),
                ]);
            }
        }
        args.extend([
            "--entrypoint".into(),
            spec.command[0].clone(),
            spec.image_id.clone(),
        ]);
        args.extend(spec.command[1..].iter().cloned());
        let output = self.call(args)?;
        // Even a create error may be an ambiguous receipt; inspect the deterministic identity.
        let receipt = self.inspect(&spec.operation_id)?.ok_or_else(|| {
            err(
                "provider_failure",
                format!("Docker create 未产生可核对容器 (exit {})", output.code),
            )
        })?;
        require(receipt.specification_digest == digest, "容器规格核对失败")?;
        Ok(receipt)
    }
    pub(crate) fn attach_command(&self, operation: &str) -> Result<std::process::Command> {
        let receipt = self
            .inspect(operation)?
            .ok_or_else(|| err("not_found", "容器不存在"))?;
        require(!receipt.running, "不能对运行中的 worker 创建第二个协议连接")?;
        let mut command = std::process::Command::new("docker");
        command.args(["start", "--attach", "--interactive", &receipt.name]);
        Ok(command)
    }
    pub fn logs(&self, operation: &str) -> Result<String> {
        let receipt = self
            .inspect(operation)?
            .ok_or_else(|| err("needs_reconciliation", "测试容器已丢失"))?;
        require(!receipt.running, "日志仅在执行静止后导出")?;
        let output = self.call(vec!["logs".into(), receipt.name])?;
        require(
            output.code == 0 && !output.truncated,
            "容器日志不可完整导出",
        )?;
        String::from_utf8(output.stdout).map_err(storage)
    }
    pub fn start(&self, operation_id: &str) -> Result<ContainerReceipt> {
        let name = Self::name(operation_id)?;
        let receipt = self
            .inspect(operation_id)?
            .ok_or_else(|| err("not_found", "容器不存在"))?;
        if !receipt.running {
            let result = self.call(vec!["start".into(), name])?;
            require(result.code == 0, "容器启动失败，需核对原资源")?;
        }
        self.inspect(operation_id)?
            .ok_or_else(|| err("needs_reconciliation", "容器启动回执缺失"))
    }
    pub fn stop(&self, operation_id: &str) -> Result<()> {
        let Some(receipt) = self.inspect(operation_id)? else {
            return Ok(());
        };
        if receipt.running {
            let result = self.call(vec![
                "stop".into(),
                "--time".into(),
                "5".into(),
                receipt.name,
            ])?;
            require(result.code == 0, "容器停止失败")?;
        }
        require(
            self.inspect(operation_id)?.is_none_or(|r| !r.running),
            "容器仍可能写入，不能释放写许可",
        )
    }
    /// The caller must commit/export artifacts before requesting removal. Shared images are never removed.
    pub fn remove(&self, operation_id: &str) -> Result<()> {
        let Some(receipt) = self.inspect(operation_id)? else {
            return Ok(());
        };
        require(!receipt.running, "必须先证明容器停止，再清理资源")?;
        let result = self.call(vec!["rm".into(), receipt.name])?;
        require(
            result.code == 0 && self.inspect(operation_id)?.is_none(),
            "容器清理尚未确认",
        )
    }
}

impl ports::Sandbox for DockerSandbox {
    fn identity(&self) -> String {
        "devdesk_adapters::DockerSandbox/v1".into()
    }
    fn resource(&self, run_id: &str) -> Resource {
        Resource {
            id: run_id.into(),
            run_id: run_id.into(),
            kind: "docker_container".into(),
            status: Status::Prepared,
            origin: Origin::External,
        }
    }
    fn cleanup(&self, resource: &mut Resource, fail: bool) {
        resource.status = if !fail
            && self
                .stop(&resource.id)
                .and_then(|_| self.remove(&resource.id))
                .is_ok()
        {
            Status::Released
        } else {
            Status::CleanupFailed
        };
    }
}
