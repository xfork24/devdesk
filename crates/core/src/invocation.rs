//! Trusted invocation metadata is created at an application boundary, never decoded from RPC.
use crate::*;
#[derive(Debug, Clone, Serialize)]
pub struct InvocationContext {
    actor: WorkflowActor,
    project_id: Option<Id>,
    #[serde(skip_serializing_if = "crate::concurrency::ReadSet::is_empty")]
    original_authority: crate::concurrency::ReadSet,
}
impl InvocationContext {
    pub(crate) fn configuration_authority(
        &self,
        store: &dyn EngineeringStore,
        method: &str,
    ) -> Result<crate::concurrency::ReadSet> {
        use crate::concurrency::*;
        self.require_scope(store.project_id().as_deref())?;
        match &self.actor {
            WorkflowActor::Human => Ok(self.original_authority.clone()),
            WorkflowActor::Controller {
                controller_id,
                session_id,
                turn_id,
                epoch,
            } => {
                let keys = [
                    ResourceKey::new("governance", controller_id),
                    ResourceKey::new("tool_turn", turn_id),
                    ResourceKey::new("session_live_workers", session_id),
                    ResourceKey::new(
                        "controller_lease_active",
                        json!([controller_id, epoch]).to_string(),
                    ),
                    ResourceKey::new("session", session_id),
                ];
                let rows = store.read_resources(&keys)?;
                require(
                    rows.len() == 5 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
                    "配置调用原授权读取不完整",
                )?;
                let record = ConfigurationRecord::from_storage(
                    rows[0]
                        .value
                        .clone()
                        .ok_or_else(|| err("not_found", "配置调用控制器缺失"))?,
                )?;
                require(
                    record.r#type == "workflow_controller" && record.id == *controller_id,
                    "配置调用的控制器身份不符",
                )?;
                let controller: WorkflowController = record.decode()?;
                require(
                    controller.session_id.as_ref() == Some(session_id)
                        && controller.epoch == *epoch
                        && controller.status == "active"
                        && Some(controller.project_id.as_str()) == store.project_id().as_deref(),
                    "配置调用控制器授权已失效",
                )?;
                controller
                    .policy
                    .permits(crate::manager_workspace::manager_operation(method))?;
                require(
                    !controller
                        .policy
                        .human_operations
                        .contains(crate::manager_workspace::manager_operation(method)),
                    "配置策略要求实际 Human 决定",
                )?;
                if method.starts_with("configuration.transfer.") {
                    controller.policy.permits("configuration.global.write")?;
                }
                require(rows[3].revision == 1, "配置调用经理租约已失效")?;
                let catalog = store.configuration_snapshot()?;
                crate::configuration::project_manager_role(&catalog.state, &controller.role_ref)?;
                let raw_session = rows[4]
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "配置调用原会话缺失"))?;
                let session: Session =
                    serde_json::from_value(store.restore_runtime_value(raw_session)?)
                        .map_err(|e| err("storage", e.to_string()))?;
                if let Some(instance) = session
                    .runtime_binding
                    .as_ref()
                    .and_then(|b| b.agent_configuration.as_ref())
                    .and_then(|a| catalog.record(&a.agent_instance_id))
                {
                    require(
                        !configuration_retired(&catalog.state, instance)
                            && catalog.is_head(instance),
                        "经理实例已换绑或移除，需要后继会话",
                    )?;
                }
                require(
                    rows[1].value.as_ref().is_some_and(|t| {
                        t["session_id"] == *session_id
                            && t["status"] == "running"
                            && t["cancel_requested"] == false
                    }),
                    "配置调用轮次已停止",
                )?;
                let workers = rows[2]
                    .value
                    .as_ref()
                    .and_then(Value::as_array)
                    .ok_or_else(|| err("storage", "配置调用缺少原生 worker"))?;
                let native = workers
                    .iter()
                    .filter(|w| w["kind"] == "harness")
                    .collect::<Vec<_>>();
                require(native.len() == 1, "配置调用缺少唯一原生 worker")?;
                let key = ResourceKey::new("tool_worker", text(native[0], "operation_id")?);
                let lease = store
                    .read_resources(std::slice::from_ref(&key))?
                    .into_iter()
                    .next()
                    .ok_or_else(|| err("storage", "原生 worker 租约缺失"))?;
                require(lease.revision == 1, "配置调用原生 worker 已被撤销")?;
                ReadSet::new(
                    rows.iter()
                        .map(|r| ResourceVersion {
                            resource: r.resource.clone(),
                            revision: r.revision,
                        })
                        .chain([
                            ResourceVersion {
                                resource: key,
                                revision: lease.revision,
                            },
                            ResourceVersion {
                                resource: ResourceKey::new("catalog_clock", "configuration"),
                                revision: catalog.revision,
                            },
                        ])
                        .collect(),
                )?
                .merge(self.original_authority.versions().iter().cloned())
            }
            _ => Err(err("permission_denied", "此执行来源没有配置发布权限")),
        }
    }
    pub fn human(project_id: Option<Id>) -> Self {
        Self {
            actor: WorkflowActor::Human,
            project_id: project_id.filter(|id| id != GLOBAL_DISCUSSION_NAMESPACE),
            original_authority: Default::default(),
        }
    }
    pub(crate) fn workflow(actor: WorkflowActor, project_id: Option<Id>) -> Self {
        Self {
            actor,
            project_id,
            original_authority: Default::default(),
        }
    }
    pub(crate) fn with_authority(mut self, reads: crate::concurrency::ReadSet) -> Result<Self> {
        self.original_authority = self
            .original_authority
            .merge(reads.versions().iter().cloned())?;
        Ok(self)
    }
    pub(crate) fn original_authority(&self) -> &crate::concurrency::ReadSet {
        &self.original_authority
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
    pub fn prepare_record(
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
}
