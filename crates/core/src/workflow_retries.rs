//! A retry/rework creates new execution identities and archives each original
//! attempt. Unknown effects remain behind an explicit reconciliation request.
use crate::{
    governance::record,
    material_commands::stable_object_id,
    platform_facts::one,
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_resources::{WorkflowResources, version},
    *,
};
use std::collections::BTreeSet;
impl Engine {
    pub(crate) fn prepare_native_retry(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
        rework: bool,
    ) -> Result<StepExecution> {
        let execution = &resources.execution;
        let definition = resources
            .instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == execution.step)
            .ok_or_else(|| err("storage", "原重试步骤定义缺失"))?;
        require(
            rework
                || matches!(
                    execution.status,
                    StepExecutionStatus::Failed | StepExecutionStatus::Cancelled
                ),
            "此执行不能技术重试",
        )?;
        require(
            !commit.execution_active(resources)?,
            "原执行或未知副作用尚未停止并核对",
        )?;
        if !rework {
            if execution
                .failure
                .as_ref()
                .is_some_and(|f| f.effect == ExecutionFailureEffect::Unknown)
            {
                return Err(err(
                    "needs_reconciliation",
                    "未知副作用必须先核对，不能技术重发",
                ));
            }
            let retries = one(
                self.store.as_ref(),
                "workflow_retry_count",
                &json!([root.record.id, execution.node_id, execution.round]).to_string(),
            )?;
            require(
                retries
                    .value
                    .as_ref()
                    .and_then(|v| v["count"].as_u64())
                    .is_some_and(|n| n < definition.budget.retry_limit),
                "技术重试预算已经用尽",
            )?;
            commit.witness([version(&retries)])?;
        }
        let impacted = if rework {
            root.instance
                .plan
                .document
                .impacted_nodes(&BTreeSet::from([execution.step.clone()]))
        } else {
            BTreeSet::new()
        };
        let checkpoint = if rework {
            self.native_rework_checkpoint(commit, resources, definition)?
        } else {
            execution.rework_workspace.clone()
        };
        let mut result = None;
        for previous in root
            .instance
            .steps
            .iter()
            .filter(|e| e.id == execution.id || (rework && impacted.contains(&e.step)))
        {
            let selected = root.step(self, &previous.id)?;
            require(
                !commit.execution_active(&selected)?,
                "受影响执行或子流程仍有活动写者或未核对副作用",
            )?;
            self.cancel_quiescent_native_children(commit, &selected.execution, 0)?;
            let current = &selected.execution;
            commit.new_record(
                record(
                    "workflow_execution_history",
                    current.id.clone(),
                    root.record.scope.clone(),
                    current.step.clone(),
                    1,
                    None,
                    json!(current),
                    true,
                ),
                &format!("workflow-history:{}", current.id),
            )?;
            let mut next = current.clone();
            next.id = stable_object_id(
                self.store.as_ref(),
                &commit.command.command_id,
                &format!("replacement:{}", current.id),
            )?;
            let definition = root
                .instance
                .plan
                .document
                .steps
                .iter()
                .find(|s| s.key == current.step)
                .ok_or_else(|| err("storage", "后继步骤定义缺失"))?;
            next.plan_revision_id = root.instance.active_plan_revision_id.clone();
            next.workflow_revision_id = root.instance.workflow_ref.clone();
            next.node_id = definition.node_id.clone();
            next.contract_digest = configuration_hash(definition);
            next.rework_workspace = checkpoint.clone();
            if rework {
                let outputs = if current.outputs.is_empty() {
                    let history = one(
                        self.store.as_ref(),
                        "workflow_last_outputs",
                        &json!([root.record.id, current.node_id]).to_string(),
                    )?;
                    commit.witness([version(&history)])?;
                    history
                        .value
                        .as_ref()
                        .map(|r| {
                            serde_json::from_value(r["data"]["outputs"].clone())
                                .map_err(|e| err("storage", e.to_string()))
                        })
                        .transpose()?
                        .unwrap_or_default()
                } else {
                    current.outputs.clone()
                };
                next.inputs.clear();
                next.input_digest.clear();
                next.rework = Some(StepReworkContext {
                    previous_execution_id: current.id.clone(),
                    reason: commit.command.payload["reason"]
                        .as_str()
                        .unwrap_or("workflow.step.rework")
                        .into(),
                    outputs,
                });
                next.round += 1;
                next.retry_of = None;
            } else {
                next.inputs = execution.inputs.clone();
                next.input_digest = execution.input_digest.clone();
                next.retry_of = Some(current.id.clone());
            }
            next.failure = None;
            next.agent_revision_id = None;
            next.sequence = 1;
            next.status = StepExecutionStatus::Queued;
            next.session_id = None;
            next.turn_id = None;
            next.tool_call_id = None;
            next.outputs.clear();
            next.started = None;
            next.finished = None;
            next.budget_wait_started = None;
            next.budget_waited_ms = 0;
            next.reason.clear();
            next.runtime_context = Value::Null;
            commit.replace_step(&selected, &next)?;
            if current.id == execution.id {
                result = Some(next);
            }
        }
        if root.instance.status != StepExecutionStatus::Running {
            let mut header = root.instance.as_ref().clone();
            header.status = StepExecutionStatus::Running;
            commit.header(root, &header)?;
        }
        self.reopen_native_ancestors(commit, root)?;
        result.ok_or_else(|| err("storage", "未生成原执行的后继"))
    }
    fn cancel_quiescent_native_children(
        &self,
        commit: &mut WorkflowCommit<'_>,
        execution: &StepExecution,
        depth: usize,
    ) -> Result<()> {
        require(depth <= WORKFLOW_DEPTH_LIMIT, "实际子树深度超过预算")?;
        let selected = one(
            self.store.as_ref(),
            "workflow_children_source",
            &execution.id,
        )?;
        commit.witness([version(&selected)])?;
        if let Some(raw) = &selected.value {
            let ids = raw["data"]["children"]
                .as_array()
                .ok_or_else(|| err("storage", "子流程集合缺失"))?;
            require(ids.len() <= WORKFLOW_STEP_LIMIT, "子流程集合超过预算")?;
            for id in ids {
                let child = WorkflowHeaderFacts::read(
                    self,
                    id.as_str()
                        .ok_or_else(|| err("storage", "子流程身份无效"))?,
                )?;
                for execution in &child.instance.steps {
                    self.cancel_quiescent_native_children(commit, execution, depth + 1)?;
                }
                if !matches!(
                    child.instance.status,
                    StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
                ) {
                    let mut next = child.instance.as_ref().clone();
                    next.cancel_requested = true;
                    next.paused = false;
                    next.status = StepExecutionStatus::Cancelled;
                    commit.header(&child, &next)?;
                } else {
                    commit.witness([version(&child.header)])?;
                }
            }
        }
        Ok(())
    }
    fn native_rework_checkpoint(
        &self,
        commit: &mut WorkflowCommit<'_>,
        resources: &WorkflowResources,
        definition: &StepDefinition,
    ) -> Result<Option<ReworkWorkspaceCheckpoint>> {
        if definition.access.writable_paths.is_empty() {
            return Ok(None);
        }
        let Some(previous) = resources
            .execution
            .rework_workspace
            .as_ref()
            .map(|c| &c.previous_workspace)
            .or_else(|| {
                definition
                    .access
                    .workspace_input
                    .as_ref()
                    .and_then(|n| resources.execution.inputs.get(n))
            })
        else {
            return Ok(None);
        };
        require(
            previous["owner_id"] == resources.instance.owner_id,
            "写入返工检查点必须属于当前工作",
        )?;
        let head = one(self.store.as_ref(), "head", &resources.instance.owner_id)?;
        commit.witness([version(&head)])?;
        let actual = head
            .value
            .as_ref()
            .ok_or_else(|| err("configuration_required", "返工工作空间代码头未登记"))?;
        if previous["revision"] == actual["revision"] {
            return Ok(None);
        }
        let output = one(
            self.store.as_ref(),
            "workflow_snapshot_checkpoint",
            &json!([
                resources.record.id,
                resources.instance.owner_id,
                previous["workspace_id"],
                actual["revision"]
            ])
            .to_string(),
        )?;
        commit.witness([version(&output)])?;
        let row = output
            .value
            .as_ref()
            .ok_or_else(|| err("needs_reconciliation", "当前代码头缺少该实例的真实平台快照"))?;
        let call = one(
            self.store.as_ref(),
            "governance",
            &text(&row["data"]["provenance"], "call_id")?,
        )?;
        commit.witness([version(&call)])?;
        require(
            row["type"] == "workflow_output"
                && row["immutable"] == true
                && row["data"]["provenance"]["kind"] == "platform"
                && row["data"]["provenance"]["tool"] == "workspace.snapshot"
                && row["data"]["provenance"]["pointer"] == ""
                && call.value.as_ref().is_some_and(|r| {
                    r["type"] == "platform_call"
                        && r["data"]["status"] == "succeeded"
                        && r["data"]["request"]["instance_id"] == resources.record.id
                        && r["data"]["receipt"] == row["data"]["value"]
                }),
            "返工检查点缺少原平台执行回执",
        )?;
        Ok(Some(ReworkWorkspaceCheckpoint {
            previous_workspace: previous.clone(),
            output_id: text(row, "id")?,
            workspace: row["data"]["value"].clone(),
        }))
    }
    fn reopen_native_ancestors(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
    ) -> Result<()> {
        let mut parent = root.instance.parent_execution_id.clone();
        let mut depth = 0;
        while let Some(id) = parent {
            depth += 1;
            require(depth <= WORKFLOW_DEPTH_LIMIT, "实际祖先超过深度预算")?;
            let owner = one(self.store.as_ref(), "workflow_execution_owner", &id)?;
            commit.witness([version(&owner)])?;
            let ancestor = WorkflowHeaderFacts::read(
                self,
                &text(
                    owner
                        .value
                        .as_ref()
                        .ok_or_else(|| err("not_found", "原祖先归属缺失"))?,
                    "instance_id",
                )?,
            )?;
            if ancestor.instance.cancel_requested
                || ancestor.instance.status == StepExecutionStatus::Cancelled
            {
                break;
            }
            let resources = ancestor.step(self, &id)?;
            if matches!(
                resources.execution.status,
                StepExecutionStatus::Failed | StepExecutionStatus::NeedsReconciliation
            ) {
                let mut next = resources.execution.clone();
                next.status = StepExecutionStatus::Running;
                next.reason.clear();
                next.failure = None;
                next.finished = None;
                commit.step(&resources, &next)?;
                if ancestor.instance.status != StepExecutionStatus::Running {
                    let mut next = ancestor.instance.as_ref().clone();
                    next.status = StepExecutionStatus::Running;
                    commit.header(&ancestor, &next)?;
                }
            } else {
                commit.witness(resources.reads.versions().iter().cloned())?;
            }
            parent = ancestor.instance.parent_execution_id.clone();
        }
        Ok(())
    }
    pub(crate) fn prepare_native_execution_reconciliation(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
    ) -> Result<StepExecution> {
        require(
            resources.execution.status == StepExecutionStatus::NeedsReconciliation,
            "原执行无需核对",
        )?;
        let previous = one(
            self.store.as_ref(),
            "workflow_reconciliation_active",
            &resources.execution.id,
        )?;
        require(previous.value.is_none(), "原执行已有待处理核对请求")?;
        commit.witness([version(&previous)])?;
        let resolution = commit.command.payload["resolution"]
            .as_str()
            .unwrap_or("inspect");
        require(
            ["inspect", "not_applied", "applied"].contains(&resolution),
            "未知核对处置",
        )?;
        if resolution != "inspect" {
            require(
                matches!(commit.context.actor(), WorkflowActor::Human),
                "副作用人工裁定需要实际 Human 签署",
            )?;
            text(&commit.command.payload, "evidence")?;
        }
        let mut next = resources.execution.clone();
        let id=commit.new_record(record("workflow_reconciliation",next.id.clone(),root.record.scope.clone(),"核对原执行结果".into(),1,None,json!({"status":"prepared","instance_id":root.record.id,"execution_id":next.id,"plan_revision_id":next.plan_revision_id,"input_digest":next.input_digest,"contract_digest":next.contract_digest,"original_step_revision":resources.step.revision,"original_header_version":version(&root.header),"command":commit.command,"actor":commit.context.actor(),"resolution":resolution,"evidence":commit.command.payload["evidence"],"issuer":"kernel_reconciliation"}),false),"workflow-reconciliation")?;
        if !next.runtime_context.is_object() {
            next.runtime_context = json!({});
        }
        next.runtime_context["reconciliation_request_id"] = json!(id);
        commit.step(resources, &next)?;
        Ok(next)
    }
}
