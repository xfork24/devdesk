//! Exact read-only control facts do not reconstruct an unrelated frozen plan.
use crate::{
    platform_facts::{one, restored},
    *,
};
impl Engine {
    pub(crate) fn finite_workflow_control_read(
        &self,
        method: &str,
        p: &Value,
    ) -> Option<Result<Value>> {
        if !p["decision_ref"].is_null()
            || !matches!(
                method,
                "workflow.outputs.read" | "workflow.execution.inspect"
            )
            || p["part"] == "contract"
        {
            return None;
        }
        Some((|| {
            let instance = p["id"]
                .as_str()
                .or_else(|| p["instance_id"].as_str())
                .ok_or_else(|| err("invalid_params", "缺少流程"))?;
            let run = one(self.store.as_ref(), "workflow_run_authority", instance)?;
            let run = run
                .value
                .as_ref()
                .ok_or_else(|| err("not_found", "原流程缺失"))?;
            let project = text(run, "project_id")?;
            let root = text(run, "root_instance_id")?;
            require(
                self.store.project_id().as_deref() == Some(project.as_str())
                    && serde_json::from_value::<Scope>(run["scope"].clone())
                        .is_ok_and(|scope| scope.project_id() == Some(project.as_str()))
                    && p["project_id"].as_str().is_none_or(|id| id == project),
                "项目不匹配",
            )?;
            if method == "workflow.outputs.read" {
                return self.native_root_outputs(&project, &root, p);
            }
            let id = text(p, "execution_id")?;
            let current = one(
                self.store.as_ref(),
                "workflow_step",
                &json!([instance, id]).to_string(),
            )?;
            let execution: StepExecution = if let Some(raw) = current.value {
                serde_json::from_value(self.store.restore_runtime_value(&raw)?)
                    .map_err(|e| err("storage", e.to_string()))?
            } else {
                let row = restored(
                    self,
                    &one(self.store.as_ref(), "workflow_history_source", &id)?,
                )?;
                require(
                    row.r#type == "workflow_execution_history"
                        && row.scope.project_id() == Some(project.as_str()),
                    "原执行历史跨项目",
                )?;
                row.decode()?
            };
            require(
                execution.id == id && execution.workflow_instance_id == instance,
                "原执行不属于当前流程",
            )?;
            if let Some(part) = p["part"].as_str() {
                require(part == "inputs", "未知执行读取部分")?;
                let mut out = crate::orchestration::read_chunk(&json!(execution.inputs), p)?;
                out["execution_id"] = json!(id);
                out["plan_revision_id"] = json!(execution.plan_revision_id);
                out["part"] = json!(part);
                return Ok(out);
            }
            let mut out = json!(execution);
            if execution.status == StepExecutionStatus::NeedsReconciliation
                && let Some((failed, previous)) = self.workspace_recovery_source(&execution)?
            {
                out["reconciliation"] = json!({"operation":"workflow.execution.reconcile","available_without_human_signature":true,"failed_turn_id":failed,"checkpoint_turn_id":previous,"basis":"Original startup reported not_applied and this execution has a sealed workspace continuation checkpoint. Reconcile verifies unchanged files, original wait timing, remaining budget and actual stop authority before restoring. Decide whether the situation additionally needs Human judgment."});
            }
            if let Some(progress) = self.publication_read_progress(&project, &root, &execution)? {
                out["publication"] = progress;
            }
            Ok(out)
        })())
    }
    pub(crate) fn publication_read_progress(
        &self,
        project: &str,
        root: &str,
        execution: &StepExecution,
    ) -> Result<Option<Value>> {
        let Some(id) = &execution.tool_call_id else {
            return Ok(None);
        };
        let source = one(self.store.as_ref(), "governance", id)?;
        let Some(raw) = source.value else {
            return Ok(None);
        };
        let call = ConfigurationRecord::from_storage(raw)?;
        if call.r#type != "platform_call" || call.data["handler"] != "git.publish" {
            return Ok(None);
        }
        let data = &call.data;
        require(
            call.scope.project_id() == Some(project)
                && data["issuer"] == "kernel_platform"
                && data["root_instance_id"] == root
                && data["request"]["execution_id"] == execution.id
                && data["request"]["instance_id"] == execution.workflow_instance_id
                && data["request"]["project_id"] == project,
            "原发布进度跨执行或根",
        )?;
        let Some(effect) = data["effect_id"].as_str() else {
            return Ok(None);
        };
        let stage = format!("platform-publish-stage:{effect}:pull_request");
        let grant = one(self.store.as_ref(), "governance", &stage)?;
        let Some(raw) = grant.value else {
            return Ok(None);
        };
        let grant = ConfigurationRecord::from_storage(raw)?;
        require(
            grant.r#type == "platform_publish_stage"
                && grant.immutable
                && grant.scope.project_id() == Some(project)
                && grant.definition_id == effect
                && grant.data["issuer"] == "kernel_platform",
            "原发布阶段身份不符",
        )?;
        let request: PullRequestRequest =
            serde_json::from_value(grant.data["action"]["PullRequest"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        let observation = one(
            self.store.as_ref(),
            "platform_observation_head",
            &json!([stage, "result"]).to_string(),
        )?;
        let Some(raw) = observation.value else {
            return Ok(None);
        };
        let row = ConfigurationRecord::from_storage(raw)?;
        require(
            row.r#type == "platform_effect_observation"
                && row.immutable
                && row.scope.project_id() == Some(project)
                && row.definition_id == stage
                && row.data["effect_id"] == stage
                && row.data["phase"] == "result"
                && row.data["issuer"] == "kernel_platform",
            "原发布观察身份不符",
        )?;
        let Some(value) = row.data["result"].get("Ok") else {
            return Ok(None);
        };
        let receipt: PullRequestReceipt =
            serde_json::from_value(value.clone()).map_err(|e| err("storage", e.to_string()))?;
        require(
            receipt.head == data["request"]["arguments"]["candidate"]["revision"]
                && receipt.repository == request.repository
                && receipt.head == request.candidate
                && receipt.base_branch == request.base_branch
                && receipt.head_branch == request.head_branch
                && receipt.marker == request.marker()
                && request.project == project,
            "原发布观察改变候选",
        )?;
        Ok(Some(
            json!({"call_id":id,"effect_id":effect,"status":data["status"],"candidate":receipt.head,"creation_receipt":receipt,"observation_id":row.id,"observed_at":row.created_at,"advisory":true,"next":"External merge requires a separate authorized action; this creation observation is not a current remote-state read or a completion receipt."}),
        ))
    }
    pub(crate) fn native_root_outputs(
        &self,
        project: &str,
        root: &str,
        p: &Value,
    ) -> Result<Value> {
        let ids: Vec<Id> = serde_json::from_value(p.get("ids").cloned().unwrap_or(json!([])))
            .map_err(|e| err("invalid_params", e.to_string()))?;
        require(ids.len() <= 100, "最多读取 100 项产物")?;
        let mut out = vec![];
        let budget =
            p["max_bytes"].as_u64().unwrap_or(16384).clamp(1024, 65536) / ids.len().max(1) as u64;
        for id in ids {
            let pointer = p["pointer"].as_str().unwrap_or("");
            let (row, value, _) = self.workflow_material_source(project, root, &id, pointer)?;
            let input_ref = crate::workflow_material_inputs::MaterialInput {
                source_id: id,
                pointer: pointer.into(),
                value_digest: configuration_hash(&value),
            }
            .reference();
            let mut request = p.clone();
            request["max_bytes"] = json!(budget.max(1));
            let chunk = crate::orchestration::read_chunk(&value, &request)?;
            let mut raw = if chunk["next_offset"].is_null() && chunk["offset"] == 0 {
                let mut raw = json!(row);
                raw["data"]["value"] = value.clone();
                raw
            } else {
                let mut data = row.data.clone();
                data.as_object_mut().unwrap().remove("value");
                let mut raw = chunk;
                raw["id"] = json!(row.id);
                raw["data"] = data;
                raw
            };
            raw["input_ref"] = json!(input_ref);
            out.push(raw);
        }
        Ok(
            json!({"outputs":out,"material_transfer":{"tool":"workflow.step.input","input":"When the receiver is waiting_input, forward one returned input_ref verbatim as workflow.step.input's input string. This grants read access to that exact source field; previously granted references remain readable for the same execution. Copying IDs or input_ref strings into prose, constant JSON, or a list of step inputs does not grant source access. For additional sources, forward each exact reference when the receiver next requests input. Existing platform evidence is not a Human answer or approval. Decide separately whether Human judgment is needed; an explicit decision.request remains a Human request and is never automatically bypassed."}}),
        )
    }
}
