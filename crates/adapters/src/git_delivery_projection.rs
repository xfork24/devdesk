//! Construct a code-only commit without checking out, editing, or advancing the
//! source Feature, its Tasks, or main. No remote effect happens here.
use super::*;
use std::collections::BTreeMap;

type TreeEntries = BTreeMap<String, (String, String, String)>;
impl CliGit {
    fn delivery_tree_entries(&self, repo: &Path, tree: Option<&str>) -> Result<TreeEntries> {
        let Some(tree) = tree else {
            return Ok(BTreeMap::new());
        };
        let output = self.git(repo, &["ls-tree", "-z", tree], None)?;
        require(
            output.code == 0 && !output.truncated,
            "交付 tree 读取未完成",
        )?;
        let mut entries = BTreeMap::new();
        for raw in output.stdout.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let text = std::str::from_utf8(raw).map_err(storage)?;
            let (metadata, name) = text
                .split_once('\t')
                .ok_or_else(|| err("adapter_contract", "tree 条目缺少名称"))?;
            let fields: Vec<_> = metadata.split_whitespace().collect();
            require(
                fields.len() == 3 && valid_oid(fields[2]) && !name.contains('/'),
                "tree 条目无效",
            )?;
            entries.insert(
                name.into(),
                (fields[0].into(), fields[1].into(), fields[2].into()),
            );
        }
        Ok(entries)
    }
    fn delivery_write_tree(&self, repo: &Path, entries: &TreeEntries) -> Result<String> {
        let mut input = vec![];
        for (name, (mode, kind, sha)) in entries {
            input.extend_from_slice(format!("{mode} {kind} {sha}\t{name}\0").as_bytes());
        }
        let out = self.git(repo, &["mktree", "-z"], Some(input))?;
        require(out.code == 0 && !out.truncated, "交付 tree 构建未确认")?;
        let sha = String::from_utf8(out.stdout)
            .map_err(storage)?
            .trim()
            .to_owned();
        require(valid_oid(&sha), "交付 tree 身份无效")?;
        Ok(sha)
    }
    fn restore_validation_path(
        &self,
        repo: &Path,
        tree: &str,
        baseline: Option<&str>,
        parts: &[&str],
    ) -> Result<String> {
        let mut entries = self.delivery_tree_entries(repo, Some(tree))?;
        let original = self.delivery_tree_entries(repo, baseline)?;
        let key = parts[0];
        if parts.len() == 1 {
            entries.remove(key);
            if let Some(value) = original.get(key) {
                entries.insert(key.into(), value.clone());
            }
        } else {
            for rows in [&entries, &original] {
                require(
                    rows.get(key).is_none_or(|row| row.1 == "tree"),
                    "验证材料父路径与正式文件冲突",
                )?;
            }
            let empty = self.delivery_write_tree(repo, &BTreeMap::new())?;
            let child = entries
                .get(key)
                .map_or(empty.as_str(), |row| row.2.as_str());
            let base_child = original.get(key).map(|row| row.2.as_str());
            let next = self.restore_validation_path(repo, child, base_child, &parts[1..])?;
            if next == empty {
                entries.remove(key);
            } else {
                entries.insert(key.into(), ("040000".into(), "tree".into(), next));
            }
        }
        self.delivery_write_tree(repo, &entries)
    }
    pub(super) fn project_delivery(
        &self,
        context: &GitContext,
        projection: &DeliveryProjection,
        operation: &str,
    ) -> Result<String> {
        projection.validate()?;
        require(
            context.target.owner == context.project
                && context.target.branch == context.workspace.mainline
                && context.source.branch.starts_with("devdesk/features/"),
            "仅 Feature 到项目主干可生成交付投影",
        )?;
        let repo = self.context(context)?;
        self.operation(
            &context.project,
            operation,
            &json!({"kind":"delivery_projection","context":context,"projection":projection}),
        )?;
        let reference = format!("refs/devdesk/delivery-projections/{operation}");
        if let Some(revision) = self.existing_ref(&repo, &reference)? {
            require(
                self.commit_parents(context, &revision)?
                    == vec![projection.target_revision.clone()],
                "原交付投影父提交不符",
            )?;
            return Ok(revision);
        }
        let mut tree = self.commit_tree(context, &projection.source_revision)?;
        for path in &projection.validation_paths {
            tree = self.restore_validation_path(
                &repo,
                &tree,
                None,
                &path.split('/').collect::<Vec<_>>(),
            )?;
        }
        // Strip workflow files before merging, so parallel validation outputs
        // cannot cause irrelevant merge conflicts. Real code conflicts remain.
        let source = self.output(
            &repo,
            &[
                "commit-tree",
                &tree,
                "-p",
                &projection.source_revision,
                "-m",
                "DevDesk filtered delivery source",
            ],
        )?;
        let merge = self.git(
            &repo,
            &[
                "merge-tree",
                "--write-tree",
                &projection.target_revision,
                &source,
            ],
            None,
        )?;
        require(
            merge.code == 0 && !merge.truncated,
            "正式代码合并存在冲突，保留源分支供修复",
        )?;
        let mut merged_tree = std::str::from_utf8(&merge.stdout)
            .map_err(storage)?
            .lines()
            .next()
            .unwrap_or("")
            .to_owned();
        require(valid_oid(&merged_tree), "交付合并 tree 无效")?;
        for path in &projection.validation_paths {
            merged_tree = self.restore_validation_path(
                &repo,
                &merged_tree,
                None,
                &path.split('/').collect::<Vec<_>>(),
            )?;
        }
        let message = format!(
            "DevDesk code delivery {operation}\n\nSource: {}\nTarget: {}\nValidation paths: {}",
            projection.source_revision,
            projection.target_revision,
            serde_json::to_string(&projection.validation_paths).map_err(storage)?
        );
        let revision = self.output(
            &repo,
            &[
                "commit-tree",
                &merged_tree,
                "-p",
                &projection.target_revision,
                "-m",
                &message,
            ],
        )?;
        self.output(
            &repo,
            &[
                "update-ref",
                &reference,
                &revision,
                &"0".repeat(revision.len()),
            ],
        )?;
        Ok(revision)
    }
}
