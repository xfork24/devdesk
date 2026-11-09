//! Model content judgments and human decisions are separate, auditable facts.
use crate::*;

pub(crate) fn reports(s: &EngineeringState, step: &WorkflowStepRuntime) -> Vec<Value> {
    let runs = if let Some(attempt) = step.outputs.get("attempt") {
        s.latest_runs(Some(attempt), None)
            .into_iter()
            .filter(|r| !r.node.starts_with("review:"))
            .collect()
    } else if let Some(delivery) = step.outputs.get("delivery") {
        s.runs
            .iter()
            .rev()
            .find(|r| r.node == format!("review:{delivery}"))
            .into_iter()
            .collect()
    } else {
        vec![]
    };
    runs.into_iter().filter_map(|run| {
        let result = run.result.as_ref()?;
        (result["approval_required"] == true && result["model_approve"].is_boolean()
            && run.status == Status::Succeeded).then(|| json!({"run_id":run.id,"node":run.node,
                "approve":result["model_approve"],"summary":result["summary"],"checks":result["checks"],
                "skills":result.get("skill_refs").filter(|v|v.is_array()).cloned().unwrap_or_else(||json!(run.binding.skills))}))
    }).collect()
}

pub(crate) fn input_hash(s: &EngineeringState, lock: &str, step: &WorkflowStepRuntime) -> String {
    let reports = reports(s, step);
    if reports.is_empty() {
        configuration_hash(&json!({"lock":lock,"outputs":step.outputs}))
    } else {
        configuration_hash(&json!({"lock":lock,"outputs":step.outputs,"model_approvals":reports}))
    }
}

impl Engine {
    pub(crate) fn validate_human_approval(
        &self,
        s: &mut EngineeringState,
        definition: &WorkflowStep,
        step: &WorkflowStepRuntime,
        reports: &[Value],
        c: &Command,
    ) -> Result<()> {
        let rejected = reports.iter().any(|report| report["approve"] == false);
        if rejected {
            require(
                c.payload["overwrite_model"] == true,
                "模型未 Approve；继续需明确选择覆盖模型结论",
            )?;
            text(&c.payload, "reason")?;
        }
        if let Some(id) = step.outputs.get("delivery") {
            let delivery = s.delivery(id)?.clone();
            require(
                !delivery.stale && s.delivery_inputs_current(&delivery),
                "交付版本或证据已失效",
            )?;
            self.check_merge_target(s, &delivery)?;
            if matches!(definition.kind.as_str(), "delivery.review" | "merge.review") {
                if rejected {
                    self.apply_command(
                        "override.accept",
                        &Command {
                            command_id: format!("{}:model-overwrite", c.command_id),
                            target: Some(id.clone()),
                            expected_revision: None,
                            payload: json!({"items":["review"],"reason":c.payload["reason"]}),
                        },
                        s,
                    )?;
                }
                self.reconcile(s, &delivery)?;
                require(s.accepted(&delivery), "交付证据或独立评审条件尚未满足")?;
            }
        }
        Ok(())
    }
}
