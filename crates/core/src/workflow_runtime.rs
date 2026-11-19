//! Workflow intent observation retains the original dispatch baseline.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    *,
};

pub struct WorkflowIntentPage {
    pub ids: Vec<Id>,
    pub next_after: Option<Id>,
}
/// One native intent and its exact decoded command; no aggregate business state.
pub struct WorkflowIntentFacts {
    pub row: ResourceSnapshot,
    pub record: ConfigurationRecord,
    pub active: bool,
}
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
        facts: WorkflowIntentFacts,
    ) -> Result<PreparedWorkflowIntent> {
        let original = facts.record;
        let stored = ConfigurationRecord::from_storage(
            facts
                .row
                .value
                .ok_or_else(|| err("not_found", "工作流意图缺失"))?,
        )?;
        require(
            matches!(self.store.namespace(), StoreNamespace::Engineering { .. })
                && stored.r#type == "workflow_intent"
                && original.id == stored.id
                && original.scope == stored.scope
                && original.revision == stored.revision
                && self
                    .store
                    .project_id()
                    .is_none_or(|project| stored.scope.project_id() == Some(project.as_str())),
            "工作流意图范围或原版本不符",
        )?;
        Ok(PreparedWorkflowIntent {
            namespace: self.store.namespace(),
            store_root: self.store.root(),
            record: original,
            stored,
            version: ResourceVersion {
                resource: facts.row.resource,
                revision: facts.row.revision,
            },
        })
    }
    pub fn prepare_workflow_intent(&self, id: &str) -> Result<PreparedWorkflowIntent> {
        let facts = self
            .store
            .workflow_intent_facts(id, false)?
            .ok_or_else(|| err("not_found", "工作流意图缺失"))?;
        self.capture_workflow_intent(facts)
    }
    pub fn workflow_intent_drivable(&self, intent: &PreparedWorkflowIntent) -> Result<bool> {
        if intent.namespace != self.store.namespace() || intent.store_root != self.store.root() {
            return Err(err("invalid_scope", "工作流意图不能跨数据集"));
        }
        self.store
            .workflow_intent_active(&intent.id, Some(intent.record.revision))
    }
    pub fn pending_workflow_intents(&self) -> Result<Vec<PreparedWorkflowIntent>> {
        let after = self
            .workflow_intent_cursor
            .lock()
            .map_err(|e| err("storage", e.to_string()))?
            .clone();
        let mut page = self.store.workflow_intent_page(after.as_deref())?;
        if page.ids.is_empty() && after.is_some() {
            page = self.store.workflow_intent_page(None)?;
        }
        let mut result = Vec::new();
        for id in &page.ids {
            if let Some(facts) = self.store.workflow_intent_facts(id, true)? {
                require(facts.active, "待执行意图需要当前资格")?;
                result.push(self.capture_workflow_intent(facts)?);
            }
        }
        *self
            .workflow_intent_cursor
            .lock()
            .map_err(|e| err("storage", e.to_string()))? = page.next_after;
        Ok(result)
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
        // No operation was admitted. Preserve the exact durable command for a
        // fresh scheduling pass; a transient conflict is not a failed execution.
        // A committed receipt takes precedence over the caller's stale response.
        if receipt.is_none()
            && result
                .as_ref()
                .is_err_and(|error| error.retryable_conflict())
        {
            return Ok(());
        }
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
                json!({"status":"completed","reported":reported,"operation_id":command.command_id,"operation_fingerprint_digest":configuration_hash(&fingerprint),"result_digest":configuration_hash(&receipt.result),"result":summary}),
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
