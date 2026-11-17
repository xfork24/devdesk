//! Business rules consume explicit facts. Activity and waivers contribute
//! their actual resource witnesses to the same optimistic commit.
use crate::{
    platform_facts::one,
    workflow_commits::WorkflowCommit,
    workflow_resources::{WorkflowResources, version},
    *,
};
impl WorkflowCommit<'_> {
    pub fn waiver(
        &mut self,
        instance: &ConfiguredWorkflowInstance,
        node: &str,
        requirement: &str,
    ) -> Result<bool> {
        let row = one(
            self.engine.store.as_ref(),
            "workflow_waiver",
            &json!([instance.active_plan_revision_id, node, requirement]).to_string(),
        )?;
        self.witness([version(&row)])?;
        Ok(row.value.as_ref().is_some_and(|v| {
            v["type"] == "workflow_waiver"
                && v["immutable"] == true
                && v["data"]["plan_revision_id"] == instance.active_plan_revision_id
                && v["data"]["node_id"] == node
                && v["data"]["requirement"] == requirement
        }))
    }
    pub fn rule(
        &mut self,
        instance: &ConfiguredWorkflowInstance,
        node: &str,
        rule: &CompletionRule,
        facts: &WorkflowFacts,
    ) -> Result<bool> {
        if self.waiver(instance, node, "completion")? {
            return Ok(true);
        }
        match rule {
            CompletionRule::All { rules } => {
                for rule in rules {
                    if !self.rule(instance, node, rule, facts)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            CompletionRule::Any { rules } => {
                for rule in rules {
                    if self.rule(instance, node, rule, facts)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            CompletionRule::Independent { .. } => {
                Ok(self.waiver(instance, node, "independence")? || rule.evaluate(facts))
            }
            CompletionRule::Covers { .. } => {
                Ok(self.waiver(instance, node, "coverage")? || rule.evaluate(facts))
            }
            _ => Ok(rule.evaluate(facts)),
        }
    }
    pub fn execution_active(&mut self, resources: &WorkflowResources) -> Result<bool> {
        self.execution_activity(&resources.execution, 0)
    }
    pub fn archived_execution_active(&mut self, execution: &StepExecution) -> Result<bool> {
        self.execution_activity(execution, 0)
    }
    fn execution_activity(&mut self, execution: &StepExecution, depth: usize) -> Result<bool> {
        require(depth <= WORKFLOW_DEPTH_LIMIT, "执行子树超过深度预算")?;
        let mut active = false;
        if let Some(session) = &execution.session_id {
            let workers = one(self.engine.store.as_ref(), "session_live_workers", session)?;
            let state = one(self.engine.store.as_ref(), "session_input_state", session)?;
            active |= workers
                .value
                .as_ref()
                .and_then(Value::as_array)
                .is_some_and(|w| !w.is_empty())
                || state.value.as_ref().is_some_and(|s| s["active"] == true);
            self.witness([version(&workers), version(&state)])?;
        }
        if let Some(id) = &execution.tool_call_id {
            let call = one(self.engine.store.as_ref(), "governance", id)?;
            active |= call.value.as_ref().is_some_and(|r| {
                !matches!(
                    r["data"]["status"].as_str(),
                    Some("succeeded" | "failed" | "cancelled")
                )
            });
            self.witness([version(&call)])?;
        }
        let calls = one(
            self.engine.store.as_ref(),
            "step_platform_calls",
            &execution.id,
        )?;
        active |= calls
            .value
            .as_ref()
            .and_then(Value::as_array)
            .is_some_and(|c| !c.is_empty());
        self.witness([version(&calls)])?;
        let children = one(
            self.engine.store.as_ref(),
            "workflow_children_source",
            &execution.id,
        )?;
        self.witness([version(&children)])?;
        if let Some(raw) = &children.value {
            let ids = raw["data"]["children"]
                .as_array()
                .ok_or_else(|| err("storage", "子流程集合缺失"))?;
            require(ids.len() <= WORKFLOW_STEP_LIMIT, "子流程集合超过预算")?;
            for id in ids {
                let child = crate::workflow_commits::WorkflowHeaderFacts::read(
                    self.engine,
                    id.as_str()
                        .ok_or_else(|| err("storage", "子流程身份无效"))?,
                )?;
                require(
                    child.instance.parent_execution_id.as_deref() == Some(execution.id.as_str()),
                    "子流程不属于原父执行",
                )?;
                self.witness([version(&child.header)])?;
                for actual in &child.instance.steps {
                    let selected = one(
                        self.engine.store.as_ref(),
                        "workflow_step",
                        &json!([child.record.id, actual.id]).to_string(),
                    )?;
                    let step: StepExecution =
                        serde_json::from_value(self.engine.store.restore_runtime_value(
                            selected.value.as_ref().ok_or_else(|| {
                                err("revision_conflict", "原子执行在准备期间已替换")
                            })?,
                        )?)
                        .map_err(|e| err("storage", e.to_string()))?;
                    require(
                        step.id == actual.id && step.workflow_instance_id == child.record.id,
                        "原子执行归属不符",
                    )?;
                    self.witness([version(&selected)])?;
                    active |= self.execution_activity(&step, depth + 1)?;
                }
            }
        }
        Ok(active)
    }
    pub fn require_acceptance(
        &mut self,
        resources: &WorkflowResources,
        facts: &WorkflowFacts,
    ) -> Result<()> {
        let execution = &resources.execution;
        let instance = &resources.instance;
        let step = instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == execution.step)
            .ok_or_else(|| err("storage", "原接纳步骤定义缺失"))?;
        require(
            matches!(
                execution.status,
                StepExecutionStatus::AwaitingAcceptance | StepExecutionStatus::WaitingConfirmation
            ),
            "步骤没有可接纳的结果",
        )?;
        require(
            !self.execution_active(resources)?,
            "执行者或未核对副作用尚未停止",
        )?;
        let approvals = one(
            self.engine.store.as_ref(),
            "workflow_confirmations",
            &execution.id,
        )?;
        self.witness([version(&approvals)])?;
        let matching = approvals
            .value
            .as_ref()
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter(|r| {
                        r["data"]["input_digest"] == execution.input_digest
                            && r["data"]["outputs"] == json!(execution.outputs)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let overridden = matching
            .iter()
            .any(|r| r["data"]["override_completion"] == true);
        if !overridden && !self.rule(instance, &step.node_id, &step.completion, facts)? {
            return Err(err("completion", "步骤完成条件尚未满足"));
        }
        if step.confirmation == ConfirmationPolicy::Human
            && matching.is_empty()
            && !self.waiver(instance, &step.node_id, "confirmation")?
        {
            return Err(err("human_confirmation", "等待实际人类决定"));
        }
        Ok(())
    }
}
