//! Business instructions come from the current frozen Workflow task.
use crate::*;
use std::collections::BTreeSet;

impl Engine {
    pub(crate) fn configured_run_prompt(&self, s: &EngineeringState, run: &Run) -> Result<Value> {
        let attempt = run.attempt_id.as_deref().and_then(|id| s.attempt(id).ok());
        let delivery = run
            .node
            .strip_prefix("review:")
            .and_then(|id| s.delivery(id).ok());
        let feature = attempt
            .map(|a| a.feature_id.as_str())
            .or_else(|| delivery.map(|d| d.feature_id.as_str()))
            .ok_or_else(|| err("configuration_required", "模型任务缺少工作流归属"))?;
        let task = attempt
            .and_then(|a| a.task_id.as_deref())
            .or_else(|| delivery.and_then(|d| d.task_id.as_deref()));
        let lock = s
            .feature_configuration(feature)
            .ok_or_else(|| err("configuration_required", "模型任务缺少冻结工作流配置"))?;
        let plan = if task.is_some() {
            &lock.task_plan
        } else {
            &lock.plan
        };
        let target = if delivery.is_some() {
            run.id.as_str()
        } else {
            attempt.unwrap().id.as_str()
        };
        let source = s
            .governance
            .iter()
            .find(|r| r.kind == "execution_specification" && r.definition_id == target)
            .and_then(|r| r.data["step"].as_str());
        let step = plan
            .steps
            .iter()
            .find(|step| Some(step.key.as_str()) == source)
            .or_else(|| {
                plan.steps.iter().find(|step| {
                    if let Some(delivery) = delivery {
                        step.kind
                            == if delivery.merge_id.is_some() {
                                "merge.review"
                            } else {
                                "delivery.review"
                            }
                            && step
                                .parameters
                                .get("phase")
                                .and_then(Value::as_str)
                                .is_none_or(|phase| phase == delivery.kind)
                    } else {
                        step.kind == "attempt.execute"
                            && step.parameters.get("phase").and_then(Value::as_str)
                                == attempt.map(|a| a.phase.as_str())
                    }
                })
            })
            .ok_or_else(|| err("configuration_required", "找不到该模型任务对应的工作流步骤"))?;
        let instructions = step
            .task_instructions
            .as_deref()
            .ok_or_else(|| err("configuration_required", "步骤缺少当前任务指令"))?;
        require(
            !instructions.trim().is_empty(),
            "工作流步骤的任务提示词不能为空",
        )?;
        let node = step
            .execution_recipe
            .iter()
            .find(|node| node.key == run.node);
        let node_instructions = node.and_then(|n| n.task_instructions.as_deref());
        let output_contract = if ["test_author", "implement", "docs"].contains(&run.node.as_str()) {
            agent_task_output_contract(step, Some(&run.node))
        } else {
            stage_answer_output_contract()
        };
        Ok(
            json!({"source_step":step.key,"instructions":instructions,"node_instructions":node_instructions,
            "output_contract":output_contract}),
        )
    }

    pub(crate) fn skill_quality_basis(
        &self,
        s: &EngineeringState,
        run: &Run,
        documents: &[Id],
    ) -> Result<Value> {
        let mut skills: BTreeSet<Id> = run.binding.skills.iter().cloned().collect();
        for proposal in s
            .proposals
            .iter()
            .filter(|p| p.adopted_objects.iter().any(|id| documents.contains(id)))
        {
            if let Some(agent) = s
                .sessions
                .iter()
                .find(|session| session.id == proposal.session_id)
                .and_then(|session| s.definitions.iter().find(|d| d.id == session.agent))
                .and_then(|d| d.agent_profile.as_ref())
            {
                skills.extend(agent.skill_revisions.iter().cloned());
            }
        }
        if let Some(delivery) = run
            .node
            .strip_prefix("review:")
            .and_then(|id| s.delivery(id).ok())
        {
            for evidence in s.runs.iter().filter(|r| delivery.evidence.contains(&r.id)) {
                skills.extend(evidence.binding.skills.iter().cloned());
            }
        }
        let specifications = if let Some(binding) = crate::agent_tasks::binding_for_run(s, run) {
            let definition: WorkflowTaskDefinition =
                serde_json::from_value(binding.data["definition"].clone())
                    .map_err(|e| err("task_contract_mismatch", e.to_string()))?;
            Some(
                definition
                    .decode_step()?
                    .spec_refs
                    .iter()
                    .map(|id| {
                        s.definitions
                            .iter()
                            .find(|d| &d.id == id)
                            .cloned()
                            .ok_or_else(|| err("task_contract_mismatch", "冻结 Spec 不存在"))
                    })
                    .collect::<Result<Vec<_>>>()?,
            )
        } else {
            self.execution_specifications(s, run)?
        };
        Ok(
            json!({"skills":skills.iter().filter_map(|id|s.definitions.iter().find(|d| &d.id==id && d.family=="skill")).collect::<Vec<_>>(),
            "specifications":specifications,
            "boundary":{"node":run.node,"delivery_kind":run.node.strip_prefix("review:").and_then(|id|s.delivery(id).ok()).map(|d|&d.kind),
                "phase":run.attempt_id.as_deref().and_then(|id|s.attempt(id).ok()).map(|a|&a.phase)}}),
        )
    }
}
