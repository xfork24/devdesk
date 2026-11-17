//! Execution controls revoke original grants and retain the exact old attempt.
//! Requesting stop never manufactures a physical stop or remote-effect proof.
use crate::{
    concurrency::*,
    governance::record,
    platform_facts::{one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_resources::{WorkflowResources, version},
    *,
};
impl WorkflowCommit<'_> {
    pub fn request_execution_stop(
        &mut self,
        resources: &WorkflowResources,
        reason: &str,
    ) -> Result<()> {
        let execution = &resources.execution;
        require(
            !matches!(
                execution.status,
                StepExecutionStatus::Completed
                    | StepExecutionStatus::Skipped
                    | StepExecutionStatus::Cancelled
            ),
            "原执行已结束",
        )?;
        let marker = one(
            self.engine.store.as_ref(),
            "workflow_stop_source",
            &execution.id,
        )?;
        self.witness([version(&marker)])?;
        if marker.value.is_none() {
            self.new_record(record("workflow_stop",execution.id.clone(),resources.record.scope.clone(),reason.into(),1,None,json!({"actor":if self.coordinator_origin.is_none(){Some(self.context.actor())}else{None},"origin":self.coordinator_origin,"reason":reason}),true),&format!("workflow-stop:{}",execution.id))?;
        }
        if let Some(session) = &execution.session_id {
            let turns = one(self.engine.store.as_ref(), "session_active_turns", session)?;
            self.witness([version(&turns)])?;
            for raw in turns
                .value
                .as_ref()
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let key = ResourceKey::new("turn", text(raw, "id")?);
                let revision = raw["revision"]
                    .as_u64()
                    .ok_or_else(|| err("storage", "原活动轮次版本缺失"))?;
                require(
                    raw["session_id"] == *session
                        && raw["project_id"] == resources.instance.project_id,
                    "原活动轮次不属于执行",
                )?;
                if raw["cancel_requested"] == true {
                    self.witness([ResourceVersion {
                        resource: key,
                        revision,
                    }])?;
                    continue;
                }
                let mut next = raw.clone();
                next["cancel_requested"] = json!(true);
                next["revision"] = json!(revision + 1);
                if raw["status"] == "queued" {
                    next["status"] = json!("cancelled");
                    next["finished"] = json!(now());
                }
                self.witness([ResourceVersion {
                    resource: key.clone(),
                    revision,
                }])?;
                self.change(ResourceChange {
                    resource: key,
                    value: next,
                })?;
            }
        }
        let calls = one(
            self.engine.store.as_ref(),
            "step_platform_calls",
            &execution.id,
        )?;
        self.witness([version(&calls)])?;
        let mut ids = calls
            .value
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|r| r["id"].as_str())
            .map(str::to_owned)
            .collect::<std::collections::BTreeSet<_>>();
        if let Some(call) = &execution.tool_call_id {
            ids.insert(call.clone());
        }
        for id in ids {
            let original = one(self.engine.store.as_ref(), "governance", &id)?;
            self.witness([version(&original)])?;
            let Some(raw) = &original.value else {
                continue;
            };
            if !matches!(
                raw["data"]["status"].as_str(),
                Some("succeeded" | "failed" | "cancelled")
            ) && raw["data"]["cancel_requested"] != true
            {
                let row = restored(self.engine, &original)?;
                require(
                    matches!(row.r#type.as_str(), "platform_call" | "tool_call"),
                    "执行工具关联类型不符",
                )?;
                let mut data = row.data.clone();
                data["cancel_requested"] = json!(true);
                self.replace_record(&original, row, data)?;
            }
        }
        Ok(())
    }
    pub fn stop_execution(
        &mut self,
        resources: &WorkflowResources,
        reason: &str,
    ) -> Result<StepExecution> {
        self.request_execution_stop(resources, reason)?;
        let execution = &resources.execution;
        let active = self.execution_active(resources)?;
        let mut next = execution.clone();
        if !active
            && execution.status != StepExecutionStatus::NeedsReconciliation
            && !execution
                .failure
                .as_ref()
                .is_some_and(|f| f.effect == ExecutionFailureEffect::Unknown)
        {
            next.status = StepExecutionStatus::Cancelled;
            next.finished = Some(now());
            self.step(resources, &next)?;
        } else {
            self.witness(resources.reads.versions().iter().cloned())?;
        }
        Ok(next)
    }
}
impl Engine {
    pub(crate) fn native_workflow_execution_control(
        &self,
        method: &str,
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
        require(
            !matches!(
                root.instance.status,
                StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
            ),
            "流程终态不可重新控制",
        )?;
        let key = c.payload["execution_id"]
            .as_str()
            .or_else(|| c.payload["step"].as_str())
            .or_else(|| c.payload["node_id"].as_str())
            .ok_or_else(|| err("invalid_params", "缺少原执行"))?;
        let resources = root.step(self, key)?;
        root.guard_request(c, Some(&resources.step))?;
        let mut commit = WorkflowCommit::new(self, method, c, context)?;
        commit.authorize(&root)?;
        if ![
            "workflow.execution.stop",
            "workflow.step.reconcile",
            "workflow.execution.reconcile",
        ]
        .contains(&method)
        {
            root.active()?;
            commit.require_no_change(&root.record.id)?;
        }
        let reason = c.payload["reason"].as_str().unwrap_or(method);
        let next = match method {
            "workflow.execution.stop" => commit.stop_execution(&resources, reason)?,
            "workflow.step.input" => {
                let step = resources
                    .instance
                    .plan
                    .document
                    .steps
                    .iter()
                    .find(|s| s.key == resources.execution.step)
                    .ok_or_else(|| err("storage", "输入步骤定义缺失"))?;
                require(
                    resources.execution.status == StepExecutionStatus::WaitingInput
                        && resources.execution.sequence < step.budget.max_turns,
                    "原步骤不在等待输入或已用尽预算",
                )?;
                require(
                    !commit.execution_active(&resources)?,
                    "原输入执行者或工具副作用尚未核对",
                )?;
                let input = text(&c.payload, "input")?;
                require(input.len() <= 65536, "补充输入超出预算")?;
                let (wait_reads, wait_changes) =
                    self.resume_native_workflow_wait(&resources.execution)?;
                commit.witness(wait_reads.versions().iter().cloned())?;
                for change in wait_changes {
                    commit.change(change)?;
                }
                let mut next = resources.execution.clone();
                next.sequence += 1;
                next.turn_id = None;
                next.status = StepExecutionStatus::Running;
                next.budget_wait_started = None;
                let rendered = render_source_task(&resources.instance, step, &next, Some(&input))?;
                let session = one(
                    self.store.as_ref(),
                    "session",
                    next.session_id
                        .as_deref()
                        .ok_or_else(|| err("not_found", "原步骤会话缺失"))?,
                )?;
                commit.witness([version(&session)])?;
                commit.runtime_intent(&resources,&next,"conversation.send",next.session_id.as_deref().unwrap(),Some(session.revision),json!({"client_message_id":format!("{}:{}",next.id,next.sequence),"input":rendered}))?;
                commit.step(&resources, &next)?;
                next
            }
            "workflow.step.retry" | "workflow.step.rework" => self.prepare_native_retry(
                &mut commit,
                &root,
                &resources,
                method == "workflow.step.rework",
            )?,
            "workflow.step.reconcile" | "workflow.execution.reconcile" => {
                self.prepare_native_execution_reconciliation(&mut commit, &root, &resources)?
            }
            _ => return Err(err("method_not_found", "未知执行控制")),
        };
        commit.decision(
            &root,
            Some(&resources.execution),
            method.rsplit('.').next().unwrap_or(method),
            reason,
        )?;
        commit.finish(fingerprint,json!({"id":root.record.id,"accepted":true,"execution":next,"plan_revision_id":root.instance.active_plan_revision_id,"control_version":root.instance.control_version}))
    }
}
