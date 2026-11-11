use crate::*;
impl Engine {
    pub(crate) fn discussion_code_view(
        &self,
        s: &EngineeringState,
        session: &Session,
    ) -> Result<Option<FrozenCode>> {
        // Controllers operate through scoped tools, not the owner's code worktree.
        // Inheriting a discussion view would also register them as workspace writers.
        if session
            .runtime_binding
            .as_ref()
            .is_some_and(|binding| binding.work_type == WorkType::Orchestration)
            || crate::controllers::binding(s, &session.id).is_some()
        {
            return Ok(None);
        }
        if let Some(view) = self.configured_session_code_view(s, session)? {
            return Ok(view);
        }
        let Some(project) = s
            .projects
            .iter()
            .find(|p| Some(&p.id) == session.project_id.as_ref() && p.execution_mode == "live")
        else {
            return Ok(None);
        };
        let Some(work) = s
            .repository_worktrees
            .iter()
            .find(|w| w.owner_id == session.owner_id && w.project_id == project.id)
        else {
            return Ok(None);
        };
        let head = if session.scope == Some(SessionScope::Project) {
            format!("main:{}", project.id)
        } else {
            session.owner_id.clone()
        };
        let revision = s
            .heads
            .iter()
            .find(|h| h.id == head)
            .ok_or_else(|| err("blocked", "代码对象尚未核对"))?
            .revision
            .clone();
        let source = CodeLocation {
            owner: session.owner_id.clone(),
            branch: work.branch.clone(),
            worktree: work.path.clone(),
        };
        let context = GitContext {
            project: project.id.clone(),
            workspace: project
                .workspace_definition
                .clone()
                .ok_or_else(|| err("blocked", "仓库定义缺失"))?,
            source: source.clone(),
            target: source,
            origin: Origin::External,
        };
        Ok(Some(FrozenCode { context, revision }))
    }
    pub fn stop_owned_runtime(&self) -> Result<()> {
        let state = self.store.snapshot()?;
        if !state
            .turns
            .iter()
            .any(|t| t.status.active() && t.status != TurnStatus::Queued)
            && !state.executions.iter().any(|e| !e.writer_revoked)
        {
            return Ok(());
        }
        self.store.transact(None, &mut |s| {
            for t in s
                .turns
                .iter_mut()
                .filter(|t| t.status.active() && t.status != TurnStatus::Queued)
            {
                t.status = TurnStatus::NeedsReconciliation;
                t.revision += 1;
            }
            Ok(Value::Null)
        })?;
        self.tick_conversations(0)
    }
}
