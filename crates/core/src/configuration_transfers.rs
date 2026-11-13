//! Recoverable publication across independent global/project stores. Staged sources stay hidden.
use crate::configuration_changes::ConfigurationChangeSet;
use crate::governance::{record, update_record};
use crate::*;
fn pending(row: &ConfigurationRecord) -> bool {
    row.r#type == "configuration_transfer"
        && ["prepared", "applying"].contains(&row.data["status"].as_str().unwrap_or(""))
}
fn merge_sources(target: &mut EngineeringState, global: &EngineeringState) -> Result<()> {
    for source in global
        .governance
        .iter()
        .filter(|r| r.is_definition() && r.scope.is_global())
    {
        if let Some(old) = target.governance.iter().find(|r| r.id == source.id) {
            require(old.source() == source.source(), "共享源内容冲突")?;
        } else {
            target.governance.push(source.clone());
        }
    }
    target.refresh_configuration_views()
}
pub(crate) fn configuration_transfer_guard(s: &EngineeringState, method: &str) -> Result<()> {
    if template_definition_write(method)
        || [
            "project.configuration.update",
            "work.configuration.update",
            "configuration.templates.reset",
            "permission.bindings.apply",
        ]
        .contains(&method)
    {
        require(
            !s.governance.iter().any(pending),
            "有待完成的跨库配置操作，请先完成或取消",
        )?;
    }
    Ok(())
}
impl Engine {
    /// Called on the global application service with an explicitly selected project service.
    pub fn stage_configuration_transfer(&self, project: &Engine, c: Command) -> Result<Value> {
        self.stage_configuration_transfer_as(project, c, &WorkflowActor::Human)
    }
    pub(crate) fn stage_configuration_transfer_as(
        &self,
        project: &Engine,
        c: Command,
        actor: &WorkflowActor,
    ) -> Result<Value> {
        require(
            self.store.project_id().is_none(),
            "跨库发布必须从全局目录发起",
        )?;
        let pid = project
            .store
            .project_id()
            .ok_or_else(|| err("project_required", "需要项目配置库"))?;
        let fp = configuration_hash(&(
            command_fingerprint("configuration.transfer.stage", &c),
            actor,
        ));
        if let Some(result) = self.store.replay(&c.command_id, &fp)? {
            return Ok(result);
        }
        let global_change: ConfigurationChangeSet =
            serde_json::from_value(c.payload["global_changeset"].clone())
                .map_err(|e| err("invalid_params", e.to_string()))?;
        let project_change: ConfigurationChangeSet =
            serde_json::from_value(c.payload["project_changeset"].clone())
                .map_err(|e| err("invalid_params", e.to_string()))?;
        require(
            global_change.operations.iter().all(|op| {
                matches!(
                    op,
                    ConfigurationOperation::Create { .. } | ConfigurationOperation::Revise { .. }
                )
            }),
            "跨库发布的共享阶段只发布源修订，停用/删除请单独预演",
        )?;
        let global = self.store.snapshot()?;
        let local = project.store.snapshot()?;
        let (global_candidate, _, _) =
            self.configuration_candidate(&global, &global_change, actor)?;
        let mut combined = local.clone();
        merge_sources(&mut combined, &global_candidate)?;
        project.configuration_candidate(&combined, &project_change, actor)?;
        let base_global = configuration_fingerprint(&global);
        let base_project = configuration_fingerprint(&local);
        let id = format!("configuration-transfer:{}", c.command_id);
        self.store.transact(Some((&c.command_id,&fp)),&mut |s|{
            require(!s.governance.iter().any(pending),"已有待完成的跨库配置操作")?;
            require(configuration_fingerprint(s)==base_global,"共享配置已变化，请重新预演")?;
            let mut row=record("configuration_transfer",pid.clone(),Scope::global(),"跨库配置发布".into(),1,None,json!({"status":"prepared","actor":actor,"project_id":pid,"global_changeset":global_change,"project_changeset":project_change,"base_global":base_global,"base_project":base_project}),false);
            row.id=id.clone();s.governance.push(row);
            Ok(json!({"id":id,"status":"prepared","project_id":pid}))
        })
    }
    pub fn apply_configuration_transfer(&self, project: &Engine, id: &str) -> Result<Value> {
        let row = self.store.snapshot()?.configuration(id)?.clone();
        require(
            row.r#type == "configuration_transfer"
                && row.data["project_id"].as_str() == project.store.project_id().as_deref(),
            "跨库操作归属不匹配",
        )?;
        if row.data["status"] == "completed" {
            return Ok(row.data["result"].clone());
        }
        require(pending(&row), "跨库操作已取消或失败")?;
        let actor: WorkflowActor = serde_json::from_value(
            row.data
                .get("actor")
                .cloned()
                .unwrap_or(json!({"kind":"human"})),
        )
        .map_err(|e| err("storage", e.to_string()))?;
        let changes: ConfigurationChangeSet =
            serde_json::from_value(row.data["global_changeset"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        let local_changes: ConfigurationChangeSet =
            serde_json::from_value(row.data["project_changeset"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        self.store.transact(None, &mut |s| {
            require(
                json!(configuration_fingerprint(s)) == row.data["base_global"],
                "共享配置已变化，不能采用暂存候选",
            )?;
            let current = s.configuration(id)?.clone();
            require(pending(&current), "跨库操作状态已变化")?;
            let mut data = current.data.clone();
            data["status"] = json!("applying");
            update_record(s.governance.iter_mut().find(|r| r.id == id).unwrap(), data);
            Ok(Value::Null)
        })?;
        let global = self.store.snapshot()?;
        let (published, refs, _) = self.configuration_candidate(&global, &changes, &actor)?;
        let action = format!("{id}:project");
        let fp = configuration_hash(
            &json!({"global":row.data["global_changeset"],"project":row.data["project_changeset"],"base_global":row.data["base_global"],"base_project":row.data["base_project"]}),
        );
        let local_result=project.store.transact(Some((&action,&fp)),&mut |s|{
            require(json!(configuration_fingerprint(s))==row.data["base_project"],"项目配置已变化，请取消并重新预演")?;
            let mut combined=s.clone();merge_sources(&mut combined,&published)?;
            let (next,_,selections)=project.configuration_candidate(&combined,&local_changes,&actor)?;
            *s=next;
            let mut receipt=record("configuration_transfer_receipt",id.into(),Scope::project(project.store.project_id().unwrap()),"已采用跨库配置".into(),1,None,json!({"transfer_id":id,"selections":selections,"published_refs":refs.iter().map(|r|&r.id).collect::<Vec<_>>()}),true);
            receipt.id=format!("{id}:receipt");let result=receipt.data.clone();s.governance.push(receipt);Ok(result)
        });
        let local_result = match local_result {
            Ok(result) => result,
            Err(error) => {
                // A failed transaction is not assumed absent until its durable receipt can be read.
                let state = project.store.snapshot()?;
                if let Some(receipt) = state
                    .governance
                    .iter()
                    .find(|r| r.id == format!("{id}:receipt"))
                {
                    receipt.data.clone()
                } else {
                    self.store.transact(None, &mut |s| {
                        let current = s.configuration(id)?.clone();
                        let mut data = current.data;
                        data["status"] = json!("failed");
                        data["error"] = json!(error.message);
                        update_record(s.governance.iter_mut().find(|r| r.id == id).unwrap(), data);
                        Ok(Value::Null)
                    })?;
                    return Err(error);
                }
            }
        };
        self.store.transact(None, &mut |s| {
            let current = s.configuration(id)?.clone();
            if current.data["status"] == "completed" {
                return Ok(current.data["result"].clone());
            }
            require(current.data["status"] == "applying", "跨库操作状态已变化")?;
            let (next, _, _) = self.configuration_candidate(s, &changes, &actor)?;
            *s = next;
            let mut data = current.data;
            data["status"] = json!("completed");
            data["result"] = local_result.clone();
            update_record(s.governance.iter_mut().find(|r| r.id == id).unwrap(), data);
            Ok(local_result.clone())
        })
    }
    pub fn cancel_configuration_transfer(&self, id: &str) -> Result<Value> {
        self.store.transact(None, &mut |s| {
            let row = s.configuration(id)?.clone();
            require(
                row.r#type == "configuration_transfer" && row.data["status"] == "prepared",
                "只可取消尚未采用的暂存操作",
            )?;
            let mut data = row.data;
            data["status"] = json!("cancelled");
            update_record(s.governance.iter_mut().find(|r| r.id == id).unwrap(), data);
            Ok(json!({"id":id,"status":"cancelled"}))
        })
    }
    pub fn recover_configuration_transfers(&self, project: &Engine) -> Result<()> {
        let rows: Vec<_> = self
            .store
            .snapshot()?
            .governance
            .into_iter()
            .filter(|r| {
                r.r#type == "configuration_transfer"
                    && r.data["status"] == "applying"
                    && r.data["project_id"].as_str() == project.store.project_id().as_deref()
            })
            .collect();
        for row in rows {
            self.apply_configuration_transfer(project, &row.id)?;
        }
        Ok(())
    }
}
