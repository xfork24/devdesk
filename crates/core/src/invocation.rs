//! Trusted invocation metadata is created at an application boundary, never decoded from RPC.
use crate::*;
#[derive(Debug, Clone, Serialize)]
pub struct InvocationContext {
    actor: WorkflowActor,
    project_id: Option<Id>,
}
impl InvocationContext {
    pub fn human(project_id: Option<Id>) -> Self {
        Self {
            actor: WorkflowActor::Human,
            project_id: project_id.filter(|id| id != GLOBAL_DISCUSSION_NAMESPACE),
        }
    }
    pub(crate) fn workflow(actor: WorkflowActor, project_id: Option<Id>) -> Self {
        Self { actor, project_id }
    }
    pub(crate) fn actor(&self) -> &WorkflowActor {
        &self.actor
    }
    pub(crate) fn require_scope(&self, project: Option<&str>) -> Result<()> {
        if self.project_id.as_deref() != project {
            return Err(err("invalid_scope", "调用上下文与存储作用域不一致"));
        }
        Ok(())
    }
    pub(crate) fn require_project(&self, project: &str) -> Result<()> {
        if self.project_id.as_deref() != Some(project) {
            return Err(err("invalid_scope", "调用上下文与 Project 不一致"));
        }
        Ok(())
    }
    pub(crate) fn prepare_record(
        &self,
        method: &str,
        command: &Command,
    ) -> crate::concurrency::ResourceChange {
        let mut row = crate::governance::record(
            "command_context",
            command.command_id.clone(),
            self.project_id
                .as_ref()
                .map_or(Scope::global(), Scope::project),
            method.into(),
            1,
            None,
            json!({"context":self,"target":command.target,"request_digest":command_fingerprint(method,command)}),
            true,
        );
        row.id = format!(
            "command-context:{}",
            configuration_hash(&command.command_id)
        );
        crate::concurrency::ResourceChange {
            resource: crate::concurrency::ResourceKey::new("governance", &row.id),
            value: row.to_storage(),
        }
    }
    pub(crate) fn record(&self, s: &mut EngineeringState, method: &str, c: &Command) -> Result<()> {
        if let Some(project) = &self.project_id {
            require(
                s.projects.is_empty() || s.projects.iter().any(|p| p.id == *project),
                "调用归属与项目不一致",
            )?;
        }
        let mut row = crate::governance::record(
            "command_context",
            c.command_id.clone(),
            self.project_id
                .as_ref()
                .map_or(Scope::global(), Scope::project),
            method.into(),
            1,
            None,
            json!({"context":self,"target":c.target,"request_digest":command_fingerprint(method,c)}),
            true,
        );
        row.id = format!("command-context:{}", c.command_id);
        if let Some(old) = s.governance.iter().find(|r| r.id == row.id) {
            require(old.data == row.data, "命令来源或内容冲突")?;
        } else {
            s.governance.push(row);
        }
        Ok(())
    }
}
