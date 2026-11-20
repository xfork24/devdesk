//! Optional deterministic actions under an exact, finite delegation.
use crate::{
    platform_facts::one, workflow_commits::WorkflowHeaderFacts,
    workflow_resources::WorkflowResources, *,
};

pub(crate) fn segment_human_request_id(execution: &StepExecution) -> String {
    format!(
        "segment-human-request:{}",
        configuration_hash(&(
            &execution.workflow_instance_id,
            &execution.id,
            &execution.input_digest,
            &execution.outputs
        ))
    )
}

impl Engine {
    pub(crate) fn prepare_segment_human_request(
        &self,
        segment: &ConfigurationRecord,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
        ready: &Value,
    ) -> Result<bool> {
        if ready["actions"]["accept"]["code"] != "human_confirmation"
            || ready["completion_satisfied"] != true
            || ready["human_satisfied"] == true
        {
            return Ok(false);
        }
        // All grants for the same exact result share one identity. The insert
        // guards absence atomically, including concurrent overlapping grants.
        let identity = segment_human_request_id(&resources.execution);
        // Includes rejected/resolved requests: never recreate the same plea.
        if let Some(prior) = one(self.store.as_ref(), "governance", &identity)?.value {
            require(
                prior["data"]["status"] != "rejected",
                "普通人工签核已拒绝，需经理处理",
            )?;
            return Ok(false);
        }
        // Preserve a request already prepared by a manager/Human for this result.
        if self
            .controller_approval_context(&root.instance.root_instance_id)?
            .iter()
            .any(|r| {
                r["current"] == true
                    && r["approval_source"] != "workflow_step"
                    && r["data"]["request"]["arguments"]["execution_id"] == resources.execution.id
            })
        {
            return Ok(false);
        }
        let packet = self
            .controller_step_approvals(&root.instance.root_instance_id)?
            .into_iter()
            .find(|p| {
                p["human_required"] == true
                    && p["arguments"]["execution_id"] == resources.execution.id
            })
            .ok_or_else(|| err("blocked", "原审批材料尚未就绪"))?;
        let mut arguments = packet["arguments"].clone();
        arguments["instance_id"] = json!(root.record.id);
        arguments["accept_result"] = json!(false);
        let command = Command {
            command_id: format!(
                "{identity}:{}:{}",
                root.header.revision, resources.step.revision
            ),
            target: Some(root.record.id.clone()),
            expected_revision: Some(root.record.revision),
            payload: json!({"plan_revision_id":root.instance.active_plan_revision_id,"control_version":root.instance.control_version,
                "operation":"workflow.step.confirm","arguments":arguments,"interaction":packet["interaction"],
                "reason":"原有界委托准备普通人工签核材料；不授予签署或豁免权"}),
        };
        self.execute_in_context(
            "workflow.decision.request",
            command,
            &InvocationContext::workflow(
                WorkflowActor::Segment {
                    authorization_id: segment.id.clone(),
                },
                Some(root.instance.project_id.clone()),
            ),
        )?;
        Ok(true)
    }

    pub(crate) fn complete_segment_instance(
        &self,
        segment: &ConfigurationRecord,
        root: &WorkflowHeaderFacts,
    ) -> Result<Value> {
        self.execute_in_context("workflow.complete", Command {
            command_id: format!("segment:{}:complete:{}",segment.id,root.header.revision),
            target: Some(root.record.id.clone()), expected_revision: Some(root.record.revision),
            payload: json!({"plan_revision_id":root.instance.active_plan_revision_id,"control_version":root.instance.control_version,
                "reason":"原完整节点委托核对全部接纳、完成规则及交付证据后登记完成"}),
        }, &InvocationContext::workflow(WorkflowActor::Segment { authorization_id: segment.id.clone() },Some(root.instance.project_id.clone())))
    }
}
