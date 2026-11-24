//! Compare committed Git objects, bounding patch expansion before reading blobs.
//! Large objects remain addressable by their exact names and object identities.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

const INLINE_BYTES: u64 = 96 * 1024;
const FILE_BYTES: u64 = 24 * 1024;
const COMPLETE_BYTES: u64 = 384 * 1024;

impl CliGit {
    pub(super) fn check_new_snapshot_files(&self, path: &Path, allowed: &[String]) -> Result<()> {
        let output = self.git(
            path,
            &["ls-files", "--others", "--exclude-standard", "-z"],
            None,
        )?;
        require(
            output.code == 0 && !output.truncated,
            "新增源文件清单超过读取预算",
        )?;
        let mut total = 0u64;
        for raw in output
            .stdout
            .split(|b| *b == 0)
            .filter(|raw| !raw.is_empty())
        {
            let name = std::str::from_utf8(raw).map_err(storage)?;
            if allowed.iter().any(|scope| {
                scope != "." && (scope == name || name.starts_with(&format!("{scope}/")))
            }) {
                continue;
            }
            let bytes = fs::symlink_metadata(path.join(name))
                .map_err(storage)?
                .len();
            total = total.saturating_add(bytes);
            if bytes > 8 * 1024 * 1024 || total > 64 * 1024 * 1024 {
                return Err(err("snapshot_source_limit", format!("新增文件 {name} 含未声明的大体积内容；运行依赖应放入 DEVDESK_RUNTIME_DIR，项目所需的大资产请在 allowed_paths 明确列出"))
                    .technical_not_applied()
                    .with_context("CliGit::check_new_snapshot_files", json!({"path":name,"bytes":bytes,"unscoped_bytes":total,"file_limit_bytes":8*1024*1024,"total_limit_bytes":64*1024*1024})));
            }
        }
        Ok(())
    }

    pub(super) fn ensure_runtime_excludes(&self, repo: &Path) -> Result<()> {
        // Lower priority than the repository's .gitignore. Already tracked
        // files are never discarded; new source and validation evidence stay
        // eligible for staging while conventional runtime caches do not.
        const RULES: &str = "\n# DevDesk runtime dependencies (not project artifacts)\n**/node_modules\n**/__pycache__/\n**/.venv\n**/.pytest_cache/\n**/.npm/\n**/.cache/\nevidence/.devdesk/*/browsers/\nevidence/.devdesk/*/npm-cache/\nevidence/.devdesk/*/apt/\nevidence/.devdesk/*/deps/\nevidence/.devdesk/*/sysroot/\nevidence/.devdesk/*/cache/\nevidence/.devdesk/*/config/\nevidence/.devdesk/*/font-cache/\nevidence/.devdesk/*/tmp/\n";
        let common = PathBuf::from(self.output(
            repo,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?);
        require(
            fs::canonicalize(&common)
                .map_err(storage)?
                .starts_with(repo),
            "Git 忽略规则路径不属于当前仓库",
        )?;
        let info = common.join("info");
        reject_symlink(&info)?;
        fs::create_dir_all(&info).map_err(storage)?;
        let path = info.join("exclude");
        reject_symlink(&path)?;
        let previous = match fs::read_to_string(&path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(storage(error)),
        };
        if !previous.contains(RULES) {
            use std::io::Write;
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(storage)?
                .write_all(RULES.as_bytes())
                .map_err(storage)?;
        }
        Ok(())
    }

    pub fn diff(&self, context: &GitContext, base: &str, head: &str) -> Result<String> {
        require(valid_oid(base) && valid_oid(head), "diff 必须使用固定对象")?;
        let repo = self.context(context)?;
        // Raw records contain only modes, object IDs, status and paths. They
        // never inflate binary patches or invoke a worktree's filters.
        let raw = self.git(
            &repo,
            &[
                "diff",
                "--raw",
                "--no-abbrev",
                "--no-renames",
                "--no-ext-diff",
                "--no-textconv",
                "-z",
                base,
                head,
                "--",
            ],
            None,
        )?;
        require(
            raw.code == 0 && !raw.truncated,
            "Git 差异文件清单超过读取预算",
        )?;
        let mut parts = raw.stdout.split(|b| *b == 0).filter(|v| !v.is_empty());
        let mut files = Vec::new();
        let mut objects = BTreeSet::new();
        while let Some(metadata) = parts.next() {
            let metadata = std::str::from_utf8(metadata).map_err(storage)?;
            let path = std::str::from_utf8(
                parts
                    .next()
                    .ok_or_else(|| err("adapter_contract", "Git 差异缺少文件名"))?,
            )
            .map_err(storage)?;
            let fields = metadata.split_whitespace().collect::<Vec<_>>();
            require(fields.len() == 5, "Git 差异对象清单无效")?;
            let ids = [fields[2].to_owned(), fields[3].to_owned()];
            for id in &ids {
                if id.bytes().any(|b| b != b'0') {
                    require(valid_oid(id), "Git 差异对象身份无效")?;
                    objects.insert(id.clone());
                }
            }
            files.push((path.to_owned(), metadata.to_owned(), ids));
        }
        let mut sizes = BTreeMap::new();
        if !objects.is_empty() {
            let input = objects
                .into_iter()
                .map(|id| format!("{id}\n"))
                .collect::<String>();
            let output = self.git(
                &repo,
                &["cat-file", "--batch-check=%(objectname) %(objectsize)"],
                Some(input.into_bytes()),
            )?;
            require(
                output.code == 0 && !output.truncated,
                "Git 差异对象大小读取未完成",
            )?;
            for line in std::str::from_utf8(&output.stdout)
                .map_err(storage)?
                .lines()
            {
                let (id, size) = line
                    .split_once(' ')
                    .ok_or_else(|| err("adapter_contract", "Git 对象大小缺失"))?;
                sizes.insert(id.to_owned(), size.parse::<u64>().map_err(storage)?);
            }
        }
        let weight = |ids: &[String; 2]| {
            ids.iter()
                .map(|id| sizes.get(id).copied().unwrap_or(0))
                .sum::<u64>()
        };
        let total = files.iter().map(|(_, _, ids)| weight(ids)).sum::<u64>();
        let args = [
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--binary",
            "--full-index",
            "--find-renames",
            base,
            head,
            "--",
        ];
        if total <= COMPLETE_BYTES {
            let output = self.git(&repo, &args, None)?;
            require(output.code == 0, "Git 固定版本差异读取失败")?;
            if !output.truncated {
                return String::from_utf8(output.stdout)
                    .map(|text| text.trim_end().to_owned())
                    .map_err(storage);
            }
        }
        let mut report = format!(
            "# DevDesk bounded Git diff\nbase: {base}\nhead: {head}\nfiles: {}\npatch_complete: false\n完整文件清单如下；大文件补丁未内联。文件内容由下列 Git 对象固定，可在对应冻结工作区按路径读取。此摘要不能当作已完整审查所有文件。\n\n",
            files.len()
        );
        for (path, metadata, _) in &files {
            report.push_str(&format!(
                "{metadata}\t{}\n",
                serde_json::to_string(path).map_err(storage)?
            ));
        }
        // Prefer shallow project files before generated evidence subtrees.
        files.sort_by_key(|(path, _, _)| (path.matches('/').count(), path.clone()));
        let mut remaining = INLINE_BYTES;
        let mut selected = Vec::new();
        for (path, _, ids) in &files {
            let bytes = weight(ids);
            if bytes <= FILE_BYTES && bytes <= remaining && selected.len() < 128 {
                remaining -= bytes;
                selected.push(format!(":(literal){path}"));
            }
        }
        if !selected.is_empty() {
            let args = args
                .into_iter()
                .chain(selected.iter().map(String::as_str))
                .collect::<Vec<_>>();
            report.push_str("\n# Inline patches for bounded files\n");
            report.push_str(&self.output(&repo, &args)?);
        }
        require(report.len() <= 1024 * 1024, "Git 差异清单超过单页预算")?;
        Ok(report)
    }
}
