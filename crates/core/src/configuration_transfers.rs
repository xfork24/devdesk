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
use crate::concurrency::*;
use crate::configuration_publication::ConfigurationWorkFact;
use std::collections::BTreeMap;

fn version(row: &ResourceSnapshot) -> ResourceVersion {
    ResourceVersion {
        resource: row.resource.clone(),
        revision: row.revision,
    }
}
fn read_transfer(store: &dyn EngineeringStore, id: &str) -> Result<ResourceSnapshot> {
    let key = ResourceKey::new("governance", id);
    let mut rows = store.read_resources(std::slice::from_ref(&key))?;
    require(
        rows.len() == 1 && rows[0].resource == key,
        "跨库操作读取不完整",
    )?;
    let row = rows.remove(0);
    require(
        row.value
            .as_ref()
            .is_some_and(|r| r["type"] == "configuration_transfer"),
        "跨库操作不存在",
    )?;
    Ok(row)
}
fn row_of(store: &dyn EngineeringStore, row: &ResourceSnapshot) -> Result<ConfigurationRecord> {
    ConfigurationRecord::from_storage(
        store.restore_runtime_record(
            row.value
                .as_ref()
                .ok_or_else(|| err("not_found", "跨库操作不存在"))?,
        )?,
    )
}

impl Engine {
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
            matches!(self.store.namespace(), StoreNamespace::Configuration),
            "跨库发布必须从全局目录发起",
        )?;
        require(!c.command_id.is_empty(), "command_id 不能为空")?;
        let pid = project
            .store
            .project_id()
            .ok_or_else(|| err("project_required", "跨库发布需要项目"))?;
        let fp = configuration_hash(&("configuration-transfer-stage/v2", &c, actor, &pid));
        if let Some(receipt) = self.store.replay_commit(&c.command_id, &fp)? {
            return Ok(receipt.result);
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
            "共享阶段只发布源修订",
        )?;
        let global = self.store.configuration_snapshot()?;
        let local = project.store.configuration_snapshot()?;
        let authority = InvocationContext::workflow(actor.clone(), Some(pid.clone()))
            .configuration_authority(project.store.as_ref(), "configuration.transfer.stage")?;
        let work = project.configuration_work_facts(&project_change)?;
        let (global_candidate, _, _) = self.configuration_candidate_with(
            &global.state,
            &global_change,
            actor,
            &BTreeMap::new(),
        )?;
        let mut combined = local.state.as_ref().clone();
        merge_sources(&mut combined, &global_candidate)?;
        project.configuration_candidate_with(&combined, &project_change, actor, &work)?;
        let gate = configuration_transfer_witness(self.store.as_ref())?;
        let key = ResourceKey::new(
            "governance",
            format!("configuration-transfer:{}", c.command_id),
        );
        let mut row = record(
            "configuration_transfer",
            pid.clone(),
            Scope::global(),
            "跨库配置发布".into(),
            1,
            None,
            json!({"status":"prepared","actor":actor,"project_id":pid,"global_changeset":global_change,"project_changeset":project_change,
                "base_global":configuration_fingerprint(&global.state),"base_project":configuration_fingerprint(&local.state),"base_global_revision":global.revision,"base_project_revision":local.revision,
                "project_authority":authority,"work_facts":work,"origin":{"kind":"configuration_coordinator","actor":actor,"request_digest":fp}}),
            false,
        );
        row.id = key.id.clone();
        // The other database's observations are retained verbatim. Its adoption
        // checks them atomically in that database, without a cross-store mutex.
        self.store
            .commit_changes(&PreparedChangeSet {
                command_id: c.command_id.clone(),
                fingerprint: fp,
                read_set: ReadSet::new(vec![
                    gate,
                    ResourceVersion {
                        resource: ResourceKey::new("catalog_clock", "configuration"),
                        revision: global.revision,
                    },
                    ResourceVersion {
                        resource: key.clone(),
                        revision: 0,
                    },
                ])?,
                changes: vec![ResourceChange {
                    resource: key,
                    value: self.store.encode_runtime_record(&row)?,
                }],
                result: json!({"id":row.id,"status":"prepared","project_id":pid,"revision":1}),
                time: now() as i64,
            })
            .map(|r| r.result)
    }

    pub fn configuration_transfer_command(
        &self,
        project: &Engine,
        c: &Command,
        apply: bool,
    ) -> Result<Value> {
        self.configuration_transfer_command_as(project, c, apply, &WorkflowActor::Human)
    }
    pub(crate) fn configuration_transfer_command_as(
        &self,
        project: &Engine,
        c: &Command,
        apply: bool,
        actor: &WorkflowActor,
    ) -> Result<Value> {
        let method = if apply {
            "configuration.transfer.apply"
        } else {
            "configuration.transfer.cancel"
        };
        let fingerprint = configuration_hash(&(
            "configuration-transfer-control/v2",
            method,
            c,
            actor,
            project.store.project_id(),
        ));
        if let Some(receipt) = self.store.replay_commit(&c.command_id, &fingerprint)? {
            return Ok(receipt.result);
        }
        let id = c
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "需要跨库操作 ID"))?;
        let original = read_transfer(self.store.as_ref(), id)?;
        let row = row_of(self.store.as_ref(), &original)?;
        require(
            row.data["project_id"].as_str() == project.store.project_id().as_deref(),
            "跨库操作项目不匹配",
        )?;
        let continuing = apply
            && row.data["status"] == "applying"
            && row.data["apply_request"]["command"] == json!(c)
            && row.data["apply_request"]["fingerprint"] == fingerprint;
        if !continuing && c.expected_revision != Some(original.revision) {
            return Err(err("revision_conflict", "跨库操作原版本已改变，请重新载入"));
        }
        if apply {
            self.apply_transfer_original(
                project,
                original,
                Some((c.clone(), fingerprint, actor.clone())),
            )
        } else {
            self.cancel_transfer_original(original, c, &fingerprint, actor)
        }
    }

    /// Recovery continues the durable original grant, never a new Human command.
    pub fn apply_configuration_transfer(&self, project: &Engine, id: &str) -> Result<Value> {
        self.apply_transfer_original(project, read_transfer(self.store.as_ref(), id)?, None)
    }
    fn apply_transfer_original(
        &self,
        project: &Engine,
        mut original: ResourceSnapshot,
        request: Option<(Command, String, WorkflowActor)>,
    ) -> Result<Value> {
        let mut row = row_of(self.store.as_ref(), &original)?;
        let id = row.id.clone();
        require(
            row.data["project_id"].as_str() == project.store.project_id().as_deref(),
            "跨库操作项目不匹配",
        )?;
        if row.data["status"] == "completed" {
            return Ok(row.data["result"].clone());
        }
        if row.data["status"] == "failed" {
            return Err(serde_json::from_value(row.data["error"].clone())
                .map_err(|e| err("storage", e.to_string()))?);
        }
        require(pending(&row), "跨库操作已经取消")?;
        let actor: WorkflowActor = serde_json::from_value(row.data["actor"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let changes: ConfigurationChangeSet =
            serde_json::from_value(row.data["global_changeset"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        let local_changes: ConfigurationChangeSet =
            serde_json::from_value(row.data["project_changeset"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        if row.data["status"] == "prepared" {
            let (command, fingerprint, principal) = request.unwrap_or_else(|| {
                let command = Command {
                    command_id: format!("{id}:apply"),
                    target: Some(id.clone()),
                    expected_revision: Some(original.revision),
                    payload: json!({"transfer_id":id}),
                };
                let fingerprint = configuration_hash(&(
                    "configuration-transfer-recoverable-apply/v2",
                    &command,
                    &actor,
                ));
                (command, fingerprint, actor.clone())
            });
            let mut data = row.data.clone();
            data["status"] = json!("applying");
            data["apply_request"] =
                json!({"command":command,"fingerprint":fingerprint,"actor":principal});
            update_record(&mut row, data);
            let receipt = self.store.commit_changes(&PreparedChangeSet {
                command_id: format!("{id}:claim-apply"),
                fingerprint: configuration_hash(&(
                    "configuration-transfer-claim/v2",
                    &id,
                    &row.data["apply_request"],
                )),
                read_set: ReadSet::new(vec![version(&original)])?,
                changes: vec![ResourceChange {
                    resource: original.resource.clone(),
                    value: self.store.encode_runtime_record(&row)?,
                }],
                result: json!({"id":id}),
                time: now() as i64,
            })?;
            original.revision = receipt
                .versions
                .iter()
                .find(|v| v.resource == original.resource)
                .ok_or_else(|| err("storage", "发布资格缺少原版本"))?
                .revision;
            original.value = Some(row.to_storage());
        } else if let Some((command, fingerprint, _)) = request {
            require(
                row.data["apply_request"]["command"] == json!(command)
                    && row.data["apply_request"]["fingerprint"] == fingerprint,
                "已有另一条原始采用请求正在执行",
            )?;
        }
        let original_command: Command =
            serde_json::from_value(row.data["apply_request"]["command"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        let original_fingerprint = text(&row.data["apply_request"], "fingerprint")?;
        if let Some(receipt) = self
            .store
            .replay_commit(&original_command.command_id, &original_fingerprint)?
        {
            return Ok(receipt.result);
        }
        let global = self.store.configuration_snapshot()?;
        if json!(global.revision) != row.data["base_global_revision"]
            || json!(configuration_fingerprint(&global.state)) != row.data["base_global"]
        {
            if let Some(receipt) = self
                .store
                .replay_commit(&original_command.command_id, &original_fingerprint)?
            {
                return Ok(receipt.result);
            }
            return Err(err(
                "revision_conflict",
                "共享原配置已改变，不能重新采用原候选",
            ));
        }
        let (published, refs, _) =
            self.configuration_candidate_with(&global.state, &changes, &actor, &BTreeMap::new())?;
        let action = format!("{id}:project");
        let fp = configuration_hash(&(
            "configuration-transfer-project/v2",
            &row.data["global_changeset"],
            &row.data["project_changeset"],
            &row.data["base_project_revision"],
            &row.data["project_authority"],
            &row.data["work_facts"],
        ));
        let adoption = (|| -> Result<Value> {
            if let Some(receipt) = project.store.replay_commit(&action, &fp)? {
                return Ok(receipt.result);
            }
            let local = project.store.configuration_snapshot()?;
            if json!(local.revision) != row.data["base_project_revision"]
                || json!(configuration_fingerprint(&local.state)) != row.data["base_project"]
            {
                return Err(err(
                    "revision_conflict",
                    "项目原配置已改变，请重新暂存并保留草稿",
                ));
            }
            let mut combined = local.state.as_ref().clone();
            merge_sources(&mut combined, &published)?;
            let work: BTreeMap<Id, ConfigurationWorkFact> =
                serde_json::from_value(row.data["work_facts"].clone())
                    .map_err(|e| err("storage", e.to_string()))?;
            let (next, _, selections) =
                project.configuration_candidate_with(&combined, &local_changes, &actor, &work)?;
            let candidate =
                ConfigurationCatalogSnapshot::new(next, local.revision, local.repository_ready)?;
            let publication =
                prepare_configuration_publication(project.store.as_ref(), &local, &candidate)?;
            let authority: ReadSet = serde_json::from_value(row.data["project_authority"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            let mut read_set = publication
                .read_set
                .merge(authority.versions().iter().cloned())?;
            let mut writes = publication.changes;
            for fact in work.values() {
                read_set = read_set.merge(fact.read_set.versions().iter().cloned())?;
                if fact.owner.resource.kind != "project" {
                    let mut raw = fact
                        .owner
                        .value
                        .clone()
                        .ok_or_else(|| err("storage", "原工作事实缺失"))?;
                    raw["revision"] = json!(fact.owner.revision + 1);
                    writes.push(ResourceChange {
                        resource: fact.owner.resource.clone(),
                        value: raw,
                    });
                }
            }
            let receipt_key = ResourceKey::new("governance", format!("{id}:receipt"));
            let mut receipt = record(
                "configuration_transfer_receipt",
                id.clone(),
                Scope::project(project.store.project_id().unwrap()),
                "已采用跨库配置".into(),
                1,
                None,
                json!({"transfer_id":id,"actor":actor,"selections":selections,"published_refs":refs.iter().map(|r|&r.id).collect::<Vec<_>>()}),
                true,
            );
            receipt.id = receipt_key.id.clone();
            read_set = read_set.merge([ResourceVersion {
                resource: receipt_key.clone(),
                revision: 0,
            }])?;
            writes.push(ResourceChange {
                resource: receipt_key,
                value: receipt.to_storage(),
            });
            project
                .store
                .commit_changes(&PreparedChangeSet {
                    command_id: action.clone(),
                    fingerprint: fp.clone(),
                    read_set,
                    changes: writes,
                    result: receipt.data.clone(),
                    time: now() as i64,
                })
                .map(|r| r.result)
        })();
        let local_result = match adoption {
            Ok(result) => result,
            Err(error) => {
                // A competing original adopter may have committed between the
                // first replay lookup and our read or failed CAS.
                if let Some(receipt) = project.store.replay_commit(&action, &fp)? {
                    receipt.result
                } else {
                    if !matches!(error.code.as_str(), "storage_busy" | "storage") {
                        self.reject_configuration_transfer(&original, &row, &error)?;
                    }
                    return Err(error);
                }
            }
        };
        let candidate =
            ConfigurationCatalogSnapshot::new(published, global.revision, global.repository_ready)?;
        let publication =
            match prepare_configuration_publication(self.store.as_ref(), &global, &candidate) {
                Ok(publication) => publication,
                Err(error) => {
                    if let Some(receipt) = self
                        .store
                        .replay_commit(&original_command.command_id, &original_fingerprint)?
                    {
                        return Ok(receipt.result);
                    }
                    return Err(error);
                }
            };
        let mut data = row.data.clone();
        data["status"] = json!("completed");
        data["result"] = local_result.clone();
        update_record(&mut row, data);
        let mut writes = publication.changes;
        writes.push(ResourceChange {
            resource: original.resource.clone(),
            value: self.store.encode_runtime_record(&row)?,
        });
        let command: Command = serde_json::from_value(row.data["apply_request"]["command"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let fingerprint = text(&row.data["apply_request"], "fingerprint")?;
        self.store
            .commit_changes(&PreparedChangeSet {
                command_id: command.command_id,
                fingerprint,
                read_set: publication.read_set.merge([version(&original)])?,
                changes: writes,
                result: local_result,
                time: now() as i64,
            })
            .map(|r| r.result)
    }

    fn reject_configuration_transfer(
        &self,
        original: &ResourceSnapshot,
        row: &ConfigurationRecord,
        error: &Error,
    ) -> Result<()> {
        let mut rejected = row.clone();
        let mut data = rejected.data.clone();
        data["status"] = json!("failed");
        data["error"] = json!(error);
        update_record(&mut rejected, data);
        self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("{}:reject:{}", row.id, original.revision),
            fingerprint: configuration_hash(&(
                "configuration-transfer-reject/v2",
                version(original),
                error,
            )),
            read_set: ReadSet::new(vec![version(original)])?,
            changes: vec![ResourceChange {
                resource: original.resource.clone(),
                value: self.store.encode_runtime_record(&rejected)?,
            }],
            result: json!({"id":row.id,"status":"failed","error":error}),
            time: now() as i64,
        })?;
        Ok(())
    }
    pub fn cancel_configuration_transfer(&self, id: &str) -> Result<Value> {
        let original = read_transfer(self.store.as_ref(), id)?;
        let row = row_of(self.store.as_ref(), &original)?;
        let actor: WorkflowActor = serde_json::from_value(row.data["actor"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let command = Command {
            command_id: format!("{id}:cancel:{}", original.revision),
            target: Some(id.into()),
            expected_revision: Some(original.revision),
            payload: Value::Null,
        };
        self.cancel_transfer_original(
            original,
            &command,
            &configuration_hash(&("configuration-transfer-cancel/v2", &command, &actor)),
            &actor,
        )
    }
    fn cancel_transfer_original(
        &self,
        original: ResourceSnapshot,
        c: &Command,
        fingerprint: &str,
        actor: &WorkflowActor,
    ) -> Result<Value> {
        let mut row = row_of(self.store.as_ref(), &original)?;
        require(
            row.data["status"] == "prepared",
            "只可取消尚未采用的原暂存操作",
        )?;
        let mut data = row.data.clone();
        data["status"] = json!("cancelled");
        data["cancel_origin"] = json!(actor);
        update_record(&mut row, data);
        self.store
            .commit_changes(&PreparedChangeSet {
                command_id: c.command_id.clone(),
                fingerprint: fingerprint.into(),
                read_set: ReadSet::new(vec![version(&original)])?,
                changes: vec![ResourceChange {
                    resource: original.resource,
                    value: self.store.encode_runtime_record(&row)?,
                }],
                result: json!({"id":row.id,"status":"cancelled","revision":row.revision}),
                time: now() as i64,
            })
            .map(|r| r.result)
    }
    pub fn recover_configuration_transfers(&self, project: &Engine) -> Result<()> {
        for original in self.pending_runtime_records("configuration_transfer", "applying")? {
            if original.value.as_ref().is_some_and(|r| {
                r["data"]["project_id"].as_str() == project.store.project_id().as_deref()
            }) {
                self.apply_transfer_original(project, original, None)?;
            }
        }
        Ok(())
    }
}
