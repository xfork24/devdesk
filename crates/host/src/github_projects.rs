//! The Web project entry point always records a GitHub connection intent before opening.
use super::*;

fn repository_name(value: &str) -> Result<String> {
    let name = value
        .trim()
        .strip_prefix("https://github.com/")
        .or_else(|| value.trim().strip_prefix("git@github.com:"))
        .unwrap_or(value.trim())
        .trim_end_matches('/');
    let name = name.strip_suffix(".git").unwrap_or(name);
    let parts: Vec<_> = name.split('/').collect();
    require(
        parts.len() == 2
            && parts.iter().all(|p| {
                !p.is_empty()
                    && *p != "."
                    && *p != ".."
                    && p.bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
            }),
        "请填写 GitHub owner/repo 或 GitHub 仓库 URL",
    )?;
    Ok(name.to_owned())
}
impl Workspace {
    pub(super) fn open_github_project(&self, params: Value) -> Result<Value> {
        let c: Command =
            serde_json::from_value(params).map_err(|e| err("invalid_params", e.to_string()))?;
        require(!c.command_id.is_empty(), "command_id 不能为空")?;
        let _switch = self.switching.lock().unwrap();
        let fingerprint = command_fingerprint("project.open_github", &c);
        if let Some(previous) = self.global.replay(&c.command_id, &fingerprint)? {
            let pid = previous["id"]
                .as_str()
                .ok_or_else(|| err("storage", "项目打开回执无效"))?;
            let e = self.load_project(pid, false)?;
            self.select(pid, e)?;
            return Ok(previous);
        }
        let existing = c
            .target
            .as_deref()
            .map(|id| {
                self.global
                    .project(id)
                    .and_then(|p| p.ok_or_else(|| err("not_found", "项目不存在")))
            })
            .transpose()?;
        let repository = repository_name(
            c.payload["repository"]
                .as_str()
                .or_else(|| {
                    existing
                        .as_ref()
                        .and_then(|p| p.workspace_definition.as_ref())
                        .map(|w| w.repository.as_str())
                })
                .ok_or_else(|| err("invalid_params", "打开项目需要 GitHub 仓库"))?,
        )?;
        let mainline = c.payload["mainline"]
            .as_str()
            .or_else(|| {
                existing
                    .as_ref()
                    .and_then(|p| p.workspace_definition.as_ref())
                    .map(|w| w.mainline.as_str())
            })
            .unwrap_or("main")
            .trim()
            .to_string();
        require(
            !mainline.is_empty()
                && mainline.len() <= 200
                && !mainline.starts_with('-')
                && !mainline.chars().any(char::is_whitespace),
            "主分支名称无效",
        )?;
        // Opening the same repository/branch reuses its project, including an interrupted clone.
        let mut existing = existing;
        if existing.is_none() {
            let mut offset = 0;
            loop {
                let (rows, total) = self.global.projects(offset, 100)?;
                existing = rows.into_iter().find(|p| {
                    p.workspace_definition.as_ref().is_some_and(|w| {
                        w.repository.eq_ignore_ascii_case(&repository) && w.mainline == mainline
                    })
                });
                offset += 100;
                if existing.is_some() || offset >= total {
                    break;
                }
            }
        }
        let (pid, e) = if let Some(project) = existing {
            require(
                project.workspace_definition.as_ref().is_some_and(|w| {
                    w.repository.eq_ignore_ascii_case(&repository) && w.mainline == mainline
                }),
                "不能更换已有项目的仓库或主分支",
            )?;
            let e = self.load_project(&project.id, false)?;
            (project.id, e)
        } else {
            let create = Command {
                command_id: format!("{}:create", c.command_id),
                target: None,
                expected_revision: None,
                payload: json!({"title":c.payload["title"].as_str().filter(|v| !v.trim().is_empty()).unwrap_or(&repository),"repository":repository,"mainline":mainline}),
            };
            let pid = self.global.reserve_project(&create)?;
            let e = self.load_project(&pid, true)?;
            self.sync_configuration(&e)?;
            e.execute("project.create", create)?;
            (pid, e)
        };
        self.sync_configuration(&e)?;
        let state = e.store.snapshot()?;
        let project = state
            .projects
            .first()
            .ok_or_else(|| err("storage", "项目不存在"))?;
        if project.execution_mode != "live"
            && !state
                .repository_operations
                .iter()
                .any(|op| matches!(op.action, RepositoryAction::Connect { .. }))
        {
            e.execute(
                "repository.connect",
                Command {
                    command_id: format!("github-connect:{pid}"),
                    target: Some(pid.clone()),
                    expected_revision: Some(project.revision),
                    payload: json!({"repository":repository,"mainline":mainline}),
                },
            )?;
        }
        self.global
            .publish_project(e.store.snapshot()?.projects.first().unwrap())?;
        self.select(&pid, e)?;
        self.global
            .transact(Some((&c.command_id, &fingerprint)), &mut |_| {
                Ok(json!({"id":pid,"repository":repository,"mainline":mainline}))
            })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_github_repository_identifiers() {
        for value in [
            "owner/repo",
            "https://github.com/owner/repo.git",
            "git@github.com:owner/repo.git",
        ] {
            assert_eq!(repository_name(value).unwrap(), "owner/repo");
        }
        for value in [
            "",
            "../repo",
            "https://other.example/owner/repo",
            "owner/repo/extra",
            "owner/repo?x=1",
        ] {
            assert!(repository_name(value).is_err(), "{value}");
        }
    }
    #[test]
    fn opening_github_project_automatically_connects_and_reuses_identity() {
        let temp = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(temp.path(), simulation::adapters()).unwrap();
        let first = w
            .dispatch(
                "project.open_github",
                json!({"command_id":"first","payload":{"repository":"owner/repo"}}),
            )
            .unwrap();
        let second = w.dispatch("project.open_github", json!({"command_id":"second","payload":{"repository":"https://github.com/owner/repo.git"}})).unwrap();
        assert_eq!(first["id"], second["id"]);
        assert_eq!(
            first,
            w.dispatch(
                "project.open_github",
                json!({"command_id":"first","payload":{"repository":"owner/repo"}})
            )
            .unwrap()
        );
        assert!(
            w.dispatch(
                "project.open_github",
                json!({"command_id":"first","payload":{"repository":"owner/different"}})
            )
            .is_err()
        );
        assert_eq!(w.global.github_projects(0, 50).unwrap().1, 1);
        let state = w.active_engine(None).unwrap().store.snapshot().unwrap();
        assert_eq!(state.repository_operations.len(), 1);
        assert!(matches!(
            state.repository_operations[0].action,
            RepositoryAction::Connect { .. }
        ));
        assert!(state.features.is_empty());
        drop(w);
        let w = Workspace::open_with_adapters(temp.path(), simulation::adapters()).unwrap();
        let resumed = w
            .dispatch(
                "project.open_github",
                json!({"command_id":"reopen","target":first["id"],"payload":{}}),
            )
            .unwrap();
        assert_eq!(first["id"], resumed["id"]);
        assert_eq!(
            w.active_engine(None)
                .unwrap()
                .store
                .snapshot()
                .unwrap()
                .repository_operations
                .len(),
            1
        );
    }
    #[test]
    fn repository_reconciliation_wakes_idle_project_and_survives_restart() {
        let temp = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(temp.path(), simulation::adapters()).unwrap();
        w.dispatch(
            "project.open_github",
            json!({"command_id":"open","payload":{"repository":"owner/repo"}}),
        )
        .unwrap();
        // This fixture has no repository adapter: first dispatch fails without external I/O.
        w.tick().unwrap();
        let e = w.active_engine(None).unwrap();
        let state = e.store.snapshot().unwrap();
        let op = state.repository_operations.first().unwrap();
        assert_eq!(op.status, RepositoryOperationStatus::NeedsReconciliation);
        assert!(
            !e.store.has_pending_work().unwrap(),
            "an unrequested failure stays idle"
        );
        w.dispatch("repository.reconcile", json!({"project_id":op.project_id,"command_id":"retry","target":op.id,"expected_revision":op.revision,"payload":{}})).unwrap();
        assert!(
            e.store.has_pending_work().unwrap(),
            "a requested repository reconciliation must wake an otherwise idle project"
        );
        let revision = e.store.snapshot().unwrap().repository_operations[0].revision;
        drop(e);
        drop(w);
        let w = Workspace::open_with_adapters(temp.path(), simulation::adapters()).unwrap();
        assert!(
            w.active_engine(None)
                .unwrap()
                .store
                .has_pending_work()
                .unwrap()
        );
        w.tick().unwrap();
        let e = w.active_engine(None).unwrap();
        let state = e.store.snapshot().unwrap();
        let retried = &state.repository_operations[0];
        assert!(
            retried.revision > revision,
            "the scheduler must actually process the original operation"
        );
        assert_eq!(
            retried.status,
            RepositoryOperationStatus::NeedsReconciliation
        );
        assert_ne!(retried.reason, "reconcile_requested");
        assert!(
            !e.store.has_pending_work().unwrap(),
            "a new failure must not cause an automatic retry loop"
        );
    }
}
