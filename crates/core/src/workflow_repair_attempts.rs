use crate::{
    governance::record,
    platform_facts::one,
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_resources::{WorkflowResources, version},
    *,
};

pub(crate) fn repair_attempt_id(execution: &StepExecution) -> String {
    format!("workflow-repair:{}:{}", execution.id, execution.sequence)
}

pub(crate) const REPAIR_INPUT_PREFIX: &str = "devdesk-repair:";

pub(crate) fn action_digest(input_digest: &str, action: &str) -> String {
    let normalized = action
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    configuration_hash(&(input_digest, normalized))
}

pub(crate) fn reviewed_input_repair(execution: &StepExecution, input: &str) -> bool {
    execution.status == StepExecutionStatus::WaitingInput && input.starts_with(REPAIR_INPUT_PREFIX)
}

impl Engine {
    pub(crate) fn workflow_has_manager(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
    ) -> Result<bool> {
        let driver = one(
            self.store.as_ref(),
            "workflow_driver_root",
            &root.instance.root_instance_id,
        )?;
        commit.witness([version(&driver)])?;
        Ok(driver
            .value
            .is_some_and(|row| row["data"]["mode"] == "agent"))
    }

    pub(crate) fn workflow_notice_escalation(
        &self,
        commit: &mut WorkflowCommit<'_>,
        target: &WorkflowHeaderFacts,
        interaction: &Value,
    ) -> Result<Option<Id>> {
        let referenced = ["evidence", "materials"]
            .into_iter()
            .filter_map(|field| interaction[field].as_array())
            .flatten()
            .filter_map(|reference| reference["execution_id"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let (pending, mut exhausted) = self.workflow_recovery_attention(commit, target, true)?;
        if pending
            .iter()
            .any(|(id, _)| referenced.contains(id.as_str()))
            || (!pending.is_empty() && !exhausted.iter().any(|id| referenced.contains(id.as_str())))
        {
            return Err(err(
                "technical_recovery_pending",
                "当前可恢复故障仍由经理处理；业务决定请使用正式决定入口",
            ));
        }
        let selected: Vec<_> = exhausted
            .iter()
            .filter(|id| referenced.contains(id.as_str()))
            .collect();
        if exhausted.len() > 1 {
            require(selected.len() == 1, "请指定耗尽额度的一个执行")?;
        }
        Ok(selected
            .first()
            .map(|id| (**id).clone())
            .or_else(|| exhausted.pop()))
    }

    pub(crate) fn workflow_recovery_attention(
        &self,
        commit: &mut WorkflowCommit<'_>,
        target: &WorkflowHeaderFacts,
        include_in_flight: bool,
    ) -> Result<(Vec<(Id, u64)>, Vec<Id>)> {
        let root_id = &target.instance.root_instance_id;
        let event = one(self.store.as_ref(), "workflow_root_event", root_id)?;
        commit.witness([version(&event)])?;
        let mut ids = vec![target.record.id.clone()];
        if target.record.id == *root_id {
            let mut cursor = String::new();
            loop {
                let page = self.native_workflow_members(root_id, &cursor)?;
                let Some(last) = page.last() else { break };
                cursor = last.clone();
                ids.extend(page.into_iter().filter(|id| id != root_id));
                require(ids.len() <= 1024, "请指定需要报告的工作流实例")?;
            }
        }
        let mut exhausted = Vec::new();
        let mut pending = Vec::new();
        for id in ids {
            let root = WorkflowHeaderFacts::read(self, &id)?;
            if matches!(
                root.instance.status,
                StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
            ) {
                continue;
            }
            commit.witness([version(&root.header)])?;
            if !include_in_flight && root.instance.paused {
                continue;
            }
            for current in &root.instance.steps {
                if !matches!(
                    current.status,
                    StepExecutionStatus::AwaitingAcceptance
                        | StepExecutionStatus::Failed
                        | StepExecutionStatus::WaitingInput
                        | StepExecutionStatus::WaitingConfirmation
                        | StepExecutionStatus::NeedsReconciliation
                ) {
                    continue;
                }
                let step = root.step(self, &current.id)?;
                let ready = self.native_step_readiness(&root, &step)?;
                let technical = &ready["recovery"];
                let publication = &ready["source_publication_recovery"];
                let refresh = &ready["delivery_refresh"];
                if technical["status"] == "needs_manager"
                    || publication["retry_allowed"] == true
                    || (include_in_flight && publication["pending"] == true)
                    || refresh["remaining"]
                        .as_u64()
                        .is_some_and(|remaining| remaining > 0)
                {
                    commit.witness([version(&step.step)])?;
                    pending.push((current.id.clone(), step.step.revision));
                }
                if technical["status"] == "exhausted"
                    || publication["human_required"] == true
                    || refresh["remaining"].as_u64() == Some(0)
                {
                    commit.witness([version(&step.step)])?;
                    exhausted.push(current.id.clone());
                }
            }
        }
        Ok((pending, exhausted))
    }

    pub(crate) fn notify_exhausted_recovery(
        &self,
        commit: &mut WorkflowCommit<'_>,
        controller: &str,
        executions: &[Id],
    ) -> Result<()> {
        for id in executions {
            let notice_id = format!("workflow-recovery-escalation:{id}");
            let existing = one(self.store.as_ref(), "governance", &notice_id)?;
            if existing.value.is_some() {
                continue;
            }
            commit.witness([version(&existing)])?;
            let decisions = one(self.store.as_ref(), "workflow_step_decisions", id)?;
            commit.witness([version(&decisions)])?;
            if decisions
                .value
                .as_ref()
                .and_then(Value::as_array)
                .is_some_and(|rows| !rows.is_empty())
            {
                continue;
            }
            let owner = one(self.store.as_ref(), "workflow_execution_owner", id)?;
            commit.witness([version(&owner)])?;
            let instance = text(owner.value.as_ref().unwrap_or(&Value::Null), "instance_id")?;
            let root = WorkflowHeaderFacts::read(self, &instance)?;
            let step = root.step(self, id)?;
            commit.witness(step.reads.versions().iter().cloned())?;
            let label = root
                .instance
                .plan
                .document
                .steps
                .iter()
                .find(|s| s.key == step.execution.step)
                .map_or(step.execution.step.as_str(), |s| s.name.as_str());
            let title = format!("{label}恢复额度已用完");
            let mut notice = record(
                "controller_interaction",
                controller.into(),
                root.record.scope.clone(),
                title.clone(),
                1,
                None,
                json!({"instance_id":instance,"plan_revision_id":root.instance.active_plan_revision_id,"recovery_execution_id":id,
                    "interaction":{"title":title,"background":"原执行仍未满足完成条件","requested_action":"核对原执行和已尝试的修复后决定后续处置",
                        "completed_work":"适用的自动恢复额度已用完","review_summary":step.execution.reason.chars().take(512).collect::<String>(),
                        "materials":[],"evidence":[{"instance_id":instance,"execution_id":id,"label":label,"purpose":"核对原执行、产物和实际恢复历史"}]}}),
                true,
            );
            notice.id = notice_id;
            commit.insert_record(&notice)?;
        }
        Ok(())
    }

    pub(crate) fn schedule_workflow_self_repair(
        &self,
        commit: &mut WorkflowCommit<'_>,
        resources: &WorkflowResources,
        next: &mut StepExecution,
        diagnosis: &str,
        action: &str,
    ) -> Result<Option<String>> {
        let step = resources
            .instance
            .plan
            .document
            .steps
            .iter()
            .find(|step| step.node_id == next.node_id)
            .ok_or_else(|| err("storage", "缺少自修步骤合同"))?;
        if next.sequence >= step.budget.max_turns {
            return Ok(Some("原会话轮次额度已用完，请由经理核对".into()));
        }
        if let Err(error) = self.require_logical_retry_budget(commit, resources, step) {
            if error.code == "retry_budget_exhausted" {
                return Ok(Some(error.message));
            }
            return Err(error);
        }
        let mut resumed = next.clone();
        resumed.sequence += 1;
        resumed.turn_id = None;
        let history = match self
            .prepare_workflow_repair_attempt(commit, resources, &resumed, action, "agent")
        {
            Ok(history) => history,
            Err(error) if error.code == "duplicate_repair_action" => {
                return Ok(Some(error.message));
            }
            Err(error) => return Err(error),
        };
        let sid = resumed
            .session_id
            .as_deref()
            .ok_or_else(|| err("storage", "自修缺少原会话"))?;
        let session = one(self.store.as_ref(), "session", sid)?;
        commit.witness([version(&session)])?;
        resumed.runtime_context["repair_history"] = json!(history);
        let supplemental = json!({"origin":"agent_self_repair","diagnosis":diagnosis,"action":action,
            "instructions":"Read prior attempts and execute this distinct repair in the original authorized scope. Verify a new result before declaring completion."}).to_string();
        let input = render_source_task(&resources.instance, step, &resumed, Some(&supplemental))?;
        commit.runtime_intent(
            resources,
            &resumed,
            "conversation.send",
            sid,
            Some(session.revision),
            json!({"client_message_id":format!("{}:{}",resumed.id,resumed.sequence),"input":input}),
        )?;
        resumed.reason = "Agent 正在按原权限自修".into();
        *next = resumed;
        Ok(None)
    }

    pub(crate) fn workflow_repair_history(
        &self,
        resources: &WorkflowResources,
    ) -> Result<crate::concurrency::ResourceSnapshot> {
        one(
            self.store.as_ref(),
            "workflow_repair_history",
            &json!([
                resources.instance.project_id,
                format!("{}:{}", resources.record.id, resources.execution.node_id)
            ])
            .to_string(),
        )
    }

    pub(crate) fn prepare_workflow_repair_attempt(
        &self,
        commit: &mut WorkflowCommit<'_>,
        resources: &WorkflowResources,
        next: &StepExecution,
        action: &str,
        phase: &str,
    ) -> Result<Vec<Value>> {
        let action = action.trim();
        require(
            !action.is_empty() && action.chars().count() <= 2048,
            "恢复需要具体而有界的修复动作",
        )?;
        let previous = self.workflow_repair_history(resources)?;
        commit.witness([version(&previous)])?;
        let mut history = previous
            .value
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        let digest = action_digest(&resources.execution.input_digest, action);
        if history
            .iter()
            .any(|item| item["started"].is_number() && item["record"]["action_digest"] == digest)
        {
            return Err(err(
                "duplicate_repair_action",
                "当前冻结输入已实际执行过相同修复动作",
            ));
        }
        let same_execution = next.id == resources.execution.id;
        let mut attempt = record(
            "workflow_repair_attempt",
            format!("{}:{}", resources.record.id, resources.execution.node_id),
            resources.record.scope.clone(),
            "步骤修复尝试".into(),
            1,
            None,
            json!({"phase":phase,"original_execution_id":resources.execution.id,
                "source_turn_id":resources.execution.turn_id,"execution_id":next.id,
                "source_failure":resources.execution.failure.as_ref().map(|failure| json!({"code":failure.code,"source_ref":failure.source_ref})),
                "session_id":if same_execution {next.session_id.clone()} else {None},
                "client_message_id":format!("{}:{}",next.id,next.sequence),
                "action":action,"action_digest":digest,"charge_turn":same_execution}),
            true,
        );
        attempt.id = repair_attempt_id(next);
        commit.insert_record(&attempt)?;
        history.truncate(16);
        Ok(history)
    }

    pub(crate) fn observe_workflow_repair_result(
        &self,
        commit: &mut WorkflowCommit<'_>,
        resources: &WorkflowResources,
        next: &StepExecution,
        result: &Value,
    ) -> Result<()> {
        let attempt = one(self.store.as_ref(), "governance", &repair_attempt_id(next))?;
        let Some(raw) = &attempt.value else {
            return Ok(());
        };
        require(
            raw["type"] == "workflow_repair_attempt"
                && raw["immutable"] == true
                && raw["data"]["execution_id"] == next.id,
            "修复结果与原尝试不符",
        )?;
        commit.witness([version(&attempt)])?;
        let mut receipt = record(
            "workflow_repair_result",
            attempt.resource.id.clone(),
            resources.record.scope.clone(),
            "修复轮次结论".into(),
            1,
            None,
            json!({"status":result["status"],"origin":"agent_report","turn_id":next.turn_id,
                "summary":result["summary"].as_str().unwrap_or("").chars().take(512).collect::<String>(),
                "diagnosis":result["recovery"]["diagnosis"].as_str().unwrap_or("").chars().take(512).collect::<String>()}),
            true,
        );
        receipt.id = format!("workflow-repair-result:{}", attempt.resource.id);
        commit.insert_record(&receipt)
    }
}
