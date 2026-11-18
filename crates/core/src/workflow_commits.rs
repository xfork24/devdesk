//! Explicit workflow resources and mutations. A step update never replaces a
//! sibling, and a header update never writes the steps array.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    material_commands::stable_object_id,
    platform_facts::{one, restored},
    workflow_resources::{WorkflowResources, version},
    *,
};
use std::collections::BTreeMap;

pub(crate) struct WorkflowHeaderFacts {
    pub record: Arc<ConfigurationRecord>,
    pub instance: Arc<ConfiguredWorkflowInstance>,
    pub header: ResourceSnapshot,
}
impl WorkflowHeaderFacts {
    pub fn read(engine: &Engine, id: &str) -> Result<Self> {
        let keys = [
            ResourceKey::new("governance", id),
            ResourceKey::new("workflow_header", id),
        ];
        let rows = engine.store.read_resources(&keys)?;
        require(
            rows.len() == 2 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "流程头事实读取不完整",
        )?;
        let raw = rows[0]
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "原流程缺失"))?;
        let mut actual = raw["data"].clone();
        actual
            .as_object_mut()
            .ok_or_else(|| err("storage", "流程头结构无效"))?
            .remove("steps");
        require(
            rows[1].value.as_ref() == Some(&actual),
            "流程头与实际实例不属于同一快照",
        )?;
        let record = restored(engine, &rows[0])?;
        require(
            record.r#type == "workflow_instance"
                && record.scope.project_id() == engine.store.project_id().as_deref(),
            "流程不属于当前项目",
        )?;
        let instance = record.decode()?;
        Ok(Self {
            record: Arc::new(record),
            instance: Arc::new(instance),
            header: rows[1].clone(),
        })
    }
    pub fn guard_request(&self, c: &Command, step: Option<&ResourceSnapshot>) -> Result<()> {
        let expected = c
            .expected_revision
            .ok_or_else(|| err("precondition_required", "流程写入需要原版本"))?;
        let exact = expected == self.record.revision;
        let header = expected > 0
            && expected <= self.record.revision
            && c.payload["control_version"] == self.instance.control_version
            && c.payload["plan_revision_id"] == self.instance.active_plan_revision_id;
        if !exact && !header {
            return Err(err(
                "revision_conflict",
                "流程原版本已变化，请保留原请求并刷新",
            ));
        }
        if let Some(plan) = c.payload["plan_revision_id"].as_str() {
            require(
                plan == self.instance.active_plan_revision_id,
                "原计划已变化",
            )?;
        }
        if let Some(step) = step {
            if let Some(expected) = c.payload["expected_step_revision"].as_u64() {
                if expected != step.revision {
                    return Err(err("revision_conflict", "步骤原版本已变化"));
                }
            } else if !exact {
                return Err(err("precondition_required", "跨流程进度提交需要原步骤版本"));
            }
        }
        Ok(())
    }
    pub fn step(&self, engine: &Engine, key: &str) -> Result<WorkflowResources> {
        let execution = self
            .instance
            .steps
            .iter()
            .find(|e| e.step == key || e.node_id == key || e.id == key)
            .ok_or_else(|| err("not_found", "原步骤不存在"))?;
        WorkflowResources::from_header(
            engine.store.as_ref(),
            &self.record,
            &self.instance,
            &self.header,
            &execution.id,
        )
    }
    pub fn active(&self) -> Result<()> {
        require(
            !self.instance.cancel_requested
                && !matches!(
                    self.instance.status,
                    StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
                ),
            "流程已结束或已请求取消",
        )
    }
}
pub(crate) struct WorkflowCommit<'a> {
    pub engine: &'a Engine,
    pub command: &'a Command,
    pub method: &'a str,
    pub context: &'a InvocationContext,
    pub reads: ReadSet,
    pub coordinator_origin: Option<Value>,
    pub identity: String,
    order: Vec<ResourceKey>,
    changes: BTreeMap<ResourceKey, Value>,
}
impl<'a> WorkflowCommit<'a> {
    pub fn new(
        engine: &'a Engine,
        method: &'a str,
        c: &'a Command,
        context: &'a InvocationContext,
    ) -> Result<Self> {
        context.require_scope(engine.store.project_id().as_deref())?;
        Ok(Self {
            engine,
            command: c,
            method,
            context,
            reads: context.original_authority().clone(),
            coordinator_origin: None,
            identity: c.command_id.clone(),
            order: vec![],
            changes: BTreeMap::new(),
        })
    }
    pub fn witness(&mut self, rows: impl IntoIterator<Item = ResourceVersion>) -> Result<()> {
        self.reads = self.reads.merge(rows)?;
        Ok(())
    }
    pub fn change(&mut self, change: ResourceChange) -> Result<()> {
        self.reads.require(&change.resource)?;
        if self.changes.contains_key(&change.resource) {
            return Err(err("invalid_params", "同一流程资源存在重复变化"));
        }
        self.order.push(change.resource.clone());
        self.changes.insert(change.resource, change.value);
        Ok(())
    }
    pub fn new_record(&mut self, mut row: ConfigurationRecord, kind: &str) -> Result<Id> {
        row.id = self.object_id(kind)?;
        let key = ResourceKey::new("governance", &row.id);
        self.witness([ResourceVersion {
            resource: key.clone(),
            revision: 0,
        }])?;
        self.change(ResourceChange {
            resource: key,
            value: self.engine.store.encode_runtime_record(&row)?,
        })?;
        Ok(row.id)
    }
    pub fn insert_record(&mut self, row: &ConfigurationRecord) -> Result<()> {
        let key = ResourceKey::new("governance", &row.id);
        self.witness([ResourceVersion {
            resource: key.clone(),
            revision: 0,
        }])?;
        self.change(ResourceChange {
            resource: key,
            value: self.engine.store.encode_runtime_record(row)?,
        })
    }
    pub fn replace_step(
        &mut self,
        resources: &WorkflowResources,
        next: &StepExecution,
    ) -> Result<()> {
        require(
            next.workflow_instance_id == resources.record.id
                && next.id != resources.execution.id
                && next.step == resources.execution.step,
            "替换执行不能更换实际步骤或实例",
        )?;
        let key = ResourceKey::new("workflow_step_replace", resources.step.resource.id.clone());
        self.witness(resources.reads.versions().iter().cloned())?;
        self.witness([
            ResourceVersion {
                resource: key.clone(),
                revision: resources.step.revision,
            },
            ResourceVersion {
                resource: ResourceKey::new(
                    "workflow_step",
                    json!([resources.record.id, next.id]).to_string(),
                ),
                revision: 0,
            },
        ])?;
        self.change(ResourceChange {
            resource: key,
            value: self.engine.store.encode_runtime_value(&json!(next))?,
        })
    }
    pub fn replace_record(
        &mut self,
        original: &ResourceSnapshot,
        mut row: ConfigurationRecord,
        data: Value,
    ) -> Result<()> {
        require(
            original.resource == ResourceKey::new("governance", &row.id)
                && row.revision == original.revision,
            "原流程记录版本或身份不符",
        )?;
        if row.data == data {
            return self.witness([version(original)]);
        }
        update_record(&mut row, data);
        self.witness([version(original)])?;
        self.change(ResourceChange {
            resource: original.resource.clone(),
            value: self.engine.store.encode_runtime_record(&row)?,
        })
    }
    pub fn header(
        &mut self,
        original: &WorkflowHeaderFacts,
        next: &ConfiguredWorkflowInstance,
    ) -> Result<()> {
        let value=self.engine.store.encode_runtime_value(&json!({"active_plan_revision_id":next.active_plan_revision_id,"control_version":next.control_version,"schema_version":next.schema_version,"owner_id":next.owner_id,"workflow_ref":next.workflow_ref,"project_id":next.project_id,"plan":next.plan,"role_agent_bindings":next.role_agent_bindings,"inputs":next.inputs,"user_request":next.user_request,"human_governance":next.human_governance,"status":next.status,"paused":next.paused,"parent_execution_id":next.parent_execution_id,"root_instance_id":next.root_instance_id,"cancel_requested":next.cancel_requested}))?;
        self.witness([version(&original.header)])?;
        self.change(ResourceChange {
            resource: original.header.resource.clone(),
            value,
        })
    }
    pub fn step(&mut self, resources: &WorkflowResources, next: &StepExecution) -> Result<()> {
        self.witness(resources.reads.versions().iter().cloned())?;
        self.change(resources.changed_step(self.engine.store.as_ref(), next)?)
    }
    pub fn has_changes(&self) -> bool {
        !self.changes.is_empty()
    }
    pub(crate) fn contains_change(&self, key: &ResourceKey) -> bool {
        self.changes.contains_key(key)
    }
    pub fn clear_changes(&mut self) {
        self.order.clear();
        self.changes.clear();
    }
    pub fn object_id(&self, kind: &str) -> Result<Id> {
        stable_object_id(self.engine.store.as_ref(), &self.identity, kind)
    }
    pub fn decision(
        &mut self,
        root: &WorkflowHeaderFacts,
        execution: Option<&StepExecution>,
        operation: &str,
        reason: &str,
    ) -> Result<Id> {
        require(
            self.coordinator_origin.is_none(),
            "后台事实投影不能制造新的执行决定",
        )?;
        self.new_record(
            record(
                "workflow_decision",
                root.record.id.clone(),
                root.record.scope.clone(),
                operation.into(),
                1,
                None,
                json!(WorkflowDecision {
                    root_instance_id: root.instance.root_instance_id.clone(),
                    instance_id: root.record.id.clone(),
                    plan_revision_id: execution
                        .map(|e| e.plan_revision_id.clone())
                        .unwrap_or_else(|| root.instance.active_plan_revision_id.clone()),
                    execution_id: execution.map(|e| e.id.clone()),
                    actor: self.context.actor().clone(),
                    operation: operation.into(),
                    reason: reason.into(),
                    input_digest: execution
                        .map(|e| e.input_digest.clone())
                        .unwrap_or_default(),
                    outputs: execution.map(|e| e.outputs.clone()).unwrap_or_default()
                }),
                true,
            ),
            &format!(
                "workflow-decision:{operation}:{}",
                execution.map(|e| e.id.as_str()).unwrap_or(&root.record.id)
            ),
        )
    }
    pub fn require_no_change(&mut self, instance: &str) -> Result<()> {
        let row = one(
            self.engine.store.as_ref(),
            "workflow_change_pending",
            instance,
        )?;
        if row.value.is_some() {
            return Err(err("change_pending", "运行计划正在变更"));
        }
        self.witness([version(&row)])
    }
    pub fn prepare(mut self, fingerprint: &str, result: Value) -> Result<PreparedChangeSet> {
        // A coordinator can observe several different input fact sets while
        // the target's own revision stays unchanged. Its derived receipt and
        // event must identify that exact observation, not only the target.
        // Public commands and supplier callbacks keep their original identity.
        let mut command = self.command.clone();
        if self.coordinator_origin.is_some() {
            command.command_id = format!("{}:{fingerprint}", command.command_id);
        }
        let mut provenance = self.context.prepare_record(self.method, &command);
        if let Some(origin) = &self.coordinator_origin {
            let mut row = ConfigurationRecord::from_storage(provenance.value)?;
            row.data["origin"] = origin.clone();
            row.content_hash = content_digest(&row.refs, &row.data);
            provenance.value = row.to_storage();
        }
        self.witness([ResourceVersion {
            resource: provenance.resource.clone(),
            revision: 0,
        }])?;
        self.change(provenance)?;
        let event = ResourceKey::new(
            "event_entry",
            stable_object_id(
                self.engine.store.as_ref(),
                &command.command_id,
                "workflow-event",
            )?,
        );
        self.witness([ResourceVersion {
            resource: event.clone(),
            revision: 0,
        }])?;
        self.change(ResourceChange{resource:event,value:json!({"target":command.target.as_deref().or_else(||result["id"].as_str()).unwrap_or(&command.command_id),"kind":self.method,"detail":json!({"command_id":command.command_id,"actor":self.context.actor()}).to_string(),"time":now()})})?;
        Ok(PreparedChangeSet {
            command_id: command.command_id,
            fingerprint: fingerprint.into(),
            read_set: self.reads,
            changes: self
                .order
                .into_iter()
                .map(|resource| {
                    let value = self.changes.remove(&resource).unwrap();
                    ResourceChange { resource, value }
                })
                .collect(),
            result,
            time: now() as i64,
        })
    }
    pub fn finish(self, fingerprint: &str, result: Value) -> Result<Value> {
        let engine = self.engine;
        engine
            .store
            .commit_changes(&self.prepare(fingerprint, result)?)
            .map(|r| r.result)
    }
}
