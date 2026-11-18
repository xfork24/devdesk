//! Dispatch prepares one actual execution and its durable runtime commands.
//! Source compilation is configuration-only. Writer preflight only reads Git;
//! external mutations remain in durable runtime commands.
use crate::{
    concurrency::*,
    governance::record,
    material_commands::stable_object_id,
    platform_facts::one,
    work_owners::WorkOwnerFacts,
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_facts::FactSelection,
    workflow_resources::{WorkflowResources, version},
    *,
};
impl WorkflowCommit<'_> {
    pub fn runtime_intent(
        &mut self,
        resources: &WorkflowResources,
        execution: &StepExecution,
        method: &str,
        target: &str,
        expected: Option<u64>,
        payload: Value,
    ) -> Result<Id> {
        if matches!(self.context.actor(), WorkflowActor::Coordinator { .. }) {
            require(
                self.coordinator_origin.is_some() && method == "conversation.send",
                "系统投影只能续派已决定的原步骤输入",
            )?;
            let source = one(
                self.engine.store.as_ref(),
                "workflow_dispatch_source",
                &json!([execution.id, execution.plan_revision_id]).to_string(),
            )?;
            let source_record = source
                .value
                .as_ref()
                .ok_or_else(|| err("permission_denied", "原步骤续派缺少真实派发决定"))?;
            let decision = one(
                self.engine.store.as_ref(),
                "governance",
                &text(&source_record["data"], "decision_id")?,
            )?;
            let decided = ConfigurationRecord::from_storage(
                decision
                    .value
                    .clone()
                    .ok_or_else(|| err("not_found", "原派发决定缺失"))?,
            )?;
            let actual: WorkflowDecision = decided.decode()?;
            require(
                source_record["type"] == "workflow_dispatch"
                    && source_record["immutable"] == true
                    && decided.r#type == "workflow_decision"
                    && decided.immutable
                    && actual.instance_id == resources.record.id
                    && actual.execution_id.as_deref() == Some(execution.id.as_str())
                    && actual.plan_revision_id == execution.plan_revision_id
                    && actual.operation == "dispatch"
                    && !matches!(actual.actor, WorkflowActor::Coordinator { .. }),
                "系统续派不能制造或替换原业务决定",
            )?;
            self.witness([version(&source), version(&decision)])?;
        }
        let (mut authority, guards) = crate::workflow_resources::live_workflow_chain(
            self.engine.store.as_ref(),
            &resources.record.id,
            &resources.execution.id,
            resources.execution.status.clone(),
        )?;
        authority.input_digest = execution.input_digest.clone();
        authority.session_id = execution.session_id.clone();
        authority.plan_revision_id = execution.plan_revision_id.clone();
        authority.contract_digest = execution.contract_digest.clone();
        authority.node_id = execution.node_id.clone();
        self.witness(guards.versions().iter().cloned())?;
        let operation = format!(
            "workflow:{}",
            configuration_hash(&(execution.id.as_str(), execution.sequence, method))
        );
        let previous = one(
            self.engine.store.as_ref(),
            "workflow_command_intent",
            &operation,
        )?;
        require(previous.value.is_none(), "原执行命令意图已经存在")?;
        self.witness([version(&previous)])?;
        let mut row = record(
            "workflow_intent",
            execution.id.clone(),
            resources.record.scope.clone(),
            method.into(),
            1,
            None,
            json!({"status":"prepared","configured":true,"execution_authority":authority,"owner_id":resources.instance.owner_id,"method":method,"command":Command{command_id:operation,target:Some(target.into()),expected_revision:expected,payload}}),
            false,
        );
        row.id = stable_object_id(
            self.engine.store.as_ref(),
            &self.command.command_id,
            &format!("workflow-intent:{method}"),
        )?;
        let key = ResourceKey::new("governance", &row.id);
        self.witness([ResourceVersion {
            resource: key.clone(),
            revision: 0,
        }])?;
        let value = if method == "conversation.send" {
            self.engine.store.encode_workflow_input_intent(&row)?
        } else {
            self.engine.store.encode_runtime_record(&row)?
        };
        self.change(ResourceChange {
            resource: key,
            value,
        })?;
        Ok(row.id)
    }
    pub fn dispatch_step(
        &mut self,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
        reason: &str,
    ) -> Result<StepExecution> {
        let execution = &resources.execution;
        let instance = &resources.instance;
        require(
            execution.status == StepExecutionStatus::Queued,
            "原步骤已派发或已经结束",
        )?;
        let step = instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == execution.step)
            .ok_or_else(|| err("storage", "原派发步骤定义缺失"))?;
        let (owner, capacity) = self.require_dispatch(root, resources)?;
        let values = capacity.value.as_ref().unwrap();
        let mut selection = FactSelection::default();
        selection.rule(&step.condition);
        for binding in step.inputs.values() {
            selection.binding(binding);
        }
        match &step.executor {
            StepExecutor::Tool { arguments, .. } => {
                for binding in arguments.values() {
                    selection.binding(binding);
                }
            }
            StepExecutor::Workflow {
                foreach: Some(collection),
                ..
            } => selection.source(&collection.source),
            _ => {}
        }
        let selected = resources.selected_facts(self.engine.store.as_ref(), &selection)?;
        self.witness(selected.reads.versions().iter().cloned())?;
        let decision = self.decision(root, Some(execution), "dispatch", reason)?;
        self.new_record(
            record(
                "workflow_dispatch",
                execution.id.clone(),
                resources.record.scope.clone(),
                reason.into(),
                1,
                None,
                json!({"plan_revision_id":execution.plan_revision_id,"authorization_plan_revision_id":instance.active_plan_revision_id,"decision_id":decision}),
                true,
            ),
            "workflow-dispatch",
        )?;
        let mut next = execution.clone();
        if !step.condition.evaluate(&selected.facts) {
            next.status = StepExecutionStatus::Skipped;
            self.step(resources, &next)?;
            return Ok(next);
        }
        if next.input_digest.is_empty() {
            next.inputs = crate::configured_runtime::material_inputs(step, &selected.facts)?;
        } else {
            require(
                configuration_hash(&next.inputs) == next.input_digest,
                "技术重试必须保留原冻结输入",
            )?;
        }
        if let Some(name) = &step.access.workspace_input
            && let Some(value) = next.inputs.get(name).cloned()
        {
            next.inputs.insert(
                name.clone(),
                crate::configured_runtime::checkpoint_workspace(&next, &value),
            );
        }
        self.require_writer_workspace(resources, step, &next)?;
        next.input_digest = configuration_hash(&next.inputs);
        next.started = Some(now());
        next.status = StepExecutionStatus::Running;
        self.freeze_native_input_evidence(resources, step, &next)?;
        match &step.executor {
            StepExecutor::Agent { .. } => {
                let catalog = self.engine.store.configuration_snapshot()?;
                let role = crate::configuration::role_record(
                    &catalog.state,
                    instance
                        .plan
                        .document
                        .role(step)
                        .ok_or_else(|| err("configuration_required", "步骤 Role 缺失"))?,
                )?;
                let agent = instance
                    .role_agent_bindings
                    .get(&role.id)
                    .ok_or_else(|| err("configuration_required", "步骤 Agent 未冻结"))?;
                next.agent_revision_id = Some(agent.clone());
                self.runtime_intent(resources,&next,"session.open",&instance.owner_id,Some(owner.owner.revision),json!({"agent":agent,"work_type":"workflow","force_new":true,"configured_execution_id":next.id}))?;
            }
            StepExecutor::Tool { arguments, .. } => {
                let args = crate::configured_runtime::frozen_tool_arguments(
                    &selected.facts,
                    arguments,
                    &next,
                )?;
                self.runtime_intent(resources,&next,"workflow.tool.execute",&instance.owner_id,Some(owner.owner.revision),json!({"execution_id":next.id,"arguments":args,"arguments_digest":configuration_hash(&args)}))?;
            }
            StepExecutor::Control => {
                self.engine.start_native_workflow_wait(
                    self,
                    resources,
                    next.started.unwrap(),
                    "awaiting_acceptance",
                )?;
                next.finished = Some(now());
                next.status = if step.confirmation == ConfirmationPolicy::Human {
                    StepExecutionStatus::WaitingConfirmation
                } else {
                    StepExecutionStatus::AwaitingAcceptance
                };
            }
            StepExecutor::Workflow { .. } => {
                self.dispatch_native_children(
                    resources,
                    step,
                    &next,
                    &selected.facts,
                    values["steps"].as_u64().unwrap_or(u64::MAX) as usize,
                )?;
            }
        }
        self.step(resources, &next)?;
        if root.instance.status == StepExecutionStatus::Queued {
            let mut header = root.instance.as_ref().clone();
            header.status = StepExecutionStatus::Running;
            self.header(root, &header)?;
        }
        Ok(next)
    }
    pub(crate) fn require_writer_workspace(
        &mut self,
        resources: &WorkflowResources,
        step: &StepDefinition,
        execution: &StepExecution,
    ) -> Result<()> {
        if step.access.writable_paths.is_empty()
            || !matches!(step.executor, StepExecutor::Agent { .. })
        {
            return Ok(());
        }
        let (view, reads) =
            self.engine
                .formal_input_code_view(&resources.instance, execution, step)?;
        self.witness(reads.versions().iter().cloned())?;
        if let Some(view) = view {
            self.engine
                .git
                .validate_workspace(&view.context, &view.revision)?;
        }
        Ok(())
    }
    pub fn require_dispatch(
        &mut self,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
    ) -> Result<(WorkOwnerFacts, ResourceSnapshot)> {
        let instance = &resources.instance;
        let step = instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == resources.execution.step)
            .ok_or_else(|| err("storage", "原步骤合同缺失"))?;
        require(
            resources.execution.status == StepExecutionStatus::Queued,
            "原步骤已派发或结束",
        )?;
        let (_, chain) = crate::workflow_resources::live_workflow_chain(
            self.engine.store.as_ref(),
            &root.record.id,
            &resources.execution.id,
            StepExecutionStatus::Queued,
        )?;
        self.witness(chain.versions().iter().cloned())?;
        let owner = WorkOwnerFacts::read(self.engine.store.as_ref(), &instance.owner_id)?;
        owner.require_editable()?;
        self.witness(owner.read_set.versions().iter().cloned())?;
        if owner.owner.resource.kind == "task" {
            let deps: Vec<Id> =
                serde_json::from_value(owner.owner.value.as_ref().unwrap()["dependencies"].clone())
                    .map_err(|e| err("storage", e.to_string()))?;
            require(deps.len() <= WORKFLOW_STEP_LIMIT, "任务依赖超过预算")?;
            for id in deps {
                let actual = one(self.engine.store.as_ref(), "task", &id)?;
                if !actual.value.as_ref().is_some_and(|v| {
                    v["status"] == "completed"
                        && v["feature_id"] == owner.owner.value.as_ref().unwrap()["feature_id"]
                }) {
                    return Err(err("task_dependencies", "Task 依赖尚未完成"));
                }
                self.witness([version(&actual)])?;
            }
        }
        for key in &step.dependencies {
            let dependent = root.step(self.engine, key)?;
            if !matches!(
                dependent.execution.status,
                StepExecutionStatus::Completed | StepExecutionStatus::Skipped
            ) {
                return Err(err("dependencies", "步骤依赖尚未接纳"));
            }
            self.witness(dependent.reads.versions().iter().cloned())?;
        }
        let capacity = one(
            self.engine.store.as_ref(),
            "workflow_capacity",
            &json!([resources.record.id, instance.root_instance_id]).to_string(),
        )?;
        let values = capacity
            .value
            .as_ref()
            .ok_or_else(|| err("storage", "流程并发事实缺失"))?;
        if matches!(
            &step.executor,
            StepExecutor::Agent { .. } | StepExecutor::Tool { .. }
        ) && (values["instance"].as_u64().unwrap_or(u64::MAX)
            >= instance.plan.document.scheduling.max_concurrency as u64
            || values["root"].as_u64().unwrap_or(u64::MAX)
                >= values["root_limit"].as_u64().unwrap_or(0))
        {
            return Err(err("capacity", "流程并发额度已满"));
        }
        self.witness([version(&capacity)])?;
        self.require_child_slot(resources)?;
        Ok((owner, capacity))
    }
    fn require_child_slot(&mut self, resources: &WorkflowResources) -> Result<()> {
        let Some(parent) = &resources.instance.parent_execution_id else {
            return Ok(());
        };
        let selected = one(
            self.engine.store.as_ref(),
            "workflow_children_source",
            parent,
        )?;
        self.witness([version(&selected)])?;
        let row = selected
            .value
            .as_ref()
            .ok_or_else(|| err("invalid_workflow", "子流程缺少实际调用关联"))?;
        let ids = row["data"]["children"]
            .as_array()
            .ok_or_else(|| err("storage", "实际子流程集合无效"))?;
        require(ids.len() <= WORKFLOW_STEP_LIMIT, "子流程集合超过预算")?;
        let mut count = 0;
        for id in ids {
            if id == &resources.record.id {
                continue;
            }
            let header = one(
                self.engine.store.as_ref(),
                "workflow_run_authority",
                id.as_str()
                    .ok_or_else(|| err("storage", "子流程身份无效"))?,
            )?;
            if header.value.as_ref().is_some_and(|r| {
                matches!(
                    r["status"].as_str(),
                    Some("running" | "needs_reconciliation")
                )
            }) {
                count += 1;
            }
            self.witness([version(&header)])?;
        }
        if count >= row["data"]["max_concurrency"].as_u64().unwrap_or(1) {
            return Err(err("foreach_capacity", "子流程并发额度已满"));
        }
        Ok(())
    }
    fn freeze_native_input_evidence(
        &mut self,
        resources: &WorkflowResources,
        step: &StepDefinition,
        execution: &StepExecution,
    ) -> Result<()> {
        let key = ResourceKey::new("governance", format!("workflow-inputs:{}", execution.id));
        let original = one(self.engine.store.as_ref(), "governance", &key.id)?;
        require(original.value.is_none(), "原执行输入证据已经冻结")?;
        self.witness([version(&original)])?;
        let mut targets = vec![];
        for (name, value) in &execution.inputs {
            let origin = match step.inputs.get(name) {
                Some(InputBinding::Source { source }) => resources
                    .instance
                    .steps
                    .iter()
                    .find(|e| e.step == source.step)
                    .and_then(|e| e.outputs.get(&source.port))
                    .cloned(),
                Some(InputBinding::Material { material_ref }) => Some(material_ref.clone()),
                _ => None,
            };
            let artifact = if let Some(id) = &origin {
                let row = one(self.engine.store.as_ref(), "governance", id)?;
                let value = row
                    .value
                    .as_ref()
                    .and_then(|r| r["data"]["artifact_id"].as_str())
                    .map(str::to_owned);
                self.witness([version(&row)])?;
                value
            } else {
                None
            };
            targets.push(json!({"input":name,"source_ref":format!("execution-input:{}:{name}",execution.id),"origin_ref":origin,"artifact_ref":artifact,"value_digest":configuration_hash(value),"binding":step.inputs.get(name)}));
        }
        let mut row = record(
            "workflow_input_manifest",
            execution.id.clone(),
            resources.record.scope.clone(),
            "冻结输入证据".into(),
            1,
            None,
            json!({"execution_id":execution.id,"input_digest":execution.input_digest,"targets":targets}),
            true,
        );
        row.id = key.id;
        self.insert_record(&row)
    }
}
impl Engine {
    pub(crate) fn native_workflow_dispatch_command(
        &self,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Value> {
        let root = WorkflowHeaderFacts::read(
            self,
            c.target
                .as_deref()
                .ok_or_else(|| err("invalid_params", "缺少原流程"))?,
        )?;
        root.active()?;
        let key = c.payload["execution_id"]
            .as_str()
            .or_else(|| c.payload["step"].as_str())
            .or_else(|| c.payload["node_id"].as_str())
            .ok_or_else(|| err("invalid_params", "缺少原派发步骤"))?;
        let resources = root.step(self, key)?;
        root.guard_request(c, Some(&resources.step))?;
        let mut commit = WorkflowCommit::new(self, "workflow.step.dispatch", c, context)?;
        commit.authorize(&root)?;
        commit.require_no_change(&root.record.id)?;
        let next = commit.dispatch_step(
            &root,
            &resources,
            c.payload["reason"]
                .as_str()
                .unwrap_or("workflow.step.dispatch"),
        )?;
        commit.finish(fingerprint,json!({"id":root.record.id,"execution":next,"plan_revision_id":root.instance.active_plan_revision_id,"control_version":root.instance.control_version}))
    }
}
