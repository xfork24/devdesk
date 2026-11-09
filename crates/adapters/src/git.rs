//! Coordinator-only Git operations. Candidate and advance identities survive lost DB receipts.
use super::*;
use crate::process::{Output, capture_command};
use std::{process::Command, time::Duration};

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct RepositoryFacts {
    pub path: String,
    pub repository: String,
    pub mainline: String,
    pub head: String,
    pub object_format: String,
}
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct WorktreeReceipt {
    pub project: Id,
    pub owner: Id,
    pub branch: String,
    pub path: String,
    pub base: String,
    pub operation: Id,
}
pub struct CliGit {
    root: PathBuf,
    writer: Mutex<()>,
}
fn valid_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn valid_repository(name: &str) -> bool {
    let mut parts = name.split('/');
    matches!((parts.next(),parts.next(),parts.next()),(Some(a),Some(b),None) if [a,b].iter().all(|s|!s.is_empty()&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b"-_.".contains(&b))))
}
fn checked_uuid(value: &str) -> Result<()> {
    require(
        uuid::Uuid::parse_str(value).is_ok(),
        "Git 资源标识必须是 UUID",
    )
}
impl CliGit {
    pub fn new(root: &Path) -> Result<Self> {
        Ok(Self {
            root: fs::canonicalize(root).map_err(storage)?,
            writer: Mutex::new(()),
        })
    }
    fn project(&self, project: &str) -> Result<PathBuf> {
        SqliteStore::project_path(&self.root, project)
    }
    fn repository(&self, project: &str) -> Result<PathBuf> {
        let path = self.project(project)?.join("repository");
        reject_symlink(&path)?;
        let canonical = fs::canonicalize(&path).map_err(storage)?;
        require(canonical == path, "仓库路径重定向")?;
        Ok(path)
    }
    fn git(&self, path: &Path, args: &[&str], input: Option<Vec<u8>>) -> Result<Output> {
        self.git_with_timeout(path, args, input, Duration::from_secs(30))
    }
    fn git_with_timeout(
        &self,
        path: &Path,
        args: &[&str],
        input: Option<Vec<u8>>,
        timeout: Duration,
    ) -> Result<Output> {
        let mut command = Command::new("git");
        // Do not inherit user-configured hooks, filters, aliases, fsmonitor, shell transports, or ambient Git routing.
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        command
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0");
        command
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "commit.gpgSign=false",
                // GitHub smart HTTP transfers can stall on HTTP/2 in some host networks.
                "-c",
                "http.version=HTTP/1.1",
                "-c",
                "user.name=DevDesk",
                "-c",
                "user.email=devdesk@localhost",
                "-c",
                "protocol.file.allow=never",
                "-c",
                "protocol.ext.allow=never",
                "-c",
                "credential.helper=",
                "-c",
                "credential.helper=!gh auth git-credential",
                "-C",
            ])
            .arg(path)
            .args(args);
        capture_command(command, timeout, input)
    }
    fn output(&self, path: &Path, args: &[&str]) -> Result<String> {
        let result = self.git(path, args, None)?;
        require(
            result.code == 0 && !result.truncated,
            "Git 操作失败、冲突或输出过大；保留原资源供核对",
        )?;
        String::from_utf8(result.stdout)
            .map(|s| s.trim_end().into())
            .map_err(storage)
    }
    fn oid(&self, path: &Path, reference: &str) -> Result<String> {
        let rev = self.output(
            path,
            &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
        )?;
        require(valid_oid(&rev), "Git 提交身份无效")?;
        Ok(rev)
    }
    fn existing_ref(&self, path: &Path, reference: &str) -> Result<Option<String>> {
        let result = self.git(path, &["show-ref", "--verify", "--quiet", reference], None)?;
        if result.code == 1 {
            return Ok(None);
        }
        require(result.code == 0 && !result.truncated, "Git ref 核对失败")?;
        let value = self.oid(path, reference)?;
        Ok(Some(value))
    }
    fn operation(&self, project: &str, operation: &str, request: &Value) -> Result<PathBuf> {
        checked_uuid(operation)?;
        let path = self.project(project)?.join("execution/git-operations");
        fs::create_dir_all(&path).map_err(storage)?;
        reject_symlink(&path)?;
        let kind = request["kind"]
            .as_str()
            .ok_or_else(|| err("blocked", "Git 操作缺少用途"))?;
        require(
            !kind.is_empty()
                && kind.len() < 40
                && kind.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
            "Git 操作用途无效",
        )?;
        let legacy = path.join(format!("{operation}.json"));
        reject_symlink(&legacy)?;
        let data = serde_json::to_vec(request).map_err(storage)?;
        if legacy.exists() {
            let old: Value =
                serde_json::from_slice(&fs::read(&legacy).map_err(storage)?).map_err(storage)?;
            if old["kind"] == kind {
                require(old == *request, "Git 操作 ID 被不同请求复用")?;
                return Ok(legacy);
            }
        }
        let path = path.join(format!("{kind}-{operation}.json"));
        reject_symlink(&path)?;
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
                file.write_all(&data).map_err(storage)?;
                file.sync_all().map_err(storage)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                require(
                    fs::read(&path).map_err(storage)? == data,
                    "Git 操作 ID 被不同请求复用",
                )?;
            }
            Err(e) => return Err(storage(e)),
        }
        Ok(path)
    }
    pub fn clone_repository(
        &self,
        project: &str,
        repository: &str,
        mainline: &str,
        operation: &str,
    ) -> Result<RepositoryFacts> {
        let _lock = self.writer.lock().map_err(storage)?;
        require(
            valid_repository(repository),
            "仓库必须是明确的 GitHub owner/repo",
        )?;
        let parent = self.project(project)?;
        require(parent.is_dir(), "Project 数据目录不存在")?;
        self.output(&parent, &["check-ref-format", "--branch", mainline])?;
        self.operation(
            project,
            operation,
            &json!({"kind":"clone","repository":repository,"mainline":mainline}),
        )?;
        let path = parent.join("repository");
        reject_symlink(&path)?;
        if !path.exists() {
            let url = format!("https://github.com/{repository}.git");
            let result = self.git_with_timeout(
                &parent,
                &[
                    "clone",
                    "--bare",
                    "--no-recurse-submodules",
                    "--",
                    &url,
                    "repository",
                ],
                None,
                Duration::from_secs(180),
            )?;
            require(result.code == 0, "仓库 clone 结果需核对，不能更换身份重试")?;
        } else {
            // A killed clone can leave a valid bare repository with no requested branch.
            // Resume into the same owned repository after checking identity; never delete it.
            let path = self.repository(project)?;
            require(
                self.output(&path, &["rev-parse", "--is-bare-repository"])? == "true",
                "仓库接入只接受所属 bare 仓库",
            )?;
            require(
                self.output(&path, &["remote", "get-url", "origin"])?
                    == format!("https://github.com/{repository}.git"),
                "仓库 origin 与冻结 GitHub 身份不符",
            )?;
            let reference = format!("refs/heads/{mainline}");
            let existing = self.git(
                &path,
                &["show-ref", "--verify", "--quiet", &reference],
                None,
            )?;
            if existing.code == 1 {
                let refspec = format!("{reference}:{reference}");
                let fetched = self.git_with_timeout(
                    &path,
                    &[
                        "fetch",
                        "--no-tags",
                        "--no-recurse-submodules",
                        "origin",
                        &refspec,
                    ],
                    None,
                    Duration::from_secs(180),
                )?;
                require(
                    fetched.code == 0,
                    "未完成的仓库下载暂无法续传，请检查 GitHub 登录和网络后核对重试",
                )?;
            } else {
                require(existing.code == 0, "仓库分支状态无法核对，保留原目录")?;
            }
            if self
                .git(&path, &["rev-parse", "--verify", "HEAD"], None)?
                .code
                != 0
            {
                self.output(&path, &["symbolic-ref", "HEAD", &reference])?;
            }
        }
        self.inspect_unlocked(project, repository, mainline)
    }
    fn inspect_unlocked(
        &self,
        project: &str,
        repository: &str,
        mainline: &str,
    ) -> Result<RepositoryFacts> {
        let path = self.repository(project)?;
        let remote = self.output(&path, &["remote", "get-url", "origin"])?;
        require(
            remote == format!("https://github.com/{repository}.git"),
            "仓库 origin 与冻结 GitHub 身份不符",
        )?;
        let head = self.oid(&path, &format!("refs/heads/{mainline}"))?;
        require(
            self.git(
                &path,
                &["cat-file", "-e", &format!("{head}:.gitmodules")],
                None,
            )?
            .code
                != 0,
            "首版不支持子模块项目",
        )?;
        Ok(RepositoryFacts {
            path: path.to_string_lossy().into(),
            repository: repository.into(),
            mainline: mainline.into(),
            head,
            object_format: self.output(&path, &["rev-parse", "--show-object-format"])?,
        })
    }
    pub fn inspect(
        &self,
        project: &str,
        repository: &str,
        mainline: &str,
    ) -> Result<RepositoryFacts> {
        let _lock = self.writer.lock().map_err(storage)?;
        self.inspect_unlocked(project, repository, mainline)
    }
    pub fn prepare_worktree(
        &self,
        project: &str,
        owner: &str,
        kind: &str,
        base: &str,
        operation: &str,
    ) -> Result<WorktreeReceipt> {
        self.prepare_named_worktree(
            project,
            owner,
            kind,
            base,
            &format!("devdesk/{kind}/{owner}"),
            operation,
        )
    }
    pub fn prepare_mainline(
        &self,
        project: &str,
        branch: &str,
        base: &str,
        operation: &str,
    ) -> Result<WorktreeReceipt> {
        self.prepare_named_worktree(project, project, "mainlines", base, branch, operation)
    }
    fn prepare_named_worktree(
        &self,
        project: &str,
        owner: &str,
        kind: &str,
        base: &str,
        branch: &str,
        operation: &str,
    ) -> Result<WorktreeReceipt> {
        let _lock = self.writer.lock().map_err(storage)?;
        checked_uuid(owner)?;
        require(
            ["features", "tasks", "candidates", "mainlines"].contains(&kind),
            "未知 worktree 归属",
        )?;
        require(valid_oid(base), "worktree 基线必须是固定 commit")?;
        let repo = self.repository(project)?;
        require(self.oid(&repo, base)? == base, "基线对象无效")?;
        let branch = branch.to_string();
        let parent = self
            .project(project)?
            .join("execution/worktrees")
            .join(kind);
        fs::create_dir_all(&parent).map_err(storage)?;
        reject_symlink(&parent)?;
        let path = parent.join(owner);
        reject_symlink(&path)?;
        let receipt = WorktreeReceipt {
            project: project.into(),
            owner: owner.into(),
            branch: branch.clone(),
            path: path.to_string_lossy().into(),
            base: base.into(),
            operation: operation.into(),
        };
        self.operation(
            project,
            operation,
            &json!({"kind":"worktree","spec":receipt}),
        )?;
        if !path.exists() {
            let refname = format!("refs/heads/{branch}");
            match self.existing_ref(&repo, &refname)? {
                Some(head) => require(head == base, "已有分支与新建基线不符")?,
                None => {
                    self.output(
                        &repo,
                        &["update-ref", &refname, base, &"0".repeat(base.len())],
                    )?;
                }
            }
            self.output(
                &repo,
                &[
                    "worktree",
                    "add",
                    "--",
                    receipt.path.as_str(),
                    branch.as_str(),
                ],
            )?;
        }
        self.verify_worktree(&repo, &path, &branch)?;
        Ok(receipt)
    }
    fn verify_worktree(&self, repo: &Path, worktree: &Path, branch: &str) -> Result<()> {
        let canonical = fs::canonicalize(worktree).map_err(storage)?;
        require(
            canonical.starts_with(&self.root) && canonical == worktree,
            "worktree 路径无效",
        )?;
        require(
            self.output(worktree, &["symbolic-ref", "--quiet", "HEAD"])?
                == format!("refs/heads/{branch}"),
            "worktree 分支归属不符",
        )?;
        let common = self.output(
            worktree,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        require(
            fs::canonicalize(common).map_err(storage)?
                == fs::canonicalize(self.output(
                    repo,
                    &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                )?)
                .map_err(storage)?,
            "worktree 指向其他仓库",
        )
    }
    fn context(&self, c: &GitContext) -> Result<PathBuf> {
        require(c.origin == Origin::External, "CliGit 拒绝模拟 GitContext")?;
        let path = self.repository(&c.project)?;
        require(
            c.workspace.path == path.to_string_lossy(),
            "冻结仓库路径不符",
        )?;
        Ok(path)
    }
    pub fn snapshot_worktree(
        &self,
        c: &GitContext,
        expected: &str,
        operation: &str,
    ) -> Result<String> {
        self.snapshot_worktree_scoped(c, expected, operation, &[".".into()], &[])
    }
    pub fn snapshot_worktree_scoped(
        &self,
        c: &GitContext,
        expected: &str,
        operation: &str,
        allowed_paths: &[String],
        protected_paths: &[String],
    ) -> Result<String> {
        let _lock = self.writer.lock().map_err(storage)?;
        let repo = self.context(c)?;
        let path = PathBuf::from(&c.source.worktree);
        self.verify_worktree(&repo, &path, &c.source.branch)?;
        require(valid_oid(expected), "必须冻结预期源版本")?;
        let reference = format!("refs/devdesk/snapshots/{operation}");
        self.operation(
            &c.project,
            operation,
            &json!({"kind":"snapshot","context":c,"expected":expected,"allowed_paths":allowed_paths,"protected_paths":protected_paths}),
        )?;
        if let Some(commit) = self.existing_ref(&repo, &reference)? {
            return Ok(commit);
        }
        require(self.oid(&path, "HEAD")? == expected, "源分支已改变")?;
        self.output(&path, &["add", "--all", "--", "."])?;
        let changed = self.git(
            &path,
            &[
                "diff-index",
                "--cached",
                "--name-only",
                "-z",
                expected,
                "--",
            ],
            None,
        )?;
        require(
            changed.code == 0 && !changed.truncated,
            "修改文件清单不可核对",
        )?;
        for name in changed.stdout.split(|b| *b == 0).filter(|v| !v.is_empty()) {
            let name = std::str::from_utf8(name).map_err(storage)?;
            require(
                !protected_paths
                    .iter()
                    .any(|p| name == p || name.starts_with(&format!("{p}/"))),
                "实现改动了冻结测试，需由测试角色另建修订和 Red 基线",
            )?;
            require(
                allowed_paths
                    .iter()
                    .any(|p| p == "." || name == p || name.starts_with(&format!("{p}/"))),
                "变更超出冻结写范围，保留现场供核对",
            )?;
        }
        let tree = self.output(&path, &["write-tree"])?;
        let commit = self.output(
            &repo,
            &[
                "commit-tree",
                &tree,
                "-p",
                expected,
                "-m",
                &format!("DevDesk task snapshot {operation}"),
            ],
        )?;
        require(valid_oid(&commit), "代码快照提交无效")?;
        let transaction = format!(
            "start\nupdate refs/heads/{} {commit} {expected}\ncreate {reference} {commit}\nprepare\ncommit\n",
            c.source.branch
        );
        let out = self.git(
            &repo,
            &["update-ref", "--stdin"],
            Some(transaction.into_bytes()),
        )?;
        require(out.code == 0, "源分支推进未知，需核对快照 ref")?;
        Ok(commit)
    }
    pub fn diff(&self, c: &GitContext, base: &str, head: &str) -> Result<String> {
        let repo = self.context(c)?;
        require(valid_oid(base) && valid_oid(head), "diff 必须使用固定对象")?;
        self.output(
            &repo,
            &["diff", "--no-ext-diff", "--no-textconv", base, head, "--"],
        )
    }
    pub fn validate_view(&self, c: &GitContext, revision: &str) -> Result<()> {
        let repo = self.context(c)?;
        let path = Path::new(&c.source.worktree);
        self.verify_worktree(&repo, path, &c.source.branch)?;
        require(
            self.oid(path, "HEAD")? == revision,
            "执行 worktree 偏离冻结版本",
        )?;
        require(
            self.output(path, &["status", "--porcelain", "--untracked-files=normal"])?
                .is_empty(),
            "执行 worktree 存在未封存修改，需核对来源",
        )
    }
    /// Export exact blobs without invoking archive filters or extracting an untrusted host tar stream.
    pub fn export_revision(
        &self,
        c: &GitContext,
        revision: &str,
        destination: &Path,
    ) -> Result<()> {
        let _lock = self.writer.lock().map_err(storage)?;
        let repo = self.context(c)?;
        require(
            valid_oid(revision) && self.oid(&repo, revision)? == revision,
            "测试代码对象无效",
        )?;
        let project = self.project(&c.project)?;
        let parent = destination
            .parent()
            .ok_or_else(|| err("blocked", "导出目录无父目录"))?;
        let parent = fs::canonicalize(parent).map_err(storage)?;
        require(
            parent.starts_with(project.join("execution")),
            "代码只能导出到所属项目执行目录",
        )?;
        reject_symlink(destination)?;
        let marker = parent.join("export-revision");
        reject_symlink(&marker)?;
        if destination.is_dir() {
            require(
                fs::read_to_string(&marker).map_err(storage)? == revision,
                "测试代码导出身份不符",
            )?;
            let manifest: Vec<(String, String, bool)> = serde_json::from_slice(
                &fs::read(parent.join("export-manifest.json")).map_err(storage)?,
            )
            .map_err(storage)?;
            fn count_files(path: &Path) -> Result<usize> {
                let mut count = 0;
                for entry in fs::read_dir(path).map_err(storage)? {
                    let entry = entry.map_err(storage)?;
                    let kind = entry.file_type().map_err(storage)?;
                    require(!kind.is_symlink(), "测试代码出现 symlink 漂移")?;
                    if kind.is_dir() {
                        count += count_files(&entry.path())?;
                    } else {
                        require(kind.is_file(), "测试代码出现特殊对象")?;
                        count += 1;
                    }
                }
                Ok(count)
            }
            require(
                count_files(destination)? == manifest.len(),
                "测试代码清单发生漂移",
            )?;
            for (name, digest, executable) in manifest {
                let path = destination.join(name);
                reject_symlink(&path)?;
                require(
                    fs::canonicalize(&path)
                        .map_err(storage)?
                        .starts_with(destination),
                    "测试代码目录漂移",
                )?;
                require(
                    format!("{:x}", Sha256::digest(fs::read(&path).map_err(storage)?)) == digest,
                    "测试代码内容偏离冻结对象",
                )?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    require(
                        (fs::metadata(path).map_err(storage)?.permissions().mode() & 0o111 != 0)
                            == executable,
                        "测试代码执行位发生漂移",
                    )?;
                }
            }
            return Ok(());
        }
        let staging = parent.join("export-staging");
        require(!staging.exists(), "存在未完成代码导出，请核对原操作")?;
        fs::create_dir(&staging).map_err(storage)?;
        let listing = self.git(
            &repo,
            &["ls-tree", "-r", "-z", "--full-tree", revision],
            None,
        )?;
        require(
            listing.code == 0 && !listing.truncated,
            "代码清单过大或不可读取",
        )?;
        let entries: Vec<_> = listing
            .stdout
            .split(|b| *b == 0)
            .filter(|v| !v.is_empty())
            .collect();
        require(entries.len() <= 2000, "首版代码导出最多 2000 个文件")?;
        let mut total = 0;
        let mut manifest = vec![];
        for entry in entries {
            let entry = std::str::from_utf8(entry).map_err(storage)?;
            let (metadata, name) = entry
                .split_once('\t')
                .ok_or_else(|| err("blocked", "代码清单无效"))?;
            let parts: Vec<_> = metadata.split(' ').collect();
            require(
                parts.len() == 3 && ["100644", "100755"].contains(&parts[0]) && parts[1] == "blob",
                "首版导出不支持 symlink、子模块或特殊对象",
            )?;
            require(
                !name.starts_with('/')
                    && name.split('/').all(|part| {
                        !part.is_empty() && part != "." && part != ".." && part != ".git"
                    }),
                "代码路径越界",
            )?;
            let blob = self.git(&repo, &["cat-file", "blob", parts[2]], None)?;
            total += blob.stdout.len();
            require(
                blob.code == 0 && !blob.truncated && total <= 32 * 1024 * 1024,
                "代码文件或总量超过导出限制",
            )?;
            manifest.push((
                name.to_string(),
                format!("{:x}", Sha256::digest(&blob.stdout)),
                parts[0] == "100755",
            ));
            let path = staging.join(name);
            fs::create_dir_all(path.parent().unwrap()).map_err(storage)?;
            fs::write(&path, &blob.stdout).map_err(storage)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(
                    &path,
                    fs::Permissions::from_mode(if parts[0] == "100755" { 0o700 } else { 0o600 }),
                )
                .map_err(storage)?;
            }
            File::open(path)
                .and_then(|f| f.sync_all())
                .map_err(storage)?;
        }
        let manifest_path = parent.join("export-manifest.json");
        reject_symlink(&manifest_path)?;
        fs::write(
            &manifest_path,
            serde_json::to_vec(&manifest).map_err(storage)?,
        )
        .map_err(storage)?;
        File::open(&manifest_path)
            .and_then(|f| f.sync_all())
            .map_err(storage)?;
        fs::write(&marker, revision).map_err(storage)?;
        File::open(&marker)
            .and_then(|f| f.sync_all())
            .map_err(storage)?;
        fs::rename(&staging, destination).map_err(storage)?;
        Ok(())
    }
    fn target_clean(&self, c: &GitContext, expected: &str) -> Result<()> {
        let repo = self.context(c)?;
        let path = PathBuf::from(&c.target.worktree);
        self.verify_worktree(&repo, &path, &c.target.branch)?;
        require(
            self.output(
                &path,
                &["status", "--porcelain", "--untracked-files=normal"],
            )?
            .is_empty(),
            "目标 worktree 存在未提交修改，不能推进",
        )?;
        require(self.oid(&path, "HEAD")? == expected, "目标已变化")
    }
    fn sync_target(&self, c: &GitContext, expected: &str, candidate: &str) -> Result<()> {
        let repo = self.context(c)?;
        let path = PathBuf::from(&c.target.worktree);
        self.verify_worktree(&repo, &path, &c.target.branch)?;
        require(
            self.oid(&path, "HEAD")? == candidate,
            "目标在 CAS 后被其他写者推进，保留核对状态",
        )?;
        require(
            self.git(&path, &["diff-files", "--quiet", "--"], None)?
                .code
                == 0,
            "目标文件存在未暂存修改，不能同步",
        )?;
        let index = self.output(&path, &["write-tree"])?;
        let old_tree = self.output(&repo, &["rev-parse", &format!("{expected}^{{tree}}")])?;
        let new_tree = self.output(&repo, &["rev-parse", &format!("{candidate}^{{tree}}")])?;
        require(
            index == old_tree || index == new_tree,
            "目标 index 非冻结基线或候选，需人工核对",
        )?;
        if index != new_tree {
            self.output(&path, &["read-tree", "-u", "-m", expected, candidate])?;
        }
        require(
            self.output(
                &path,
                &["status", "--porcelain", "--untracked-files=normal"],
            )?
            .is_empty(),
            "目标 worktree 尚未同步至候选",
        )
    }
    fn remote_head(&self, repo: &Path, branch: &str) -> Result<Option<String>> {
        let reference = format!("refs/heads/{branch}");
        self.output(repo, &["check-ref-format", &reference])?;
        let output = self.output(repo, &["ls-remote", "--heads", "origin", &reference])?;
        if output.is_empty() {
            return Ok(None);
        }
        let mut lines = output.lines();
        let line = lines.next().unwrap();
        require(lines.next().is_none(), "远端 ref 不唯一")?;
        let (oid, name) = line
            .split_once('\t')
            .ok_or_else(|| err("adapter_contract", "远端 ref 结构错误"))?;
        require(valid_oid(oid) && name == reference, "远端 ref 身份不符")?;
        Ok(Some(oid.into()))
    }
    fn publish_ref(
        &self,
        c: &GitContext,
        branch: &str,
        revision: &str,
        expected: Option<&str>,
        operation: &str,
        stage: PublishStage,
    ) -> Result<RepositoryReceipt> {
        let _lock = self.writer.lock().map_err(storage)?;
        let repo = self.context(c)?;
        require(
            valid_oid(revision) && expected.is_none_or(valid_oid),
            "远端操作缺少固定 SHA",
        )?;
        require(
            branch == c.target.branch || branch.starts_with("devdesk/candidates/"),
            "发布分支不属于冻结上下文",
        )?;
        self.inspect_unlocked(&c.project, &c.workspace.repository, &c.workspace.mainline)?;
        let policy = crate::github::GitHubPullRequests
            .diagnose(&c.workspace.repository, &c.target.branch)?;
        require(policy.compatible, "GitHub 规则不兼容，禁止推送")?;
        self.operation(&c.project,operation,&json!({"kind":"publish_ref","context":c,"branch":branch,"revision":revision,"expected":expected,"stage":stage}))?;
        let actual = self.remote_head(&repo, branch)?;
        if actual.as_deref() != Some(revision) {
            require(
                actual.as_deref() == expected,
                "远端目标已变化，精确 lease 拒绝覆盖",
            )?;
            if stage == PublishStage::RemoteAdvance {
                require(
                    expected.is_some()
                        && self.output(&repo, &["rev-list", "--parents", "-n", "1", revision])?
                            == format!("{revision} {}", expected.unwrap()),
                    "远端接纳只能新增冻结的单父候选",
                )?;
            }
            let reference = format!("refs/heads/{branch}");
            let lease = format!("--force-with-lease={reference}:{}", expected.unwrap_or(""));
            let refspec = format!("{revision}:{reference}");
            let output = self.git(
                &repo,
                &["push", "--porcelain", &lease, "origin", &refspec],
                None,
            )?;
            // Re-read even when the subprocess lost/failed its acknowledgement.
            require(
                self.remote_head(&repo, branch)?.as_deref() == Some(revision),
                if output.code == 0 {
                    "远端写入回执未确认"
                } else {
                    "精确 lease 推送未成功，需核对远端目标"
                },
            )?;
        }
        Ok(RepositoryReceipt::PublishedRef {
            revision: revision.into(),
            branch: branch.into(),
        })
    }
    fn refresh_mainline(
        &self,
        c: &GitContext,
        expected: &str,
        revision: &str,
        operation: &str,
    ) -> Result<RepositoryReceipt> {
        let _lock = self.writer.lock().map_err(storage)?;
        let repo = self.context(c)?;
        require(
            valid_oid(expected)
                && valid_oid(revision)
                && c.target.owner == c.project
                && c.target.branch == c.workspace.mainline,
            "主干同步定位或 SHA 不符",
        )?;
        self.inspect_unlocked(&c.project, &c.workspace.repository, &c.workspace.mainline)?;
        let path = PathBuf::from(&c.target.worktree);
        require(
            path == self
                .project(&c.project)?
                .join("execution/worktrees/mainlines")
                .join(&c.project),
            "主干同步 worktree 越界",
        )?;
        self.verify_worktree(&repo, &path, &c.target.branch)?;
        require(
            self.output(&path, &["status", "--porcelain"])?.is_empty(),
            "主干工作区存在未提交修改",
        )?;
        self.operation(
            &c.project,
            operation,
            &json!({"kind":"refresh_mainline","context":c,"expected":expected,"revision":revision}),
        )?;
        let head = self.oid(&path, "HEAD")?;
        require(head == expected || head == revision, "主干在同步前已变化")?;
        if head != revision {
            require(
                self.remote_head(&repo, &c.target.branch)?.as_deref() == Some(revision),
                "远端主干已再次变化，请重新观察后同步",
            )?;
            self.output(&repo, &["fetch", "--no-tags", "origin", revision])?;
            require(
                self.oid(&repo, "FETCH_HEAD")? == revision,
                "抓取回执与冻结 SHA 不符",
            )?;
            self.output(&path, &["merge", "--ff-only", revision])?;
        }
        require(
            self.oid(&path, "HEAD")? == revision
                && self.output(&path, &["status", "--porcelain"])?.is_empty(),
            "主干快进回执未确认",
        )?;
        Ok(RepositoryReceipt::MainlineRefreshed {
            previous: expected.into(),
            revision: revision.into(),
        })
    }
    fn candidate_unlocked(
        &self,
        c: &GitContext,
        source: &str,
        target: &str,
        operation: &str,
    ) -> Result<String> {
        let repo = self.context(c)?;
        require(
            valid_oid(source) && valid_oid(target),
            "候选必须使用固定源与目标对象",
        )?;
        self.operation(
            &c.project,
            operation,
            &json!({"kind":"candidate","context":c,"source":source,"target":target}),
        )?;
        let reference = format!("refs/devdesk/candidates/{operation}");
        if let Some(commit) = self.existing_ref(&repo, &reference)? {
            require(
                self.output(&repo, &["rev-list", "--parents", "-n", "1", &commit])?
                    == format!("{commit} {target}"),
                "候选父提交身份不符",
            )?;
            return Ok(commit);
        }
        let merge = self.git(&repo, &["merge-tree", "--write-tree", target, source], None)?;
        require(
            merge.code == 0 && !merge.truncated,
            "候选重放存在冲突；保留源分支，需新建修复执行",
        )?;
        let tree = String::from_utf8(merge.stdout)
            .map_err(storage)?
            .lines()
            .next()
            .unwrap_or("")
            .to_owned();
        require(valid_oid(&tree), "候选 tree 无效")?;
        let commit = self.output(
            &repo,
            &[
                "commit-tree",
                &tree,
                "-p",
                target,
                "-m",
                &format!("DevDesk candidate {operation}\n\nSource: {source}\nTarget: {target}"),
            ],
        )?;
        require(valid_oid(&commit), "候选 commit 无效")?;
        self.output(
            &repo,
            &["update-ref", &reference, &commit, &"0".repeat(commit.len())],
        )?;
        Ok(commit)
    }
}
impl ports::Git for CliGit {
    fn identity(&self) -> String {
        "devdesk_adapters::CliGit/v1".into()
    }
    fn repository_operation(&self, op: &RepositoryOperation) -> Result<RepositoryReceipt> {
        let make = |r: WorktreeReceipt, kind: WorktreeKind| RepositoryWorktree {
            id: r.owner.clone(),
            project_id: op.project_id.clone(),
            owner_id: r.owner,
            kind,
            branch: r.branch,
            path: r.path,
            base: r.base,
            operation_id: op.id.clone(),
        };
        match &op.action {
            RepositoryAction::RefreshMainline {
                context,
                expected,
                revision,
            } => self.refresh_mainline(context, expected, revision, &op.id),
            RepositoryAction::Candidate {
                context,
                source,
                target,
                ..
            } => Ok(RepositoryReceipt::Candidate {
                revision: self.candidate(context, source, target, &op.id)?,
            }),
            RepositoryAction::PublishRef {
                context,
                branch,
                revision,
                expected,
                stage,
                ..
            } => self.publish_ref(
                context,
                branch,
                revision,
                expected.as_deref(),
                &op.id,
                *stage,
            ),
            RepositoryAction::PullRequest { .. } => {
                Err(err("unsupported", "PR API 属于独立 PullRequests 端口"))
            }
            RepositoryAction::Snapshot {
                run,
                context,
                expected,
                allowed_paths,
                protected_paths,
            } => {
                require(
                    run == &op.id && context.project == op.project_id,
                    "Git 封存操作归属不符",
                )?;
                Ok(RepositoryReceipt::Snapshot {
                    revision: self.snapshot_worktree_scoped(
                        context,
                        expected,
                        &op.id,
                        allowed_paths,
                        protected_paths,
                    )?,
                })
            }
            RepositoryAction::Connect {
                repository,
                mainline,
            } => {
                let policy = crate::github::GitHubPullRequests.diagnose(repository, mainline)?;
                require(
                    policy.compatible,
                    &format!(
                        "GitHub 仓库暂不可接入：{}",
                        policy.blocked_reasons.join("；")
                    ),
                )?;
                let facts = self.clone_repository(&op.project_id, repository, mainline, &op.id)?;
                let phase = uuid::Uuid::from_u128(
                    uuid::Uuid::parse_str(&op.id).map_err(storage)?.as_u128()
                        ^ 0x6d61696e6c696e650000000000000000,
                )
                .to_string();
                let work = self.prepare_mainline(&op.project_id, mainline, &facts.head, &phase)?;
                Ok(RepositoryReceipt::Connected {
                    policy,
                    workspace: Workspace {
                        repository: facts.repository,
                        path: facts.path,
                        mainline: facts.mainline,
                    },
                    head: facts.head,
                    worktree: make(work, WorktreeKind::Mainline),
                })
            }
            RepositoryAction::CreateWorktree {
                owner,
                worktree_kind,
                base,
            } => {
                let kind = match worktree_kind {
                    WorktreeKind::Feature => "features",
                    WorktreeKind::Task => "tasks",
                    WorktreeKind::Candidate => "candidates",
                    WorktreeKind::Mainline => {
                        return Err(err("blocked", "Mainline 必须在仓库登记时创建"));
                    }
                };
                let work = self.prepare_worktree(&op.project_id, owner, kind, base, &op.id)?;
                Ok(RepositoryReceipt::Worktree {
                    worktree: make(work, *worktree_kind),
                })
            }
        }
    }
    fn execution_revision(&self, a: &Attempt, _: &str) -> Result<String> {
        let c = a
            .git_context
            .as_ref()
            .ok_or_else(|| err("blocked", "缺少冻结代码定位"))?;
        let repo = self.context(c)?;
        self.oid(&repo, &format!("refs/heads/{}", c.source.branch))
    }
    fn candidate(
        &self,
        c: &GitContext,
        source: &str,
        target: &str,
        operation: &str,
    ) -> Result<String> {
        let _lock = self.writer.lock().map_err(storage)?;
        self.candidate_unlocked(c, source, target, operation)
    }
    fn diff(&self, c: &GitContext, base: &str, head: &str) -> Result<String> {
        CliGit::diff(self, c, base, head)
    }
    fn preflight_advance(&self, c: &GitContext, expected: &str) -> Result<()> {
        let _lock = self.writer.lock().map_err(storage)?;
        self.context(c)?;
        self.inspect_unlocked(&c.project, &c.workspace.repository, &c.workspace.mainline)?;
        let policy = crate::github::GitHubPullRequests
            .diagnose(&c.workspace.repository, &c.target.branch)?;
        require(policy.compatible, "GitHub 规则不兼容，禁止本地 CAS")?;
        require(
            policy.head.as_deref() == Some(expected),
            "远端目标已变化，禁止先行本地 CAS",
        )
    }
    fn advance(
        &self,
        c: &GitContext,
        actual: &str,
        expected: &str,
        candidate: &str,
        operation: &str,
    ) -> Result<String> {
        let _lock = self.writer.lock().map_err(storage)?;
        let repo = self.context(c)?;
        require(
            actual == expected && valid_oid(expected) && valid_oid(candidate),
            "CAS 预期目标不符",
        )?;
        let request =
            json!({"kind":"advance","context":c,"expected":expected,"candidate":candidate});
        self.operation(&c.project, operation, &request)?;
        let marker = format!("refs/devdesk/advances/{operation}");
        if let Some(receipt) = self.existing_ref(&repo, &marker)? {
            require(receipt == candidate, "推进回执与候选不符")?;
            self.sync_target(c, expected, candidate)?;
            return Ok(receipt);
        }
        self.target_clean(c, expected)?;
        require(
            self.output(&repo, &["rev-list", "--parents", "-n", "1", candidate])?
                == format!("{candidate} {expected}"),
            "只能接纳目标恰好增加一个提交的同一候选",
        )?;
        let target = format!("refs/heads/{}", c.target.branch);
        self.output(&repo, &["check-ref-format", &target])?;
        let input = format!(
            "start\nupdate {target} {candidate} {expected}\ncreate {marker} {candidate}\nprepare\ncommit\n"
        );
        let result = self.git(&repo, &["update-ref", "--stdin"], Some(input.into_bytes()))?;
        if self.existing_ref(&repo, &marker)?.as_deref() == Some(candidate) {
            self.sync_target(c, expected, candidate)?;
            return Ok(candidate.into());
        }
        require(
            result.code == 0,
            "目标已变化或 CAS 未成功；不能覆盖并发写者",
        )?;
        Err(err("needs_reconciliation", "CAS 回执未知"))
    }
    fn reconcile_advance(&self, c: &GitContext, operation: &str) -> Result<AdvanceObservation> {
        let _lock = self.writer.lock().map_err(storage)?;
        checked_uuid(operation)?;
        let repo = self.context(c)?;
        let path = self
            .project(&c.project)?
            .join("execution/git-operations")
            .join(format!("advance-{operation}.json"));
        let legacy = path.parent().unwrap().join(format!("{operation}.json"));
        reject_symlink(&path)?;
        reject_symlink(&legacy)?;
        let path = if path.is_file() {
            path
        } else if legacy.is_file()
            && serde_json::from_slice::<Value>(&fs::read(&legacy).map_err(storage)?)
                .map_err(storage)?["kind"]
                == "advance"
        {
            legacy
        } else {
            // The marker and target update are one ref transaction, always after this intent file.
            require(
                self.existing_ref(&repo, &format!("refs/devdesk/advances/{operation}"))?
                    .is_none(),
                "推进 marker 缺少原意图，需要核对",
            )?;
            return Ok(AdvanceObservation::NotApplied {
                actual_target: self.oid(&repo, &format!("refs/heads/{}", c.target.branch))?,
            });
        };
        let request: Value =
            serde_json::from_slice(&fs::read(path).map_err(storage)?).map_err(storage)?;
        require(
            request["kind"] == "advance" && request["context"] == json!(c),
            "推进核对请求身份不符",
        )?;
        if let Some(revision) =
            self.existing_ref(&repo, &format!("refs/devdesk/advances/{operation}"))?
        {
            require(request["candidate"] == revision, "推进 marker 身份不符")?;
            self.sync_target(
                c,
                request["expected"]
                    .as_str()
                    .ok_or_else(|| err("blocked", "推进缺少冻结基线"))?,
                &revision,
            )?;
            return Ok(AdvanceObservation::Applied { revision });
        }
        Ok(AdvanceObservation::NotApplied {
            actual_target: self.oid(&repo, &format!("refs/heads/{}", c.target.branch))?,
        })
    }
}
