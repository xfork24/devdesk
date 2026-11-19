//! Human-requested transport migration keeps the logical work and its source
//! evidence, while replacing the publication and every downstream decision.
use super::*;
use crate::workflow_commits::{WorkflowCommit, WorkflowHeaderFacts};
use std::collections::BTreeSet;

impl Engine {
    pub(crate) fn native_delivery_republish(
        &self,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Value> {
        require(
            matches!(context.actor(), WorkflowActor::Human),
            "交付分支迁移必须由实际人类请求",
        )?;
        let reason = text(&c.payload, "reason")?;
        require(
            !reason.trim().is_empty() && reason.len() <= 8192,
            "交付迁移需要明确原因",
        )?;
        if let Some(receipt) = self.store.replay_commit(&c.command_id, fingerprint)? {
            return Ok(receipt.result);
        }
        let root = WorkflowHeaderFacts::read(
            self,
            c.target
                .as_deref()
                .ok_or_else(|| err("invalid_params", "缺少流程"))?,
        )?;
        require(
            !matches!(
                root.instance.status,
                StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
            ),
            "已结束流程保留历史交付，不重建 PR",
        )?;
        let policy = root
            .instance
            .plan
            .document
            .delivery_policy
            .as_ref()
            .ok_or_else(|| err("configuration_required", "迁移需要冻结交付政策"))?;
        let regression = root
            .instance
            .plan
            .document
            .steps
            .iter()
            .find(|step| step.key == policy.merge_regression_step)
            .ok_or_else(|| err("storage", "缺少合并回归步骤"))?;
        let StepExecutor::Tool { arguments, .. } = &regression.executor else {
            return Err(err("configuration_required", "合并回归不是平台步骤"));
        };
        let Some(InputBinding::Source { source }) = arguments.get("workspace") else {
            return Err(err("configuration_required", "合并回归缺少交付来源"));
        };
        let publication_step = root.step(self, &source.step)?;
        let definition = root
            .instance
            .plan
            .document
            .steps
            .iter()
            .find(|step| step.key == source.step)
            .ok_or_else(|| err("storage", "缺少 PR 发布定义"))?;
        require(
            matches!(&definition.executor,StepExecutor::Tool{tool_ref,..} if root.instance.plan.definitions.get(tool_ref)
            .is_some_and(|tool|tool.data["qualified_name"]=="git.delivery.open")),
            "迁移只允许重建 PR 交付平台步骤",
        )?;
        root.guard_request(c, Some(&publication_step.step))?;
        let selected = one(
            self.store.as_ref(),
            "governance",
            &format!("workflow-delivery:{}", root.record.id),
        )?;
        let state = restored(self, &selected)?;
        let publication: DeliveryPublication =
            serde_json::from_value(state.data["publication"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        require(
            publication_step.execution.status == StepExecutionStatus::Completed
                && state.data["synchronized"] != true
                && !publication.pull_request.merged
                && publication
                    .pull_request
                    .head_branch
                    .starts_with("devdesk/candidates/"),
            "只迁移尚未合入的历史候选分支交付",
        )?;
        require(
            c.payload["expected_pr_number"] == publication.pull_request.number
                && c.payload["expected_head"] == publication.pull_request.head
                && c.payload["expected_candidate"] == publication.revision,
            "原 PR 或候选已变化，请刷新后重试",
        )?;
        let mut preserved = vec![];
        for key in [
            &policy.source_publication_step,
            &policy.source_regression_step,
            &policy.code_review_step,
        ] {
            let resource = root.step(self, key)?;
            require(
                resource.execution.status == StepExecutionStatus::Completed,
                "迁移必须保留已接纳的源发布、测试和评审",
            )?;
            preserved.extend(resource.reads.versions().iter().cloned());
        }
        let head = one(self.store.as_ref(), "head", &root.instance.owner_id)?;
        require(
            head.value
                .as_ref()
                .is_some_and(|head| head["revision"] == publication.source_revision),
            "源工作版本已变化，不能迁移旧审批",
        )?;
        let affected = root
            .instance
            .plan
            .document
            .impacted_nodes(&BTreeSet::from([publication_step.execution.step.clone()]));
        require(
            !affected.contains(&policy.source_publication_step)
                && !affected.contains(&policy.source_regression_step)
                && !affected.contains(&policy.code_review_step)
                && affected.iter().all(|key| {
                    root.instance.plan.document.steps.iter().any(|step| {
                        &step.key == key
                            && !matches!(
                                step.executor,
                                StepExecutor::Agent { .. } | StepExecutor::Workflow { .. }
                            )
                    })
                }),
            "交付迁移不能重跑开发、评审或 Task 子流程",
        )?;
        let mut commit = WorkflowCommit::new(self, "workflow.delivery.republish", c, context)?;
        commit.authorize(&root)?;
        commit.require_no_change(&root.record.id)?;
        commit.witness([version(&root.header), version(&selected), version(&head)])?;
        commit.witness(preserved)?;
        commit.witness([self.platform_receipt_proof(
            &root.instance.root_instance_id,
            "git.delivery.open",
            &json!(publication),
        )?])?;
        let replacement = self.prepare_native_retry_scope(
            &mut commit,
            &root,
            &publication_step,
            true,
            Some(&affected),
        )?;
        commit.new_record(record("workflow_delivery_migration",root.record.id.clone(),root.record.scope.clone(),"改由开发分支重新发布 PR".into(),1,None,
            json!({"status":"requested","actor":context.actor(),"publication":publication,"replacement_execution_id":replacement.id,
                "affected":affected,"reason":c.payload["reason"],"issuer":"kernel_platform"}),true),"delivery-migration")?;
        commit.finish(fingerprint,json!({"id":root.record.id,"execution":replacement,"superseded_pr":publication.pull_request,"source_revision":publication.source_revision,"affected":affected}))
    }
}
