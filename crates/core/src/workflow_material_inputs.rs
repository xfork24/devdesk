//! Platform material is supplemental evidence, never a Human answer or approval.
use crate::{
    concurrency::*,
    governance::record,
    platform_facts::{one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_resources::version,
    *,
};

const PREFIX: &str = "devdesk-material-input:";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MaterialInput {
    pub source_id: Id,
    pub pointer: String,
    pub value_digest: String,
}
impl MaterialInput {
    pub fn is_reference(input: &str) -> bool {
        input.starts_with(PREFIX)
    }
    pub fn reference(&self) -> String {
        format!("{PREFIX}{}", serde_json::to_string(self).unwrap())
    }
    pub fn rendered(&self) -> String {
        json!({"kind":"platform_material","source_id":self.source_id,"pointer":self.pointer,"value_digest":self.value_digest,
            "read_tool":"workflow.outputs.read","arguments":{"ids":[self.source_id],"pointer":self.pointer,"offset":0,"max_bytes":16384},
            "instructions":"This is existing platform evidence, not a Human answer or approval. Read every chunk through next_offset before judging it. Preserve the original task and Human gates."}).to_string()
    }
}
impl Engine {
    pub(crate) fn reconcile_material_decision(
        &self,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Value> {
        require(
            matches!(context.actor(), WorkflowActor::Human),
            "仅限维护已有材料转交请求",
        )?;
        let reason = text(&c.payload, "reason")?;
        if let Some(receipt) = self.store.replay_commit(&c.command_id, fingerprint)? {
            return Ok(receipt.result);
        }
        let selected = one(
            self.store.as_ref(),
            "governance",
            c.target
                .as_deref()
                .ok_or_else(|| err("invalid_params", "缺少原请求"))?,
        )?;
        let pending = restored(self, &selected)?;
        require(
            c.expected_revision == Some(selected.revision)
                && pending.r#type == "workflow_pending_decision"
                && pending.data["status"] == "pending"
                && pending.data["request"]["operation"] == "workflow.step.input",
            "原材料转交请求已变化",
        )?;
        let root = WorkflowHeaderFacts::read(self, &pending.definition_id)?;
        root.active()?;
        require(
            !root.instance.paused
                && root.instance.active_plan_revision_id == pending.data["plan_revision_id"],
            "原计划或控制状态已变化",
        )?;
        let args = &pending.data["request"]["arguments"];
        let resources = root.step(self, &text(args, "execution_id")?)?;
        require(
            resources.execution.status == StepExecutionStatus::WaitingInput
                && args["expected_step_revision"] == resources.step.revision
                && args["control_version"] == root.instance.control_version,
            "原等待输入的执行已变化",
        )?;
        let actor: WorkflowActor = serde_json::from_value(pending.data["actor"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let WorkflowActor::Controller {
            controller_id,
            session_id,
            epoch,
            ..
        } = &actor
        else {
            return Err(err("blocked", "原请求不来自项目经理"));
        };
        let authority = one(self.store.as_ref(), "controller_authority", controller_id)?;
        require(
            authority.value.as_ref().is_some_and(|r| {
                r["data"]["status"] == "active"
                    && r["data"]["session_id"] == *session_id
                    && r["data"]["epoch"] == *epoch
            }),
            "原经理已撤权",
        )?;
        let mut commit = WorkflowCommit::new(self, "workflow.decision.reconcile", c, context)?;
        commit.witness([
            version(&selected),
            version(&authority),
            version(&root.header),
        ])?;
        commit.witness(resources.reads.versions().iter().cloned())?;
        let material = commit
            .resolve_material_input_from(&root, &text(args, "input")?, &actor)?
            .ok_or_else(|| err("blocked", "请求不属于可核对的原始平台材料"))?;
        let mut data = pending.data.clone();
        data["status"] = json!("withdrawn");
        data["material_input_ref"] = json!(material.reference());
        data["resolution_reason"] = json!(reason);
        commit.replace_record(&selected, pending.clone(), data)?;
        commit.new_record(record("workflow_material_transfer_resolution",pending.id.clone(),root.record.scope.clone(),"按维护指令撤回材料转交确认".into(),1,None,json!({"decision_id":pending.id,"material":material,"origin_actor":actor,"resolution":"withdrawn","reason":reason}),true),"material-transfer-resolution")?;
        commit.finish(
            fingerprint,
            json!({"id":pending.id,"status":"withdrawn","material_input_ref":material.reference()}),
        )
    }
    pub(crate) fn supplemental_material_read(
        &self,
        instance: &ConfiguredWorkflowInstance,
        execution: &StepExecution,
        id: &str,
        pointer: &str,
    ) -> Result<Option<ReadSet>> {
        let selected = one(
            self.store.as_ref(),
            "workflow_material_inputs",
            &execution.id,
        )?;
        for raw in selected
            .value
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let row = ConfigurationRecord::from_storage(raw.clone())?;
            let data = &row.data;
            if !row.immutable
                || row.r#type != "workflow_material_input"
                || row.scope.project_id() != Some(instance.project_id.as_str())
                || row.definition_id != execution.id
                || data["issuer"] != "kernel_platform"
                || data["root_instance_id"] != instance.root_instance_id
                || data["instance_id"] != execution.workflow_instance_id
                || data["execution_id"] != execution.id
                || data["execution_plan_revision_id"] != execution.plan_revision_id
                || data["contract_digest"] != execution.contract_digest
                || data["input_digest"] != execution.input_digest
                || data["sequence"]
                    .as_u64()
                    .is_none_or(|s| s > execution.sequence)
                || data["material"]["source_id"] != id
                || data["material"]["pointer"] != pointer
            {
                continue;
            }
            let (_, value, reads) = self.workflow_material_source(
                &instance.project_id,
                &instance.root_instance_id,
                id,
                pointer,
            )?;
            require(
                data["material"]["value_digest"] == configuration_hash(&value),
                "补充材料原件已变化",
            )?;
            return Ok(Some(reads.merge([ResourceVersion {
                resource: ResourceKey::new("governance", &row.id),
                revision: row.revision,
            }])?));
        }
        Ok(None)
    }
    pub(crate) fn workflow_material_source(
        &self,
        project: &str,
        root: &str,
        id: &str,
        pointer: &str,
    ) -> Result<(ConfigurationRecord, Value, ReadSet)> {
        let selected = one(self.store.as_ref(), "governance", id)?;
        let row = restored(self, &selected)?;
        require(
            row.immutable
                && row.scope.project_id() == Some(project)
                && matches!(row.r#type.as_str(), "workflow_output" | "typed_material"),
            "原材料不属于当前项目",
        )?;
        let mut reads = ReadSet::new(vec![version(&selected)])?;
        if row.r#type == "typed_material" {
            require(row.data["root_instance_id"] == root, "材料跨流程根")?;
        } else {
            let execution = text(&row.data, "execution_id")?;
            let owner = one(self.store.as_ref(), "workflow_execution_owner", &execution)?;
            reads = reads.merge([version(&owner)])?;
            let instance = if let Some(raw) = &owner.value {
                text(raw, "instance_id")?
            } else {
                let history = one(self.store.as_ref(), "workflow_history_source", &execution)?;
                reads = reads.merge([version(&history)])?;
                text(&restored(self, &history)?.data, "workflow_instance_id")?
            };
            let run = one(self.store.as_ref(), "workflow_run_authority", &instance)?;
            reads = reads.merge([version(&run)])?;
            require(
                run.value
                    .as_ref()
                    .is_some_and(|r| r["root_instance_id"] == root && r["project_id"] == project),
                "材料跨流程根",
            )?;
        }
        let value = row.data["value"]
            .pointer(pointer)
            .ok_or_else(|| err("invalid_params", "材料字段不存在"))?
            .clone();
        Ok((row, value, reads))
    }
}
impl WorkflowCommit<'_> {
    pub(crate) fn resolve_material_input(
        &mut self,
        root: &WorkflowHeaderFacts,
        input: &str,
    ) -> Result<Option<MaterialInput>> {
        let actor = self.context.actor().clone();
        self.resolve_material_input_from(root, input, &actor)
    }
    pub(crate) fn resolve_material_input_from(
        &mut self,
        root: &WorkflowHeaderFacts,
        input: &str,
        source_actor: &WorkflowActor,
    ) -> Result<Option<MaterialInput>> {
        if let Some(encoded) = input.strip_prefix(PREFIX) {
            let reference: MaterialInput =
                serde_json::from_str(encoded).map_err(|e| err("invalid_params", e.to_string()))?;
            let (_, value, reads) = self.engine.workflow_material_source(
                &root.instance.project_id,
                &root.instance.root_instance_id,
                &reference.source_id,
                &reference.pointer,
            )?;
            require(
                configuration_hash(&value) == reference.value_digest,
                "原材料内容摘要不匹配",
            )?;
            self.witness(reads.versions().iter().cloned())?;
            return Ok(Some(reference));
        }
        let WorkflowActor::Controller {
            controller_id,
            session_id,
            epoch,
            ..
        } = source_actor
        else {
            return Ok(None);
        };
        let (controller_id, session_id, epoch) =
            (controller_id.clone(), session_id.clone(), *epoch);
        // Backwards-compatible exact forwarding after real platform reads.
        // Discover bounded candidates, then witness each selected source/call;
        // appending an unrelated read does not revoke an admitted transfer.
        let candidates = one(
            self.engine.store.as_ref(),
            "controller_material_sources",
            &json!([controller_id, epoch, session_id]).to_string(),
        )?;
        let parsed = serde_json::from_str::<Value>(input).ok();
        for candidate in candidates
            .value
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let id = text(candidate, "source_id")?;
            let pointer = candidate["pointer"].as_str().unwrap_or("");
            let Ok((_, value, reads)) = self.engine.workflow_material_source(
                &root.instance.project_id,
                &root.instance.root_instance_id,
                &id,
                pointer,
            ) else {
                continue;
            };
            // Older managers wrapped the exact report in explanatory prose.
            // Discard that prose and forward only the sealed source reference;
            // it must never become a Human answer or an extra instruction.
            let contains_original =
                (value.is_object() || value.is_array()) && input.contains(&value.to_string());
            if value.as_str() != Some(input)
                && parsed.as_ref() != Some(&value)
                && !contains_original
            {
                continue;
            }
            let call = one(
                self.engine.store.as_ref(),
                "governance",
                &text(candidate, "call_id")?,
            )?;
            let row = restored(self.engine, &call)?;
            require(
                row.r#type == "controller_tool_call"
                    && row.definition_id == controller_id
                    && row.data["epoch"] == epoch
                    && row.data["session_id"] == session_id
                    && row.data["method"] == "workflow.outputs.read"
                    && row.data["status"] == "succeeded"
                    && row.data["arguments"]["ids"]
                        .as_array()
                        .is_some_and(|ids| ids.contains(&json!(id)))
                    && row.data["arguments"]["pointer"].as_str().unwrap_or("") == pointer,
                "原材料读取来源不符",
            )?;
            self.witness(reads.versions().iter().cloned())?;
            self.witness([version(&call)])?;
            return Ok(Some(MaterialInput {
                source_id: id,
                pointer: pointer.into(),
                value_digest: configuration_hash(&value),
            }));
        }
        Ok(None)
    }
    pub(crate) fn grant_material_input(
        &mut self,
        resources: &crate::workflow_resources::WorkflowResources,
        next: &StepExecution,
        reference: &MaterialInput,
    ) -> Result<Id> {
        self.new_record(record("workflow_material_input",next.id.clone(),resources.record.scope.clone(),"转交已有平台材料".into(),1,None,
            json!({"issuer":"kernel_platform","root_instance_id":resources.instance.root_instance_id,"instance_id":resources.record.id,
                "execution_id":next.id,"execution_plan_revision_id":next.plan_revision_id,"contract_digest":next.contract_digest,"input_digest":next.input_digest,
                "sequence":next.sequence,"material":reference,"actor":self.context.actor()}),true),"material-input")
    }
}
