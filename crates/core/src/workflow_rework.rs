//! Business rework uses immutable previews and a shared logical-work CAS budget.
use crate::{
    concurrency::*,
    governance::record,
    platform_facts::{one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_facts::FactSelection,
    workflow_resources::version,
    workflow_start::queued_execution,
    *,
};
use std::collections::BTreeSet;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReworkSelection {
    policy: String,
    target: ReworkTarget,
    review_instance_id: Id,
    review_execution_id: Id,
    report_output_id: Id,
    #[serde(default)]
    producer_output_id: Option<Id>,
    #[serde(default)]
    checkpoint_ref: Option<Id>,
    #[serde(default)]
    baseline_output_id: Option<Id>,
    assessment: String,
    acceptance_requirements: String,
}

pub(crate) fn budget_id(instance: &ConfiguredWorkflowInstance, policy: &str) -> Id {
    format!(
        "rework-budget:{}",
        configuration_hash(&json!([
            instance.root_instance_id,
            instance.owner_id,
            policy
        ]))
    )
}

impl Engine {
    pub(crate) fn reopen_rework_owner(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
    ) -> Result<()> {
        let owner =
            crate::work_owners::WorkOwnerFacts::read(self.store.as_ref(), &root.instance.owner_id)?;
        commit.witness(owner.read_set.versions().iter().cloned())?;
        if owner.owner.resource.kind == "task"
            && owner
                .owner
                .value
                .as_ref()
                .is_some_and(|v| v["status"] == "completed")
        {
            require(
                owner
                    .parent
                    .as_ref()
                    .is_none_or(|p| p.value.as_ref().is_some_and(|v| v["status"] != "completed")),
                "已完成 Feature 的工作须先显式重新确认范围",
            )?;
            if !commit.contains_change(&owner.owner.resource) {
                let mut next = owner.owner.value.clone().unwrap();
                next["status"] = json!(Status::Running);
                next["revision"] = json!(owner.owner.revision + 1);
                commit.change(ResourceChange {
                    resource: owner.owner.resource,
                    value: next,
                })?;
            }
        }
        Ok(())
    }
    fn policy_repair_steps(root: &WorkflowHeaderFacts, key: &str) -> BTreeSet<String> {
        let doc = &root.instance.plan.document;
        let area = doc.impacted_nodes(&BTreeSet::from([doc.rework_policies[key]
            .entry_step
            .clone()]));
        let review_steps = doc
            .review_policies
            .values()
            .map(|p| p.review_step.as_str())
            .collect::<BTreeSet<_>>();
        doc.review_policies
            .values()
            .filter(|p| p.rework_policy == key)
            .filter_map(|p| doc.steps.iter().find(|s| s.key == p.review_step))
            .flat_map(|s| doc.ancestors(s))
            .filter(|s| area.contains(*s) && !review_steps.contains(*s))
            .map(str::to_owned)
            .collect()
    }
    pub(crate) fn rework_budget_view(
        &self,
        root: &WorkflowHeaderFacts,
        key: &str,
    ) -> Result<(ResourceSnapshot, Value)> {
        let policy = &root.instance.plan.document.rework_policies[key];
        // Stable through execution, child and plan replacement, and step renames.
        let id = budget_id(&root.instance, key);
        let selected = one(self.store.as_ref(), "governance", &id)?;
        let used = selected
            .value
            .as_ref()
            .and_then(|r| r["data"]["used"].as_u64())
            .unwrap_or(0);
        let adopted = selected.value.is_some();
        let revision = selected.revision;
        Ok((
            selected,
            json!({"id":id,"policy":key,"used":used,"limit":policy.max_reworks,
            "remaining":policy.max_reworks.saturating_sub(used),"history_baseline_known":adopted,"revision":revision}),
        ))
    }

    fn rework_budget_scopes(
        &self,
        root: &WorkflowHeaderFacts,
        impacted: &BTreeSet<String>,
    ) -> Result<Vec<(WorkflowHeaderFacts, BTreeSet<String>)>> {
        let mut scopes = Vec::new();
        let mut todo = vec![(root.record.id.clone(), impacted.clone(), 0)];
        let mut seen = BTreeSet::new();
        while let Some((id, affected, depth)) = todo.pop() {
            require(
                depth <= WORKFLOW_DEPTH_LIMIT
                    && seen.insert(id.clone())
                    && seen.len() <= WORKFLOW_STEP_LIMIT,
                "返工影响子树超出预算或有环",
            )?;
            let member = WorkflowHeaderFacts::read(self, &id)?;
            for e in member
                .instance
                .steps
                .iter()
                .filter(|e| affected.contains(&e.step))
            {
                let children = one(self.store.as_ref(), "workflow_children_source", &e.id)?;
                for child in children
                    .value
                    .as_ref()
                    .and_then(|r| r["data"]["children"].as_array())
                    .into_iter()
                    .flatten()
                {
                    let child = WorkflowHeaderFacts::read(
                        self,
                        child
                            .as_str()
                            .ok_or_else(|| err("storage", "子流程身份无效"))?,
                    )?;
                    todo.push((
                        child.record.id.clone(),
                        child
                            .instance
                            .plan
                            .document
                            .steps
                            .iter()
                            .map(|s| s.key.clone())
                            .collect(),
                        depth + 1,
                    ));
                }
            }
            scopes.push((member, affected));
        }
        Ok(scopes)
    }

    pub(crate) fn consume_native_rework_budgets(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        impacted: &BTreeSet<String>,
    ) -> Result<()> {
        let mut charged = BTreeSet::new();
        for (member, affected) in self.rework_budget_scopes(root, impacted)? {
            commit.witness([version(&member.header)])?;
            for (key, policy) in &member.instance.plan.document.rework_policies {
                if Self::policy_repair_steps(&member, key).is_disjoint(&affected)
                    || !member.instance.steps.iter().any(|e| {
                        e.step == policy.entry_step
                            && (e.started.is_some() || e.status != StepExecutionStatus::Queued)
                    })
                {
                    continue;
                }
                let (selected, budget) = self.rework_budget_view(&member, key)?;
                if !charged.insert(selected.resource.id.clone())
                    || commit.contains_change(&selected.resource)
                {
                    continue;
                }
                require(
                    budget["history_baseline_known"] == true,
                    "采用返工策略前需要明确历史额度基准，不能把历史轮次当成零次",
                )?;
                let used = budget["used"].as_u64().unwrap();
                if used >= policy.max_reworks {
                    return Err(err(
                        "rework_budget_exhausted",
                        format!(
                            "业务返工额度已耗尽：used={used}, limit={}, remaining=0",
                            policy.max_reworks
                        ),
                    ));
                }
                let mut data = budget.clone();
                data["used"] = json!(used + 1);
                data["remaining"] = json!(policy.max_reworks - used - 1);
                data["command_id"] = json!(commit.command.command_id);
                if selected.value.is_some() {
                    let row = restored(self, &selected)?;
                    commit.replace_record(&selected, row, data)?;
                } else {
                    let mut row = record(
                        "workflow_rework_budget",
                        member.instance.owner_id.clone(),
                        member.record.scope.clone(),
                        key.clone(),
                        1,
                        None,
                        data,
                        false,
                    );
                    row.id = selected.resource.id.clone();
                    commit.insert_record(&row)?;
                }
                commit.new_record(record("workflow_rework_consumption",selected.resource.id.clone(),member.record.scope.clone(),key.clone(),1,None,
                    json!({"used":used+1,"limit":policy.max_reworks,"instance_id":member.record.id,"plan_revision_id":member.instance.active_plan_revision_id,"command_id":commit.command.command_id,"rework":commit.command.payload["rework"],"reason":commit.command.payload["reason"]}),true), &format!("rework-consumption:{}",selected.resource.id))?;
            }
        }
        Ok(())
    }

    pub(crate) fn preview_native_rework(&self, p: &Value) -> Result<Value> {
        let root = WorkflowHeaderFacts::read(
            self,
            p["instance_id"]
                .as_str()
                .or_else(|| p["id"].as_str())
                .ok_or_else(|| err("invalid_params", "缺少返工实例"))?,
        )?;
        if let Some(project) = p["project_id"].as_str() {
            require(project == root.instance.project_id, "返工跨项目")?;
        }
        let c = Command {
            command_id: "rework-preview".into(),
            target: Some(root.record.id.clone()),
            expected_revision: Some(root.record.revision),
            payload: p.clone(),
        };
        let context = InvocationContext::workflow(
            WorkflowActor::Coordinator {
                source_id: root.record.id.clone(),
            },
            Some(root.instance.project_id.clone()),
        );
        let mut commit = WorkflowCommit::new(self, "workflow.rework.preview", &c, &context)?;
        let (preview, _, _) = self.prepare_rework_preview(&mut commit, &root)?;
        Ok(preview)
    }

    fn prepare_rework_preview(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
    ) -> Result<(Value, ReworkSelection, BTreeSet<String>)> {
        let p = &commit.command.payload;
        require(
            p["plan_revision_id"] == root.instance.active_plan_revision_id,
            "返工预演需要当前计划",
        )?;
        let selection: ReworkSelection = serde_json::from_value(p["rework"].clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        require(
            !selection.assessment.trim().is_empty()
                && !selection.acceptance_requirements.trim().is_empty(),
            "经理需记录根因、影响范围、选择理由及验收要求",
        )?;
        let policy = root
            .instance
            .plan
            .document
            .rework_policies
            .get(&selection.policy)
            .ok_or_else(|| err("invalid_params", "未声明的返工策略"))?;
        require(
            policy.allowed_targets.contains(&selection.target),
            "Workflow 未允许所选返工方式",
        )?;
        let review = WorkflowHeaderFacts::read(self, &selection.review_instance_id)?;
        require(
            review.instance.root_instance_id == root.instance.root_instance_id,
            "评审与返工目标不属于同一流程根",
        )?;
        let resources = review.step(self, &selection.review_execution_id)?;
        let e = &resources.execution;
        require(
            matches!(
                e.status,
                StepExecutionStatus::AwaitingAcceptance | StepExecutionStatus::WaitingConfirmation
            ),
            "评审不在待处置状态",
        )?;
        let review_policy = review
            .instance
            .plan
            .document
            .review_policies
            .values()
            .find(|p| p.review_step == e.step)
            .ok_or_else(|| err("invalid_params", "评审未声明拒绝处理策略"))?;
        if review.record.id == root.record.id {
            require(
                review_policy.rework_policy == selection.policy,
                "评审与返工策略不匹配",
            )?;
        }
        let mut facts = FactSelection::default();
        facts.rule(&review_policy.rejected_when);
        let facts = resources.selected_facts(self.store.as_ref(), &facts)?;
        commit.witness(facts.reads.versions().iter().cloned())?;
        require(
            review_policy.rejected_when.evaluate(&facts.facts),
            "评审未被拒绝",
        )?;
        require(
            e.outputs
                .values()
                .any(|id| id == &selection.report_output_id),
            "拒绝报告不属于本轮评审",
        )?;
        let report = one(
            self.store.as_ref(),
            "governance",
            &selection.report_output_id,
        )?;
        commit.witness([version(&report)])?;
        let report = restored(self, &report)?;
        require(
            report.r#type == "workflow_output"
                && report.immutable
                && report.data["input_digest"] == e.input_digest,
            "拒绝报告来源无效",
        )?;
        let affected = match selection.target {
            ReworkTarget::ProducerStep => {
                let id = selection
                    .producer_output_id
                    .as_deref()
                    .ok_or_else(|| err("invalid_params", "责任步骤返工需要具体文档 output ID"))?;
                let output = one(self.store.as_ref(), "governance", id)?;
                commit.witness([version(&output)])?;
                let output = restored(self, &output)?;
                let producer = root.step(self, &text(&output.data, "execution_id")?)?;
                require(
                    output.r#type == "workflow_output"
                        && output.immutable
                        && producer.execution.outputs.values().any(|o| o == id)
                        && output.data["input_digest"] == producer.execution.input_digest,
                    "文档不是当前责任执行的确切产物",
                )?;
                commit.witness(producer.reads.versions().iter().cloned())?;
                root.instance
                    .plan
                    .document
                    .impacted_nodes(&BTreeSet::from([producer.execution.step.clone()]))
            }
            ReworkTarget::AfterConfirmedCheckpoint => {
                let id = selection
                    .checkpoint_ref
                    .as_deref()
                    .ok_or_else(|| err("invalid_params", "需要实际确认锚点"))?;
                let row = one(self.store.as_ref(), "governance", id)?;
                commit.witness([version(&row)])?;
                let confirmation = restored(self, &row)?;
                let anchor = root.step(self, &text(&confirmation.data, "execution_id")?)?;
                require(
                    confirmation.r#type == "workflow_confirmation"
                        && confirmation.immutable
                        && confirmation.data["actor"]["kind"] == "human"
                        && anchor.execution.status == StepExecutionStatus::Completed
                        && confirmation.data["input_digest"] == anchor.execution.input_digest
                        && confirmation.data["outputs"] == json!(anchor.execution.outputs),
                    "确认锚点已过期或并非真实人工确认",
                )?;
                commit.witness(anchor.reads.versions().iter().cloned())?;
                let mut affected = root
                    .instance
                    .plan
                    .document
                    .impacted_nodes(&BTreeSet::from([anchor.execution.step.clone()]));
                affected.remove(&anchor.execution.step);
                affected
            }
            ReworkTarget::ChildWorkflow => {
                require(
                    root.instance.parent_execution_id.is_some(),
                    "仅允许重建对应子工作流",
                )?;
                let id = selection
                    .baseline_output_id
                    .as_deref()
                    .ok_or_else(|| err("invalid_params", "子流程重执行必须指定封存代码基线"))?;
                let output = one(self.store.as_ref(), "governance", id)?;
                commit.witness([version(&output)])?;
                let output = restored(self, &output)?;
                let workspace = &output.data["value"];
                let call = one(
                    self.store.as_ref(),
                    "governance",
                    &text(&output.data["provenance"], "call_id")?,
                )?;
                commit.witness([version(&call)])?;
                let call = restored(self, &call)?;
                let source =
                    WorkflowHeaderFacts::read(self, &text(&call.data["request"], "instance_id")?)?;
                commit.witness([version(&source.header)])?;
                require(
                    call.r#type == "platform_call"
                        && call.data["status"] == "succeeded"
                        && call.data["receipt"] == *workspace
                        && source.instance.root_instance_id == root.instance.root_instance_id
                        && output.data["provenance"]["pointer"] == "",
                    "代码基线缺少同一根流程的真实平台回执",
                )?;
                let head = one(self.store.as_ref(), "head", &root.instance.owner_id)?;
                commit.witness([version(&head)])?;
                require(
                    output.r#type == "workflow_output"
                        && output.immutable
                        && output.data["provenance"]["kind"] == "platform"
                        && output.data["provenance"]["tool"] == "workspace.snapshot"
                        && workspace["owner_id"] == root.instance.owner_id
                        && head
                            .value
                            .as_ref()
                            .is_some_and(|h| h["revision"] == workspace["revision"]),
                    "代码基线必须已通过真实平台封存并匹配当前受保护代码头",
                )?;
                for step in root
                    .instance
                    .plan
                    .document
                    .steps
                    .iter()
                    .filter(|s| !s.access.writable_paths.is_empty())
                {
                    let selected = root.step(self, &step.key)?;
                    let mut effective = selected.execution.clone();
                    if let Some(name) = &step.access.workspace_input {
                        effective.inputs.insert(name.clone(), workspace.clone());
                    }
                    commit.require_writer_workspace(&selected, step, &effective)?;
                }
                root.instance
                    .plan
                    .document
                    .steps
                    .iter()
                    .map(|s| s.key.clone())
                    .collect()
            }
        };
        require(
            !Self::policy_repair_steps(root, &selection.policy).is_disjoint(&affected),
            "所选范围未覆盖责任入口，不能仅重复评审",
        )?;
        if root.record.id == review.record.id {
            require(affected.contains(&e.step), "返工范围未覆盖拒绝评审")?;
        }
        let mut steps = Vec::new();
        let mut budgets = Vec::new();
        let mut retained = Vec::new();
        let mut scopes = self.rework_budget_scopes(root, &affected)?;
        let mut parent_id = root.instance.parent_execution_id.clone();
        let mut withdrawn_calls = Vec::new();
        for depth in 0..WORKFLOW_DEPTH_LIMIT {
            let Some(id) = parent_id else { break };
            let owner = one(self.store.as_ref(), "workflow_execution_owner", &id)?;
            commit.witness([version(&owner)])?;
            let parent = WorkflowHeaderFacts::read(
                self,
                &text(
                    owner
                        .value
                        .as_ref()
                        .ok_or_else(|| err("storage", "父调用归属缺失"))?,
                    "instance_id",
                )?,
            )?;
            let call = parent.step(self, &id)?;
            commit.witness(call.reads.versions().iter().cloned())?;
            if (depth == 0 && selection.target == ReworkTarget::ChildWorkflow)
                || matches!(
                    call.execution.status,
                    StepExecutionStatus::Completed
                        | StepExecutionStatus::AwaitingAcceptance
                        | StepExecutionStatus::WaitingConfirmation
                )
            {
                withdrawn_calls.push(json!({"instance_id":parent.record.id,"execution_id":id,"outputs":call.execution.outputs,"header_revision":parent.header.revision}));
                let mut downstream = parent
                    .instance
                    .plan
                    .document
                    .impacted_nodes(&BTreeSet::from([call.execution.step.clone()]));
                downstream.remove(&call.execution.step);
                scopes.extend(self.rework_budget_scopes(&parent, &downstream)?);
            }
            parent_id = parent.instance.parent_execution_id.clone();
        }
        let mut instances = Vec::new();
        for (member, scope) in scopes {
            commit.witness([version(&member.header)])?;
            instances.push(json!({"id":member.record.id,"header_revision":member.header.revision}));
            for execution in &member.instance.steps {
                let resources = member.step(self, &execution.id)?;
                commit.witness(resources.reads.versions().iter().cloned())?;
                if scope.contains(&execution.step) {
                    let active = commit.execution_active(&resources)?;
                    steps.push(json!({"instance_id":member.record.id,"execution_id":execution.id,"step_revision":resources.step.revision,"step":execution.step,"outputs":execution.outputs,"must_stop":active,"input_digest":execution.input_digest}));
                } else {
                    retained.push(json!({"instance_id":member.record.id,"execution_id":execution.id,"outputs":execution.outputs}));
                }
            }
            for key in member.instance.plan.document.rework_policies.keys() {
                if !Self::policy_repair_steps(&member, key).is_disjoint(&scope) {
                    let (row, budget) = self.rework_budget_view(&member, key)?;
                    commit.witness([version(&row)])?;
                    budgets.push(budget);
                }
            }
        }
        let mut value = json!({"instance_id":root.record.id,"plan_revision_id":root.instance.active_plan_revision_id,"control_version":root.instance.control_version,
            "rework":selection,"review_input_digest":e.input_digest,"report_output_id":selection.report_output_id,"report_digest":configuration_hash(&report.data),
            "invalidated":steps,"retained":retained,"budgets":budgets,"withdrawn_parent_results":withdrawn_calls,"instances":instances,"parent_execution_id":root.instance.parent_execution_id,"header_revision":root.header.revision});
        value["preview_hash"] = json!(configuration_hash(&value));
        Ok((value, selection, affected))
    }

    pub(crate) fn prepare_selected_rework(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
    ) -> Result<StepExecution> {
        let (preview, selection, affected) = self.prepare_rework_preview(commit, root)?;
        require(
            commit.command.payload["preview_hash"] == preview["preview_hash"],
            "返工预演已过期，请保留原决定并重新预演",
        )?;
        if selection.target == ReworkTarget::ChildWorkflow {
            return self.prepare_child_rework(commit, root, &selection, &affected);
        }
        let key = root
            .instance
            .steps
            .iter()
            .find(|e| affected.contains(&e.step))
            .ok_or_else(|| err("invalid_params", "没有受影响步骤"))?;
        let selected = root.step(self, &key.id)?;
        self.prepare_native_retry_scope(commit, root, &selected, true, Some(&affected))
    }

    fn prepare_child_rework(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        selection: &ReworkSelection,
        affected: &BTreeSet<String>,
    ) -> Result<StepExecution> {
        self.reopen_rework_owner(commit, root)?;
        for e in &root.instance.steps {
            let physical = root
                .instance
                .plan
                .document
                .steps
                .iter()
                .find(|s| s.key == e.step)
                .is_some_and(|s| {
                    matches!(
                        s.executor,
                        StepExecutor::Agent { .. } | StepExecutor::Tool { .. }
                    )
                });
            if physical
                && e.failure
                    .as_ref()
                    .is_some_and(|f| f.effect == ExecutionFailureEffect::Unknown)
            {
                return Err(err(
                    "needs_reconciliation",
                    "子流程仍有未知副作用，必须先核对",
                ));
            }
            let resources = root.step(self, &e.id)?;
            require(
                !commit.execution_active(&resources)?,
                "子流程仍有活动执行者或未知副作用",
            )?;
            self.cancel_quiescent_native_children(commit, e, 0)?;
            commit.new_record(
                record(
                    "workflow_execution_history",
                    e.id.clone(),
                    root.record.scope.clone(),
                    e.step.clone(),
                    1,
                    None,
                    json!(e),
                    true,
                ),
                &format!("workflow-history:{}", e.id),
            )?;
        }
        self.consume_native_rework_budgets(commit, root, affected)?;
        let parent_id = root.instance.parent_execution_id.as_deref().unwrap();
        let owner = one(self.store.as_ref(), "workflow_execution_owner", parent_id)?;
        commit.witness([version(&owner)])?;
        let parent = WorkflowHeaderFacts::read(
            self,
            &text(
                owner
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "父调用缺失"))?,
                "instance_id",
            )?,
        )?;
        let call = parent.step(self, parent_id)?;
        require(
            parent.instance.root_instance_id == root.instance.root_instance_id
                && !parent.instance.cancel_requested,
            "原父调用已失效",
        )?;
        let group = one(self.store.as_ref(), "workflow_children_source", parent_id)?;
        commit.witness([version(&group)])?;
        let original = restored(self, &group)?;
        let mut group_data = original.data.clone();
        let children = group_data["children"]
            .as_array_mut()
            .ok_or_else(|| err("storage", "父调用子集合缺失"))?;
        let index = children
            .iter()
            .position(|v| v == root.record.id.as_str())
            .ok_or_else(|| err("invalid_params", "子实例不属于当前实际调用集合"))?;
        let id = commit.object_id("rework-child-instance")?;
        children[index] = json!(id);
        let mut next = root.instance.as_ref().clone();
        next.control_version = 1;
        next.status = StepExecutionStatus::Queued;
        next.cancel_requested = false;
        next.paused = false;
        let baseline = restored(
            self,
            &one(
                self.store.as_ref(),
                "governance",
                selection.baseline_output_id.as_deref().unwrap(),
            )?,
        )?;
        for value in next.inputs.values_mut() {
            if value["owner_id"] == next.owner_id && value.get("workspace_id").is_some() {
                *value = baseline.data["value"].clone();
            }
        }
        next.steps = next
            .plan
            .document
            .steps
            .iter()
            .map(|s| {
                let old = root
                    .instance
                    .steps
                    .iter()
                    .find(|e| e.step == s.key)
                    .unwrap();
                let mut e = queued_execution(
                    &id,
                    &next.workflow_ref,
                    s,
                    commit.object_id(&format!("rework-child-step:{}", s.node_id))?,
                    old.path.clone(),
                );
                e.round = old.round + 1;
                e.rework = Some(StepReworkContext {
                    previous_execution_id: old.id.clone(),
                    reason: commit.command.payload["reason"]
                        .as_str()
                        .unwrap_or("子流程返工")
                        .into(),
                    outputs: old.outputs.clone(),
                    assessment: json!(selection),
                });
                Ok(e)
            })
            .collect::<Result<Vec<_>>>()?;
        let mut frozen = record(
            "workflow_plan_revision",
            next.root_instance_id.clone(),
            root.record.scope.clone(),
            "子流程返工".into(),
            1,
            None,
            json!({"instance_id":id,"parent_revision_id":root.instance.active_plan_revision_id,"plan":next.plan,"inputs":next.inputs,"role_agent_bindings":next.role_agent_bindings,"owner_id":next.owner_id}),
            true,
        );
        frozen.id = commit.object_id("rework-child-plan")?;
        next.active_plan_revision_id = frozen.id.clone();
        for e in &mut next.steps {
            e.plan_revision_id = frozen.id.clone();
        }
        commit.insert_record(&frozen)?;
        let mut row = record(
            "workflow_instance",
            next.owner_id.clone(),
            root.record.scope.clone(),
            root.record.name.clone(),
            1,
            None,
            json!(next),
            false,
        );
        row.id = id.clone();
        commit.insert_record(&row)?;
        commit.new_record(record("workflow_invocation",parent_id.into(),root.record.scope.clone(),"子流程返工后继".into(),1,None,
            json!({"parent_execution_id":parent_id,"parent_plan_revision_id":call.execution.plan_revision_id,"child_instance_id":id,"initial_plan_revision_id":next.active_plan_revision_id,"owner_id":next.owner_id,"member_identity":next.owner_id,"predecessor_instance_id":root.record.id,"input_digest":configuration_hash(&next.inputs),"baseline_output_id":selection.baseline_output_id}),true),"rework-child-invocation")?;
        commit.new_record(
            record(
                "workflow_children",
                parent_id.into(),
                original.scope.clone(),
                original.name.clone(),
                1,
                None,
                group_data,
                true,
            ),
            "rework-child-group",
        )?;
        let mut cancelled = root.instance.as_ref().clone();
        cancelled.status = StepExecutionStatus::Cancelled;
        cancelled.cancel_requested = true;
        commit.header(root, &cancelled)?;
        self.invalidate_rework_ancestors(commit, &parent, &call.execution)?;
        Ok(next.steps[0].clone())
    }

    pub(crate) fn invalidate_rework_ancestors(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        call: &StepExecution,
    ) -> Result<()> {
        let mut affected = root
            .instance
            .plan
            .document
            .impacted_nodes(&BTreeSet::from([call.step.clone()]));
        affected.remove(&call.step);
        // Parent result no longer describes its current children. Keep the call
        // identity (and unaffected siblings) while withdrawing its old outputs.
        let resources = root.step(self, &call.id)?;
        commit.new_record(
            record(
                "workflow_parent_result_withdrawal",
                call.id.clone(),
                root.record.scope.clone(),
                "子结果返工，撤回原父调用结果".into(),
                1,
                None,
                json!({"execution":call,"reason":commit.command.payload["reason"]}),
                true,
            ),
            &format!("rework-parent-result:{}", call.id),
        )?;
        let mut next = call.clone();
        next.status = StepExecutionStatus::Running;
        next.outputs.clear();
        next.input_digest = configuration_hash(&next.inputs);
        next.finished = None;
        next.failure = None;
        if json!(next) != json!(call) {
            commit.step(&resources, &next)?;
        } else {
            commit.witness(resources.reads.versions().iter().cloned())?;
        }
        if let Some(e) = root
            .instance
            .steps
            .iter()
            .find(|e| affected.contains(&e.step))
        {
            let selected = root.step(self, &e.id)?;
            self.prepare_native_retry_scope(commit, root, &selected, true, Some(&affected))?;
        } else if root.instance.status != StepExecutionStatus::Running {
            let mut next = root.instance.as_ref().clone();
            next.status = StepExecutionStatus::Running;
            commit.header(root, &next)?;
            self.reopen_native_ancestors(commit, root)?;
        }
        Ok(())
    }
}

impl WorkflowCommit<'_> {
    pub(crate) fn initialize_rework_budgets(
        &mut self,
        instance: &ConfiguredWorkflowInstance,
        scope: &Scope,
    ) -> Result<()> {
        for key in instance.plan.document.rework_policies.keys() {
            let id = budget_id(instance, key);
            let resource = ResourceKey::new("governance", &id);
            if self.contains_change(&resource) {
                continue;
            }
            let selected = one(self.engine.store.as_ref(), "governance", &id)?;
            self.witness([version(&selected)])?;
            if selected.value.is_none() {
                let mut row = record(
                    "workflow_rework_budget",
                    instance.owner_id.clone(),
                    scope.clone(),
                    key.clone(),
                    1,
                    None,
                    json!({"used":0,"history_baseline_known":true,"initial_plan_revision_id":instance.active_plan_revision_id}),
                    false,
                );
                row.id = id;
                self.insert_record(&row)?;
            }
        }
        Ok(())
    }
}
