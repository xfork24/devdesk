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
    pub(crate) fn rust_retry_count(
        &self,
        resources: &WorkflowResources,
    ) -> Result<(crate::concurrency::ResourceSnapshot, u64)> {
        let record = one(
            self.store.as_ref(),
            "workflow_rust_retries",
            &json!([
                resources.instance.project_id,
                resources.record.id,
                resources.execution.node_id
            ])
            .to_string(),
        )?;
        let count = record
            .value
            .as_ref()
            .and_then(|v| v["count"].as_u64())
            .unwrap_or(0);
        Ok((record, count))
    }

    pub(crate) fn logical_retries(
        &self,
        resources: &WorkflowResources,
    ) -> Result<(crate::concurrency::ResourceSnapshot, u64)> {
        let execution = &resources.execution;
        let attempts = one(
            self.store.as_ref(),
            "workflow_logical_retries",
            &json!([
                resources.instance.project_id,
                resources.record.id,
                execution.node_id,
                execution.id
            ])
            .to_string(),
        )?;
        let used = attempts
            .value
            .as_ref()
            .and_then(|v| v["count"].as_u64())
            .unwrap_or(0);
        Ok((attempts, used))
    }

    pub(crate) fn require_logical_retry_budget(
        &self,
        commit: &mut WorkflowCommit<'_>,
        resources: &WorkflowResources,
        definition: &StepDefinition,
    ) -> Result<crate::concurrency::ResourceSnapshot> {
        let (attempts, used) = self.logical_retries(resources)?;
        let started = attempts
            .value
            .as_ref()
            .and_then(|v| v["started"].as_u64())
            .unwrap_or(0);
        if started != 0 && used >= definition.budget.retry_limit {
            return Err(err("retry_budget_exhausted", "步骤实际重试额度已用尽"));
        }
        commit.witness([version(&attempts)])?;
        Ok(attempts)
    }

    pub(crate) fn prepare_native_retry(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
        rework: bool,
    ) -> Result<StepExecution> {
        if !rework
            && resources
                .execution
                .failure
                .as_ref()
                .is_some_and(|f| f.code == "stale_target" && f.automatic_retry_safe())
        {
            return self.prepare_delivery_refresh(commit, root, resources);
        }
        if !rework && matches!(commit.context.actor(), WorkflowActor::Driver { .. }) {
            let (attempts, count) = self.rust_retry_count(resources)?;
            if count >= root.instance.plan.document.scheduling.max_retries {
                return Err(err("retry_budget_exhausted", "Rust 自动调度重试额度已用尽"));
            }
            commit.witness([version(&attempts)])?;
        }
        self.prepare_native_retry_scope(commit, root, resources, rework, None)
    }
    fn prepare_delivery_refresh(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        failed: &WorkflowResources,
    ) -> Result<StepExecution> {
        let document = &root.instance.plan.document;
        let candidate_regression =
            document.require_delivery_evidence_step("candidate_regression")?;
        let source_publication = document.require_delivery_evidence_step("source_publication")?;
        let source_regression = document.require_delivery_evidence_step("source_regression")?;
        let review = document.require_delivery_evidence_step("review")?;
        require(
            matches!(
                failed.execution.status,
                StepExecutionStatus::Failed | StepExecutionStatus::Cancelled
            ) && !commit.execution_active(failed)?,
            "目标刷新必须先停止并核对原操作",
        )?;
        self.release_failed_delivery_fence(commit, failed)?;
        let regression = document
            .steps
            .iter()
            .find(|s| s.key == candidate_regression)
            .ok_or_else(|| err("storage", "合并回归步骤缺失"))?;
        let StepExecutor::Tool { arguments, .. } = &regression.executor else {
            return Err(err("configuration_required", "合并回归必须为平台工具"));
        };
        let Some(InputBinding::Source { source }) = arguments.get("workspace") else {
            return Err(err("configuration_required", "合并回归缺少确切 PR 来源"));
        };
        let publication = document
            .steps
            .iter()
            .find(|s| s.key == source.step)
            .ok_or_else(|| err("storage", "PR 步骤缺失"))?;
        let StepExecutor::Tool {
            tool_ref,
            arguments,
            ..
        } = &publication.executor
        else {
            return Err(err("configuration_required", "PR 创建必须为平台工具"));
        };
        require(
            root.instance
                .plan
                .definitions
                .get(tool_ref)
                .is_some_and(|r| r.data["qualified_name"] == "git.delivery.open"),
            "刷新不能改变发布合同",
        )?;
        let Some(InputBinding::Source { source }) = arguments.get("target") else {
            return Err(err("configuration_required", "PR 缺少目标读取来源"));
        };
        let target = root.step(self, &source.step)?;
        let affected = document.impacted_nodes(&BTreeSet::from([target.execution.step.clone()]));
        require(
            affected.contains(&failed.execution.step)
                && !affected.contains(review)
                && !affected.contains(source_regression)
                && affected.iter().all(|key| {
                    document.steps.iter().any(|s| {
                        &s.key == key
                            && !matches!(
                                s.executor,
                                StepExecutor::Agent { .. } | StepExecutor::Workflow { .. }
                            )
                    })
                }),
            "目标刷新只能重做后续平台验证及人工审核",
        )?;
        let state = one(
            self.store.as_ref(),
            "governance",
            &format!("workflow-delivery:{}", root.record.id),
        )?;
        let published = root.step(self, source_publication)?;
        require(
            published.execution.status == StepExecutionStatus::Completed,
            "目标刷新必须保留已接纳的源发布",
        )?;
        let reviewed = root.step(self, review)?;
        require(
            reviewed.execution.status == StepExecutionStatus::Completed,
            "目标刷新必须保留已接纳的代码评审",
        )?;
        let source_revision = text(&reviewed.execution.inputs["workspace"], "revision")?;
        if state.value.is_some() {
            let saved = crate::platform_facts::restored(self, &state)?;
            require(saved.data["synchronized"] != true, "已完成合入不能重新刷新")?;
        }
        commit.witness([
            version(&state),
            version(&published.step),
            version(&reviewed.step),
        ])?;
        let key = format!("delivery-refresh:{}:{source_revision}", root.record.id);
        let budget = one(self.store.as_ref(), "governance", &key)?;
        commit.witness([version(&budget)])?;
        let used = budget
            .value
            .as_ref()
            .and_then(|r| r["data"]["used"].as_u64())
            .unwrap_or(0);
        let definition = document
            .steps
            .iter()
            .find(|s| s.key == failed.execution.step)
            .ok_or_else(|| err("storage", "目标刷新步骤合同缺失"))?;
        self.require_logical_retry_budget(commit, failed, definition)?;
        require(
            used < definition.budget.retry_limit,
            "当前源版本的目标刷新额度已耗尽",
        )?;
        let mut row = if budget.value.is_some() {
            crate::platform_facts::restored(self, &budget)?
        } else {
            let mut r = record(
                "workflow_delivery_refresh_budget",
                root.record.id.clone(),
                root.record.scope.clone(),
                "目标版本刷新额度".into(),
                1,
                None,
                json!({}),
                false,
            );
            r.id = key;
            r
        };
        let data = json!({"used":used+1,"source_revision":source_revision,"failed_execution":failed.execution.id});
        if budget.value.is_some() {
            crate::governance::update_record(&mut row, data);
        } else {
            row.data = data;
            row.content_hash = content_digest(&row.refs, &row.data);
        }
        commit.change(crate::concurrency::ResourceChange {
            resource: budget.resource,
            value: row.to_storage(),
        })?;
        self.prepare_native_retry_scope(commit, root, &target, true, Some(&affected))
    }
    pub(crate) fn prepare_native_retry_scope(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
        rework: bool,
        scope: Option<&BTreeSet<String>>,
    ) -> Result<StepExecution> {
        self.prepare_native_retry_with_assessment(commit, root, resources, rework, scope, None)
    }

    pub(crate) fn prepare_native_retry_with_assessment(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
        rework: bool,
        scope: Option<&BTreeSet<String>>,
        assessment: Option<&Value>,
    ) -> Result<StepExecution> {
        let execution = &resources.execution;
        require(
            !rework || scope.is_some() || root.instance.plan.document.rework_policies.is_empty(),
            "此 Workflow 的业务返工需要经理选择方式、评审依据及确切预演",
        )?;
        self.require_execution_time(resources)?;
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
            let group = one(
                self.store.as_ref(),
                "workflow_children_source",
                &execution.id,
            )?;
            commit.witness([version(&group)])?;
            for id in group
                .value
                .as_ref()
                .and_then(|r| r["data"]["children"].as_array())
                .into_iter()
                .flatten()
            {
                let child = WorkflowHeaderFacts::read(
                    self,
                    id.as_str()
                        .ok_or_else(|| err("storage", "子流程身份无效"))?,
                )?;
                commit.witness([version(&child.header)])?;
                require(
                    child.instance.plan.document.rework_policies.is_empty()
                        || !child.instance.steps.iter().any(|e| e.started.is_some()),
                    "已开始的子流程不能通过技术重试重建；请重试原子执行或使用共享额度的业务返工",
                )?;
            }
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
            self.require_logical_retry_budget(commit, resources, definition)?;
        }
        let impacted = if let Some(scope) = scope {
            scope.clone()
        } else if rework {
            root.instance
                .plan
                .document
                .impacted_nodes(&BTreeSet::from([execution.step.clone()]))
        } else {
            BTreeSet::new()
        };
        if rework {
            self.reopen_rework_owner(commit, root)?;
            if assessment.is_none() && commit.method != "workflow.delivery.republish" {
                self.consume_native_rework_budgets(commit, root, &impacted)?;
            }
        }
        let checkpoint = if rework {
            self.native_rework_checkpoint(commit, resources, definition)?
        } else {
            execution.rework_workspace.clone()
        };
        // Refuse a new attempt before archiving executions or consuming retries.
        // A genuine sealed checkpoint is the next writer's effective base.
        if let Some(name) = &definition.access.workspace_input
            && let Some(workspace) = checkpoint
                .as_ref()
                .map(|c| &c.workspace)
                .or_else(|| execution.inputs.get(name))
        {
            let mut effective = execution.clone();
            effective.inputs.insert(name.clone(), workspace.clone());
            commit.require_writer_workspace(resources, definition, &effective)?;
        }
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
            let physical = root
                .instance
                .plan
                .document
                .steps
                .iter()
                .find(|s| s.key == current.step)
                .is_some_and(|s| {
                    matches!(
                        s.executor,
                        StepExecutor::Agent { .. } | StepExecutor::Tool { .. }
                    )
                });
            if physical
                && current
                    .failure
                    .as_ref()
                    .is_some_and(|f| f.effect == ExecutionFailureEffect::Unknown)
            {
                return Err(err(
                    "needs_reconciliation",
                    "返工不能绕过未知副作用，必须先核对原执行",
                ));
            }
            let (attempts, _) = self.logical_retries(&selected)?;
            commit.witness([version(&attempts)])?;
            let mut history = record(
                "workflow_execution_history",
                current.id.clone(),
                root.record.scope.clone(),
                current.step.clone(),
                1,
                None,
                json!(current),
                true,
            );
            history.refs.insert(
                "physically_started".into(),
                json!(
                    attempts
                        .value
                        .as_ref()
                        .is_some_and(|v| v["current_started"] == true)
                ),
            );
            history.content_hash = content_digest(&history.refs, &history.data);
            commit.new_record(history, &format!("workflow-history:{}", current.id))?;
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
                    assessment: if let Some(assessment) = assessment {
                        if current.id == execution.id {
                            assessment.clone()
                        } else {
                            Value::Null
                        }
                    } else {
                        let mut assessment = commit.command.payload["rework"].clone();
                        if let Some(history) = &commit.review_repair_history {
                            assessment["repair_history"] = history.clone();
                        }
                        assessment
                    },
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
                if let Some(failure) = &current.failure
                    && failure.code == "needs_manager"
                {
                    let reason = commit.command.payload["reason"].as_str().unwrap_or("");
                    require(reason.chars().count() <= 4096, "经理重试指令超出预算")?;
                    let mut directive = record(
                        "workflow_manager_retry",
                        next.id.clone(),
                        root.record.scope.clone(),
                        "经理重试指令".into(),
                        1,
                        None,
                        json!({"source_execution_id":current.id,"diagnosis_source_ref":failure.source_ref,
                            "diagnosis":failure.message.chars().take(4096).collect::<String>(),"reason":reason}),
                        true,
                    );
                    directive.id = format!("workflow-manager-retry:{}", next.id);
                    commit.insert_record(&directive)?;
                }
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
            if current.id == execution.id
                && (assessment.is_some()
                    || (!rework
                        && (current.runtime_context["repair_protocol"] == 1
                            || matches!(definition.executor, StepExecutor::Tool { .. })
                            || current
                                .failure
                                .as_ref()
                                .is_some_and(|failure| failure.code == "needs_manager"))))
            {
                let reason = commit.command.payload["reason"]
                    .as_str()
                    .filter(|reason| !reason.trim().is_empty());
                if !rework && matches!(commit.context.actor(), WorkflowActor::Controller { .. }) {
                    require(reason.is_some(), "经理重试需要基于原诊断的具体修复动作")?;
                }
                let default = assessment
                    .and_then(|value| value["instructions"].as_str())
                    .unwrap_or_else(|| {
                        current
                            .failure
                            .as_ref()
                            .map(|failure| failure.message.as_str())
                            .unwrap_or("按原执行证据重跑")
                    });
                let action: String = reason.unwrap_or(default).chars().take(2048).collect();
                let phase = if matches!(commit.context.actor(), WorkflowActor::Driver { .. }) {
                    "driver"
                } else {
                    "manager"
                };
                self.prepare_workflow_repair_attempt(commit, &selected, &next, &action, phase)?;
            }
            commit.replace_step(&selected, &mut next)?;
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
    pub(crate) fn cancel_quiescent_native_children(
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
                self.reopen_rework_owner(commit, &child)?;
                for execution in &child.instance.steps {
                    let physical = child
                        .instance
                        .plan
                        .document
                        .steps
                        .iter()
                        .find(|s| s.key == execution.step)
                        .is_some_and(|s| {
                            matches!(
                                s.executor,
                                StepExecutor::Agent { .. } | StepExecutor::Tool { .. }
                            )
                        });
                    if physical
                        && execution
                            .failure
                            .as_ref()
                            .is_some_and(|f| f.effect == ExecutionFailureEffect::Unknown)
                    {
                        return Err(err(
                            "needs_reconciliation",
                            "子流程仍有未知副作用，不能取消并返工",
                        ));
                    }
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
        require(output.value.is_some(), "当前代码头缺少该实例的真实平台快照")?;
        let row = crate::platform_facts::restored(self, &output)?;
        let call = one(
            self.store.as_ref(),
            "governance",
            &text(&row.data["provenance"], "call_id")?,
        )?;
        commit.witness([version(&call)])?;
        let call = crate::platform_facts::restored(self, &call)?;
        require(
            row.r#type == "workflow_output"
                && row.immutable
                && row.data["provenance"]["kind"] == "platform"
                && [json!("workspace.snapshot"), json!("workspace.checkpoint")]
                    .contains(&row.data["provenance"]["tool"])
                && row.data["provenance"]["pointer"] == ""
                && call.r#type == "platform_call"
                && call.data["status"] == "succeeded"
                && call.data["request"]["instance_id"] == resources.record.id
                && call.data["receipt"] == row.data["value"],
            "返工检查点缺少原平台执行回执",
        )?;
        Ok(Some(ReworkWorkspaceCheckpoint {
            previous_workspace: previous.clone(),
            output_id: row.id,
            workspace: row.data["value"].clone(),
        }))
    }
    pub(crate) fn reopen_native_ancestors(
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
                StepExecutionStatus::Completed
                    | StepExecutionStatus::AwaitingAcceptance
                    | StepExecutionStatus::WaitingConfirmation
            ) {
                self.invalidate_rework_ancestors(commit, &ancestor, &resources.execution)?;
                return Ok(());
            }
            if matches!(
                resources.execution.status,
                StepExecutionStatus::Failed | StepExecutionStatus::NeedsReconciliation
            ) {
                let mut next = resources.execution.clone();
                next.status = StepExecutionStatus::Running;
                next.reason.clear();
                next.failure = None;
                next.finished = None;
                commit.step(&resources, &mut next)?;
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
        let execution = &resources.execution;
        let recheck_output = matches!(
            commit.method,
            "workflow.step.reconcile" | "workflow.execution.reconcile"
        ) && commit.command.payload["resolution"]
            .as_str()
            .unwrap_or("inspect")
            == "inspect"
            && commit.command.payload["execution_id"] == execution.id
            && !root.instance.cancel_requested
            && execution.status == StepExecutionStatus::Failed
            && execution.session_id.is_some()
            && execution.outputs.is_empty()
            && execution.failure.as_ref().is_some_and(|failure| {
                failure.code == "output_invalid"
                    && failure.effect == ExecutionFailureEffect::Applied
                    && execution.turn_id.as_ref() == Some(&failure.source_ref)
            });
        require(
            execution.status == StepExecutionStatus::NeedsReconciliation || recheck_output,
            "原执行无需核对",
        )?;
        let previous = one(
            self.store.as_ref(),
            "workflow_reconciliation_active",
            &resources.execution.id,
        )?;
        require(previous.value.is_none(), "原执行已有待处理核对请求")?;
        commit.witness([version(&previous)])?;
        let source_recovery = matches!(
            commit.method,
            "workflow.step.rework" | "workflow.step.retry"
        );
        let resolution = if source_recovery {
            require(
                commit.command.payload["execution_id"] == resources.execution.id,
                "恢复原源分支必须绑定确切执行",
            )?;
            let (_, basis) = self
                .source_publication_rework_target(root, resources)?
                .ok_or_else(|| err("needs_reconciliation", "该步骤必须先取得原执行的可核对结果"))?;
            commit.witness(basis.versions().iter().cloned())?;
            if commit.method == "workflow.step.retry" {
                require(
                    matches!(commit.context.actor(), WorkflowActor::Controller { .. }),
                    "源分支技术重试由项目经理执行",
                )?;
                self.require_source_publication_retry(commit, root, resources)?;
                crate::delivery_recovery::SOURCE_PUBLICATION_RETRY
            } else {
                require(
                    matches!(commit.context.actor(), WorkflowActor::Human),
                    "源分支人工返工需要实际用户确认",
                )?;
                crate::delivery_recovery::SOURCE_PUBLICATION_REWORK
            }
        } else {
            commit.command.payload["resolution"]
                .as_str()
                .unwrap_or("inspect")
        };
        require(
            source_recovery || ["inspect", "not_applied", "applied"].contains(&resolution),
            "未知核对处置",
        )?;
        if resolution != "inspect" && !source_recovery {
            require(
                matches!(commit.context.actor(), WorkflowActor::Human),
                "副作用人工裁定需要实际 Human 签署",
            )?;
            text(&commit.command.payload, "evidence")?;
        }
        let mut next = resources.execution.clone();
        let id=commit.new_record(record("workflow_reconciliation",next.id.clone(),root.record.scope.clone(),"核对原执行结果".into(),1,None,json!({"status":"prepared","instance_id":root.record.id,"node_id":next.node_id,"execution_id":next.id,"plan_revision_id":next.plan_revision_id,"input_digest":next.input_digest,"contract_digest":next.contract_digest,"original_step_revision":resources.step.revision,"original_header_version":version(&root.header),"command":commit.command,"actor":commit.context.actor(),"resolution":resolution,"evidence":commit.command.payload["evidence"],"issuer":"kernel_reconciliation"}),false),"workflow-reconciliation")?;
        if !next.runtime_context.is_object() {
            next.runtime_context = json!({});
        }
        next.runtime_context["reconciliation_request_id"] = json!(id);
        commit.step(resources, &mut next)?;
        Ok(next)
    }
}
