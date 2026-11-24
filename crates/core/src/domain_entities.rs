//! Domain-owned object data is scoped to a single project and package version.
use crate::{
    concurrency::*, governance::record, platform_facts::one, workflow_commits::WorkflowCommit,
    workflow_resources::version, *,
};
use std::collections::BTreeMap;

pub(crate) const KIND: &str = "domain_entity";

/// A trusted package freezes this binding when admitting an entity-owned root.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DomainWorkflowBinding {
    pub digest: String,
    pub owner_type: Id,
    pub input: String,
    pub outputs: BTreeMap<String, DomainOutputBinding>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DomainOutputBinding {
    pub type_ref: Id,
    pub schema: Value,
}

impl Engine {
    pub fn read_domain_entity(&self, digest: &str, id: &str) -> Result<Value> {
        let project = self.domain_entity_project()?;
        let row = one(
            self.store.as_ref(),
            "governance",
            &domain_entity_key(&project, id),
        )?;
        let entity = extract(&row, &project, digest)?;
        entity.ok_or_else(|| err("not_found", "领域对象不存在"))
    }

    pub fn read_domain_submission(&self, root: &str) -> Result<Option<Value>> {
        let row = one(
            self.store.as_ref(),
            "governance",
            &format!("domain-submission:{root}"),
        )?;
        Ok(row.value.map(|value| value["data"].clone()))
    }

    pub fn write_domain_entity(
        &self,
        digest: &str,
        command: &Command,
        context: &InvocationContext,
    ) -> Result<Value> {
        let project = self.domain_entity_project()?;
        context.require_project(&project)?;
        require(
            matches!(context.actor(), WorkflowActor::Human),
            "领域对象编辑需要人类会话",
        )?;
        let id = command
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "领域对象缺少 ID"))?;
        let key = domain_entity_key(&project, id);
        let receipt_id = format!(
            "domain-entity:{}",
            configuration_hash(&(digest, &project, &command.command_id))
        );
        let fingerprint = configuration_hash(&(digest, &project, command, context));
        if let Some(receipt) = self.store.replay_commit(&receipt_id, &fingerprint)? {
            return Ok(receipt.result);
        }
        let live = one(self.store.as_ref(), "project_live", &project)?;
        require(live.revision == 1, "历史项目只读")?;
        let active = one(self.store.as_ref(), "workflow_active_owner", id)?;
        require(
            active
                .value
                .as_ref()
                .is_some_and(|row| row["active"] == false),
            "领域对象有在途根流程",
        )?;
        let row = one(self.store.as_ref(), "governance", &key)?;
        if command.expected_revision != Some(row.revision) {
            return Err(err("revision_conflict", "领域对象修订已变化"));
        }
        let type_ref = text(&command.payload, "type_ref")?;
        let previous = extract(&row, &project, digest)?;
        require(
            previous
                .as_ref()
                .is_none_or(|entity| entity["type_ref"] == type_ref),
            "领域对象类型不可变更",
        )?;
        require(
            previous
                .as_ref()
                .is_none_or(|entity| entity.get("origin").is_none()),
            "工作流提交的领域产物不可覆盖",
        )?;
        let revision = row.revision + 1;
        let entity = json!({
            "id":id,"project_id":project,"type_ref":type_ref,
            "revision":revision,"data":command.payload["data"],
        });
        let stored = storage_record(&project, &key, digest, &entity, revision)?;
        self.store
            .commit_changes(&PreparedChangeSet {
                command_id: receipt_id,
                fingerprint,
                read_set: ReadSet::new(vec![version(&row), version(&live), version(&active)])?,
                changes: vec![ResourceChange {
                    resource: row.resource,
                    value: stored.to_storage(),
                }],
                result: entity,
                time: now() as i64,
            })
            .map(|receipt| receipt.result)
    }

    fn domain_entity_project(&self) -> Result<Id> {
        self.store
            .project_id()
            .ok_or_else(|| err("invalid_scope", "领域对象需要项目库"))
    }
}

fn storage_record(
    project: &str,
    key: &str,
    digest: &str,
    entity: &Value,
    revision: u64,
) -> Result<ConfigurationRecord> {
    // Domain payloads remain opaque to the configuration source-reference codec.
    let mut stored = record(
        KIND,
        key.into(),
        Scope::project(project),
        text(entity, "type_ref")?,
        revision,
        None,
        json!({"digest":digest,"payload":entity.to_string()}),
        false,
    );
    stored.id = key.into();
    stored.revision = revision;
    Ok(stored)
}

pub(crate) fn workflow_binding_key(root: &str) -> String {
    format!("domain-workflow:{root}")
}

impl WorkflowCommit<'_> {
    pub(crate) fn record_domain_submission(
        &mut self,
        root: &ConfiguredWorkflowInstance,
        submitted: &BTreeMap<String, Id>,
    ) -> Result<()> {
        if submitted.is_empty() {
            return Ok(());
        }
        let mut row = record(
            "domain_workflow_submission",
            root.root_instance_id.clone(),
            Scope::project(&root.project_id),
            "Domain workflow submission".into(),
            1,
            None,
            json!({"owner_id":root.owner_id,"entities":submitted}),
            true,
        );
        row.id = format!("domain-submission:{}", root.root_instance_id);
        self.insert_record(&row)
    }
    pub(crate) fn commit_domain_outputs(
        &mut self,
        root: &ConfiguredWorkflowInstance,
        facts: &WorkflowFacts,
        decision: &str,
    ) -> Result<BTreeMap<String, Id>> {
        if root.parent_execution_id.is_some() {
            return Ok(BTreeMap::new());
        }
        let store = self.engine.store.as_ref();
        let frozen = one(
            store,
            "governance",
            &workflow_binding_key(&root.root_instance_id),
        )?;
        let Some(row) = &frozen.value else {
            return Ok(BTreeMap::new());
        };
        let binding: DomainWorkflowBinding = serde_json::from_str(&text(&row["data"], "contract")?)
            .map_err(|error| err("storage", error.to_string()))?;
        require(
            row["type"] == "domain_workflow_binding"
                && row["definition_id"] == root.root_instance_id,
            "原领域交付约束缺失",
        )?;
        let owner = one(
            store,
            "governance",
            &domain_entity_key(&root.project_id, &root.owner_id),
        )?;
        let entity = extract(&owner, &root.project_id, &binding.digest)?
            .ok_or_else(|| err("not_found", "原领域对象缺失"))?;
        require(
            entity["type_ref"] == binding.owner_type
                && entity["revision"] == row["data"]["owner_revision"],
            "领域工作流必须提交到原对象修订",
        )?;
        self.witness([version(&frozen), version(&owner)])?;
        let mut submitted = BTreeMap::new();
        for (port, target) in &binding.outputs {
            let source = root
                .plan
                .document
                .outputs
                .get(port)
                .ok_or_else(|| err("blocked", "工作流缺少冻结的领域交付端口"))?;
            let value = facts
                .value(source)
                .ok_or_else(|| err("blocked", "领域交付缺少原产物"))?;
            require(value.to_string().len() <= 16 * 1024, "领域对象数据超出预算")?;
            self.engine
                .tool_executor
                .as_ref()
                .ok_or_else(|| err("configuration_required", "缺少领域交付 Schema 校验器"))?
                .validate_arguments(&target.schema, value)?;
            let execution = root
                .steps
                .iter()
                .find(|step| {
                    step.step == source.step && step.status == StepExecutionStatus::Completed
                })
                .ok_or_else(|| err("blocked", "领域交付需要已接纳的实际步骤"))?;
            let output_id = execution
                .outputs
                .get(&source.port)
                .ok_or_else(|| err("blocked", "领域交付需要原始输出身份"))?;
            let original = one(store, "governance", output_id)?;
            let raw = original
                .value
                .as_ref()
                .ok_or_else(|| err("storage", "原始输出缺失"))?;
            let artifact = text(&raw["data"], "artifact_id")?;
            require(
                raw["type"] == "workflow_output" && raw["data"]["execution_id"] == execution.id,
                "领域交付产物与原执行不符",
            )?;
            self.witness([version(&original)])?;
            let step = root
                .plan
                .document
                .steps
                .iter()
                .find(|step| step.key == source.step)
                .ok_or_else(|| err("storage", "原产物步骤定义缺失"))?;
            let mut signature = None;
            if step.confirmation == ConfirmationPolicy::Human {
                let approvals = one(store, "workflow_confirmations", &execution.id)?;
                signature = approvals
                    .value
                    .as_ref()
                    .and_then(Value::as_array)
                    .and_then(|rows| {
                        rows.iter()
                            .find(|row| {
                                row["data"]["actor"]["kind"] == "human"
                                    && row["data"]["input_digest"] == execution.input_digest
                                    && row["data"]["outputs"] == json!(execution.outputs)
                            })
                            .and_then(|row| row["id"].as_str().map(str::to_owned))
                    });
                require(signature.is_some(), "领域交付缺少绑定原产物的人工签署")?;
                self.witness([version(&approvals)])?;
            }
            let id = self.object_id(&format!("domain-output:{port}"))?;
            let key = domain_entity_key(&root.project_id, &id);
            let entity = json!({
                "id":id,"project_id":root.project_id,"type_ref":target.type_ref,"revision":1,"data":value,
                "origin":{"owner_id":root.owner_id,"owner_revision":owner.revision,
                    "root_id":root.root_instance_id,"plan_revision_id":root.active_plan_revision_id,
                    "execution_id":execution.id,"output_id":output_id,"artifact_id":artifact,
                    "input_digest":execution.input_digest,"value_digest":configuration_hash(value),
                    "producer":raw["data"]["provenance"],"decision_id":decision,"human_signature_id":signature}
            });
            let stored = storage_record(&root.project_id, &key, &binding.digest, &entity, 1)?;
            self.insert_record(&stored)?;
            submitted.insert(port.clone(), id);
        }
        Ok(submitted)
    }
}

pub(crate) fn extract(
    row: &ResourceSnapshot,
    project: &str,
    digest: &str,
) -> Result<Option<Value>> {
    let Some(value) = row.value.as_ref() else {
        return Ok(None);
    };
    let record = ConfigurationRecord::from_storage(value.clone())?;
    require(
        record.r#type == KIND
            && record.scope == Scope::project(project)
            && record.data["digest"] == digest,
        "领域对象不属于当前项目和包版本",
    )?;
    let payload = record.data["payload"]
        .as_str()
        .ok_or_else(|| err("storage", "领域对象数据缺失"))?;
    serde_json::from_str(payload)
        .map(Some)
        .map_err(|e| err("storage", e.to_string()))
}
