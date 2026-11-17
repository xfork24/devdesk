//! Workflow intent observation retains the original dispatch baseline.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    *,
};

/// Captured before executing a durable command. Not deserializable or mutable.
/// The original restored command is readable, while its stored representation
/// and native version remain private and cannot be replaced by a callback.
pub struct PreparedWorkflowIntent {
    namespace: StoreNamespace,
    store_root: String,
    record: ConfigurationRecord,
    stored: ConfigurationRecord,
    version: ResourceVersion,
}
impl std::ops::Deref for PreparedWorkflowIntent {
    type Target = ConfigurationRecord;
    fn deref(&self) -> &Self::Target {
        &self.record
    }
}
impl Engine {
    pub fn evaluate_workflows(&self) -> Result<()> {
        self.evaluate_configured_workflows()
    }
    fn capture_workflow_intent(
        &self,
        original: ConfigurationRecord,
    ) -> Result<PreparedWorkflowIntent> {
        let key = ResourceKey::new("governance", &original.id);
        let rows = self.store.read_resources(std::slice::from_ref(&key))?;
        require(
            rows.len() == 1 && rows[0].resource == key,
            "工作流意图读取不完整",
        )?;
        let stored = ConfigurationRecord::from_storage(
            rows[0]
                .value
                .clone()
                .ok_or_else(|| err("not_found", "工作流意图缺失"))?,
        )?;
        require(
            matches!(self.store.namespace(), StoreNamespace::Engineering { .. })
                && stored.r#type == "workflow_intent"
                && original.r#type == stored.r#type
                && original.scope == stored.scope
                && original.definition_id == stored.definition_id
                && original.data["method"] == stored.data["method"]
                && original.data["command"]["command_id"] == stored.data["command"]["command_id"]
                && self
                    .store
                    .project_id()
                    .is_none_or(|project| stored.scope.project_id() == Some(project.as_str())),
            "工作流意图范围或原请求不符",
        )?;
        if stored.revision != original.revision {
            return Err(err("revision_conflict", "工作流意图在准备前已变化"));
        }
        Ok(PreparedWorkflowIntent {
            namespace: self.store.namespace(),
            store_root: self.store.root(),
            record: original,
            stored,
            version: ResourceVersion {
                resource: key,
                revision: rows[0].revision,
            },
        })
    }
    /// Explicit preparation for internal coordinators outside the pending scan.
    /// This still uses a complete state to restore task inputs; it performs no external execution.
    pub fn prepare_workflow_intent(&self, id: &str) -> Result<PreparedWorkflowIntent> {
        let original = self.store.snapshot()?.configuration(id)?.clone();
        self.capture_workflow_intent(original)
    }
    pub fn pending_workflow_intents(&self) -> Result<Vec<PreparedWorkflowIntent>> {
        if !self
            .store
            .workflow_runtime_pending(ports::WorkflowRuntimeWork::WorkflowIntents)?
        {
            return Ok(vec![]);
        }
        let s = self.store.snapshot()?;
        let active: Vec<String> = s
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_instance" && r.data["status"] != "cancelled")
            .filter(|r| {
                !s.workflow_instance_suspended(&r.id)
                    && r.data["owner_id"]
                        .as_str()
                        .is_some_and(|owner| !s.workflow_paused(owner))
            })
            .flat_map(|r| {
                r.data["steps"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|step| step["status"] == "running")
                    .filter_map(|step| step["id"].as_str().map(str::to_owned))
            })
            .collect();
        let mut active = active;
        active.extend(
            s.governance
                .iter()
                .filter(|r| r.r#type == "workflow_controller" && r.data["status"] == "active")
                .map(|r| r.id.clone()),
        );
        s.governance
            .iter()
            .filter(|r| {
                r.r#type == "workflow_intent"
                    && r.data["status"] == "prepared"
                    && active.contains(&r.definition_id)
            })
            .take(16)
            .cloned()
            .map(|r| self.capture_workflow_intent(r))
            .collect()
    }
    pub fn observe_workflow_intent(
        &self,
        intent: &PreparedWorkflowIntent,
        result: &Result<Value>,
    ) -> Result<()> {
        if intent.namespace != self.store.namespace() || intent.store_root != self.store.root() {
            return Err(err("invalid_scope", "工作流观察不能跨命名空间"));
        }
        if result
            .as_ref()
            .is_err_and(|e| e.code == "workflow_tool_suspended")
        {
            require(
                intent.data["method"] == "workflow.tool.execute"
                    && intent.data["status"] == "prepared",
                "暂停结果不属于原待执行工具意图",
            )?;
            return Ok(());
        }
        let command: Command = serde_json::from_value(intent.data["command"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let method = text(&intent.data, "method")?;
        let fingerprint = command_fingerprint(&method, &command);
        let receipt = self
            .store
            .replay_commit(&command.command_id, &fingerprint)?;
        let reported = match result {
            Ok(value) => {
                json!({"result_id":text(value,"id")?,"result_digest":configuration_hash(value)})
            }
            Err(error) => json!({"error":error}),
        };
        let (observed, summary) = if let Some(receipt) = receipt {
            // A committed command receipt also resolves an unconfirmed response.
            // Preserve that response separately without declaring the effect absent.
            let id = text(&receipt.result, "id")?;
            if let Ok(value) = result {
                require(value["id"] == id, "工作流结果与原操作身份不符")?;
            }
            let summary = json!({"id":id});
            (
                json!({"status":"completed","reported":reported,"operation_id":command.command_id,"operation_fingerprint":fingerprint,"result_digest":configuration_hash(&receipt.result),"result":summary}),
                summary,
            )
        } else if let Err(error) = result {
            (
                json!({"status":"failed","reported":reported,"error":error}),
                Value::Null,
            )
        } else {
            return Err(err("adapter_contract", "工作流成功结果缺少原操作回执"));
        };
        let digest = configuration_hash(&(
            "workflow-intent-observation/v1",
            &intent.id,
            &command.command_id,
            &observed,
        ));
        let observation_key = ResourceKey::new(
            "governance",
            format!("workflow-intent-observation:{digest}"),
        );
        let observe_id = format!("workflow-intent-observe:{digest}");
        if self.store.replay_commit(&observe_id, &digest)?.is_none() {
            let mut row = record(
                "workflow_intent_observation",
                intent.id.clone(),
                intent.scope.clone(),
                method.clone(),
                1,
                None,
                json!({"intent_id":intent.id,"intent_revision":intent.version.revision,"original_command_id":command.command_id,"value":observed}),
                true,
            );
            row.id = observation_key.id.clone();
            self.store.commit_changes(&PreparedChangeSet {
                command_id: observe_id,
                fingerprint: digest.clone(),
                read_set: ReadSet::new(vec![ResourceVersion {
                    resource: observation_key.clone(),
                    revision: 0,
                }])?,
                changes: vec![ResourceChange {
                    resource: observation_key.clone(),
                    value: row.to_storage(),
                }],
                result: json!({"recorded":true}),
                time: now() as i64,
            })?;
        }
        let accept_id = format!("workflow-intent-accept:{digest}");
        if self.store.replay_commit(&accept_id, &digest)?.is_some() {
            return Ok(());
        }
        if intent.data["status"] != "prepared" {
            return Err(err("revision_conflict", "原工作流意图已结束或撤销"));
        }
        let mut row = intent.stored.clone();
        let mut data = row.data.clone();
        data["status"] = observed["status"].clone();
        if observed["status"] == "completed" {
            data["result"] = summary;
        } else {
            data["error"] = observed["error"].clone();
        }
        data["observation_id"] = json!(observation_key.id);
        update_record(&mut row, data);
        self.store.commit_changes(&PreparedChangeSet {
            command_id: accept_id,
            fingerprint: digest,
            read_set: ReadSet::new(vec![
                intent.version.clone(),
                ResourceVersion {
                    resource: ResourceKey::new("tool_intent", &intent.id),
                    revision: intent.record.revision,
                },
                ResourceVersion {
                    resource: observation_key,
                    revision: 1,
                },
            ])?,
            changes: vec![ResourceChange {
                resource: intent.version.resource.clone(),
                value: row.to_storage(),
            }],
            result: json!({"recorded":true}),
            time: now() as i64,
        })?;
        Ok(())
    }
}
