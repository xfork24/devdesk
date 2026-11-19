//! A native writer may continue only the exact quiescent work recorded by the
//! kernel for that same Session, execution, frozen input and permission scope.
use super::*;
use rusqlite::OpenFlags;
use std::{collections::BTreeSet, io::Read};

impl CliGit {
    pub(super) fn pending_work_state(&self, c: &GitContext, revision: &str) -> Result<Value> {
        let repo = self.context(c)?;
        let path = Path::new(&c.source.worktree);
        self.verify_worktree(&repo, path, &c.source.branch)?;
        require(
            self.oid(path, "HEAD")? == revision,
            "工作树已偏离原冻结版本",
        )?;
        require(
            self.output(path, &["ls-files", "--unmerged"])?.is_empty(),
            "工作树仍有未解决的合并冲突",
        )?;
        let mut names = BTreeSet::new();
        for args in [
            vec!["diff", "--name-only", "--no-renames", "-z", "HEAD", "--"],
            vec![
                "diff",
                "--cached",
                "--name-only",
                "--no-renames",
                "-z",
                "HEAD",
                "--",
            ],
            vec!["ls-files", "--others", "--exclude-standard", "-z"],
        ] {
            let output = self.git(path, &args, None)?;
            require(output.code == 0, "工作树文件清单读取失败")?;
            for bytes in output.stdout.split(|b| *b == 0).filter(|b| !b.is_empty()) {
                let name = std::str::from_utf8(bytes).map_err(storage)?;
                require(
                    Path::new(name)
                        .components()
                        .all(|p| matches!(p, std::path::Component::Normal(_))),
                    "工作树文件越界",
                )?;
                names.insert(name.to_owned());
            }
        }
        require(names.len() <= 8192, "未封存文件超过核对预算")?;
        let mut files = Vec::new();
        let mut total = 0u64;
        for name in names {
            let mut full = path.to_path_buf();
            for part in Path::new(&name).components() {
                full.push(part);
                if full.exists() || full.is_symlink() {
                    reject_symlink(&full)?;
                }
            }
            let metadata = match fs::symlink_metadata(&full) {
                Ok(meta) => meta,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    files.push(json!({"path":name,"kind":"deleted"}));
                    continue;
                }
                Err(e) => return Err(storage(e)),
            };
            require(metadata.is_file(), "未封存内容不是普通文件，保留现场待核对")?;
            total += metadata.len();
            require(total <= 64 * 1024 * 1024, "未封存内容超过核对预算")?;
            let mut hash = Sha256::new();
            let mut file = File::open(&full).map_err(storage)?;
            let mut buffer = [0u8; 65536];
            let mut read = 0u64;
            loop {
                let n = file.read(&mut buffer).map_err(storage)?;
                if n == 0 {
                    break;
                }
                read += n as u64;
                require(read <= metadata.len(), "核对期间文件仍在增长，保留现场")?;
                hash.update(&buffer[..n]);
            }
            require(read == metadata.len(), "核对期间文件长度已变化，保留现场")?;
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o111 != 0
            };
            #[cfg(not(unix))]
            let executable = false;
            files.push(json!({"path":name,"kind":"file","bytes":metadata.len(),"executable":executable,"sha256":format!("{:x}",hash.finalize())}));
        }
        let index = self.git(
            path,
            &[
                "diff",
                "--cached",
                "--binary",
                "--full-index",
                "--no-ext-diff",
                "--no-textconv",
                "HEAD",
                "--",
            ],
            None,
        )?;
        require(index.code == 0, "原暂存区无法核对")?;
        Ok(
            json!({"revision":revision,"files":files,"index_sha256":format!("{:x}",Sha256::digest(&index.stdout))}),
        )
    }

    pub(super) fn native_workspace_continuation(
        &self,
        c: &GitContext,
        revision: &str,
    ) -> Result<bool> {
        let database = self.project(&c.project)?.join("engineering.sqlite3");
        if !database.is_file() {
            return Ok(false);
        }
        let conn = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(storage)?;
        conn.busy_timeout(Duration::from_secs(5)).map_err(storage)?;
        // The core has already reserved a unique writer and frozen its original
        // startup request. Merely having an old Session is not admission.
        let mut q = conn.prepare("SELECT o.data FROM governance_records o JOIN turns t ON t.id=o.definition_id AND t.status='dispatching' JOIN executions e ON e.id=json_extract(o.data,'$.execution_id') AND e.writer_revoked=0 AND e.status='prepared' WHERE o.type='conversation_open_intent' AND json_extract(o.data,'$.status')='prepared' AND json_extract(o.data,'$.request.code_view.context.source.worktree')=?1 LIMIT 2").map_err(storage)?;
        let requests = q
            .query_map([&c.source.worktree], |r| r.get::<_, String>(0))
            .map_err(storage)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage)?;
        if requests.len() != 1 {
            return Ok(false);
        }
        let pending: Value = serde_json::from_str(&requests[0]).map_err(storage)?;
        let request = &pending["request"];
        let view = json!({"context":c,"revision":revision});
        let authority = &pending["authority"]["workflow"];
        let scope = &request["session"]["runtime_binding"]["permissions"]["scope"];
        if !scope.is_object()
            || request["code_view"] != view
            || !authority.is_object()
            || request["session"]["id"] != pending["session_id"]
            || request["session"]["id"] != authority["session_id"]
            || request["configured_task"]["source"]["execution_id"] != authority["execution_id"]
            || request["execution"]["operation_id"] != pending["worker_operation_id"]
        {
            return Ok(false);
        }
        let mut q = conn.prepare("SELECT w.data FROM governance_records w JOIN turns t ON t.id=json_extract(w.data,'$.turn_id') AND t.status='completed' WHERE w.type='workspace_continuation_checkpoint' AND w.immutable=1 AND w.definition_id=?1 ORDER BY w.rowid DESC LIMIT 1").map_err(storage)?;
        let saved = q
            .query_map([pending["session_id"].as_str().unwrap_or("")], |r| {
                r.get::<_, String>(0)
            })
            .map_err(storage)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage)?;
        let Some(saved) = saved.first() else {
            return Ok(false);
        };
        let checkpoint: Value = serde_json::from_str(saved).map_err(storage)?;
        if checkpoint["issuer"] != "kernel_workspace_checkpoint"
            || checkpoint["authority"] != *authority
            || checkpoint["view"] != view
            || checkpoint["authorization"] != *scope
            || checkpoint["agent_revision_id"] != request["session"]["agent"]
        {
            return Ok(false);
        }
        let state = self.pending_work_state(c, revision)?;
        if checkpoint["state"] != state {
            return Ok(false);
        }
        let within = |path: &str, parent: &str| {
            parent == "." || path == parent || path.starts_with(&format!("{parent}/"))
        };
        for file in state["files"].as_array().into_iter().flatten() {
            let name = file["path"].as_str().unwrap_or("");
            if !scope["writable_paths"].as_array().is_some_and(|paths| {
                paths
                    .iter()
                    .any(|p| p.as_str().is_some_and(|p| within(name, p)))
            }) || scope["read_only_paths"].as_array().is_some_and(|paths| {
                paths
                    .iter()
                    .any(|p| p.as_str().is_some_and(|p| within(name, p)))
            }) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
