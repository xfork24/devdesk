//! Shared, read-only business eligibility. Authorization and CAS remain at command admission.
use crate::configured_runtime::{active_execution, facts, material_inputs};
use crate::orchestration::{rule_satisfied, waiver};
use crate::*;

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowActionEligibility {
    pub allowed: bool,
    pub code: &'static str,
    pub reason: String,
}
impl WorkflowActionEligibility {
    fn ready() -> Self {
        Self {
            allowed: true,
            code: "ready",
            reason: String::new(),
        }
    }
    fn blocked(code: &'static str, reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            code,
            reason: reason.into(),
        }
    }
    pub(crate) fn require(&self) -> Result<()> {
        if self.allowed {
            Ok(())
        } else {
            Err(err(
                if self.code == "change_pending" {
                    "change_pending"
                } else {
                    "blocked"
                },
                &self.reason,
            ))
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct WorkflowStepActions {
    pub dispatch: WorkflowActionEligibility,
    pub accept: WorkflowActionEligibility,
    pub retry: WorkflowActionEligibility,
}
fn common(
    state: &EngineeringState,
    id: &str,
    instance: &ConfiguredWorkflowInstance,
) -> Option<WorkflowActionEligibility> {
    if instance.cancel_requested
        || matches!(
            instance.status,
            StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
        )
    {
        Some(WorkflowActionEligibility::blocked(
            "terminal",
            "流程已结束或取消",
        ))
    } else if crate::workflow_changes::change_pending(state, id) {
        Some(WorkflowActionEligibility::blocked(
            "change_pending",
            "运行计划正在变更",
        ))
    } else {
        None
    }
}
pub(crate) fn step_actions(
    state: &EngineeringState,
    id: &str,
    instance: &ConfiguredWorkflowInstance,
    step: &StepDefinition,
    execution: &StepExecution,
) -> Result<WorkflowStepActions> {
    if let Some(blocked) = common(state, id, instance) {
        return Ok(WorkflowStepActions {
            dispatch: blocked.clone(),
            accept: blocked.clone(),
            retry: blocked,
        });
    }
    let current = facts(state, instance)?;
    step_actions_with_facts(state, id, instance, step, execution, &current)
}
pub(crate) fn step_actions_with_facts(
    state: &EngineeringState,
    id: &str,
    instance: &ConfiguredWorkflowInstance,
    step: &StepDefinition,
    execution: &StepExecution,
    current: &WorkflowFacts,
) -> Result<WorkflowStepActions> {
    if let Some(blocked) = common(state, id, instance) {
        return Ok(WorkflowStepActions {
            dispatch: blocked.clone(),
            accept: blocked.clone(),
            retry: blocked,
        });
    }
    let dispatch = if execution.status != StepExecutionStatus::Queued {
        WorkflowActionEligibility::blocked("execution_state", "步骤已派发或已结束")
    } else if state.workflow_instance_suspended(id) {
        WorkflowActionEligibility::blocked("paused", "流程暂停中")
    } else if !step.dependencies.iter().all(|key| {
        instance.steps.iter().any(|e| {
            &e.step == key
                && matches!(
                    e.status,
                    StepExecutionStatus::Completed | StepExecutionStatus::Skipped
                )
        })
    }) {
        WorkflowActionEligibility::blocked("dependencies", "依赖尚未接纳")
    } else {
        let slot = if let Some(parent) = &instance.parent_execution_id {
            let group = state
                .governance
                .iter()
                .find(|r| r.r#type == "workflow_children" && r.definition_id == *parent)
                .ok_or_else(|| err("invalid_workflow", "子流程来源缺失"))?;
            let children = group.data["children"]
                .as_array()
                .ok_or_else(|| err("storage", "子流程集合无效"))?;
            let others = children
                .iter()
                .filter_map(Value::as_str)
                .filter(|child| *child != id)
                .filter_map(|child| state.configuration(child).ok())
                .filter(|r| r.data["status"] == "running")
                .count();
            others < group.data["max_concurrency"].as_u64().unwrap_or(1) as usize
        } else {
            true
        };
        if !slot {
            WorkflowActionEligibility::blocked("foreach_capacity", "子流程并发已满")
        } else if step.condition.evaluate(&current) {
            match material_inputs(step, current) {
                Ok(_) => WorkflowActionEligibility::ready(),
                Err(error) => WorkflowActionEligibility::blocked("inputs", error.message),
            }
        } else {
            WorkflowActionEligibility::ready()
        } // Dispatch records the configured skip.
    };
    let overridden = state.governance.iter().any(|r| {
        r.r#type == "workflow_confirmation"
            && r.definition_id == execution.id
            && r.data["override_completion"] == true
            && r.data["input_digest"] == execution.input_digest
            && r.data["outputs"] == json!(execution.outputs)
    });
    let accept = if !matches!(
        execution.status,
        StepExecutionStatus::AwaitingAcceptance | StepExecutionStatus::WaitingConfirmation
    ) {
        WorkflowActionEligibility::blocked("execution_state", "步骤没有可接纳的结果")
    } else if active_execution(state, execution) {
        WorkflowActionEligibility::blocked("execution_stop", "执行者尚未停止")
    } else if !overridden
        && !rule_satisfied(state, instance, &step.node_id, &step.completion, current)
    {
        WorkflowActionEligibility::blocked("completion", "完成条件未满足")
    } else if step.confirmation == ConfirmationPolicy::Human
        && !current.approvals.contains(&step.key)
        && !waiver(state, instance, &step.node_id, "confirmation")
    {
        WorkflowActionEligibility::blocked("human_confirmation", "等待人类决定")
    } else {
        WorkflowActionEligibility::ready()
    };
    let retry = if !matches!(
        execution.status,
        StepExecutionStatus::Failed | StepExecutionStatus::Cancelled
    ) {
        WorkflowActionEligibility::blocked("execution_state", "此步骤不能技术重试")
    } else if active_execution(state, execution) {
        WorkflowActionEligibility::blocked("execution_stop", "请先停止原执行者并核对副作用")
    } else if execution.sequence > step.budget.retry_limit {
        WorkflowActionEligibility::blocked("retry_budget", "已达到重试上限")
    } else {
        WorkflowActionEligibility::ready()
    };
    Ok(WorkflowStepActions {
        dispatch,
        accept,
        retry,
    })
}
pub(crate) fn completion_action(
    state: &EngineeringState,
    id: &str,
    instance: &ConfiguredWorkflowInstance,
) -> Result<WorkflowActionEligibility> {
    if let Some(blocked) = common(state, id, instance) {
        return Ok(blocked);
    }
    if !instance.steps.iter().all(|e| {
        matches!(
            e.status,
            StepExecutionStatus::Completed | StepExecutionStatus::Skipped
        )
    }) {
        return Ok(WorkflowActionEligibility::blocked(
            "steps",
            "仍有未处置的步骤",
        ));
    }
    let current = facts(state, instance)?;
    if !rule_satisfied(
        state,
        instance,
        "$workflow",
        &instance.plan.document.completion,
        &current,
    ) {
        return Ok(WorkflowActionEligibility::blocked(
            "completion",
            "整体完成条件未满足",
        ));
    }
    if instance
        .plan
        .document
        .outputs
        .values()
        .any(|source| current.value(source).is_none())
    {
        return Ok(WorkflowActionEligibility::blocked(
            "outputs",
            "缺少最终交付输出",
        ));
    }
    Ok(WorkflowActionEligibility::ready())
}
