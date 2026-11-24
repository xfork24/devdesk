//! The source Feature retains validation materials. Only its reviewed code
//! projection is published as a mainline PR, after all source gates have passed.
use super::*;

impl Engine {
    pub(super) fn delivery_children_gate(
        &self,
        resources: &WorkflowResources,
        plan: &mut PlatformPlan,
    ) -> Result<()> {
        let key = resources
            .instance
            .plan
            .document
            .delivery_policy
            .as_ref()
            .and_then(|p| p.children_step.as_ref())
            .ok_or_else(|| err("configuration_required", "正式交付需要冻结的 Task 汇合步骤"))?;
        self.delivery_step_result(resources, key, plan)?;
        let parent = resources
            .instance
            .steps
            .iter()
            .find(|step| &step.step == key)
            .ok_or_else(|| err("storage", "Task 汇合执行缺失"))?;
        let collection = one(self.store.as_ref(), "workflow_children_source", &parent.id)?;
        plan.basis = plan.basis.merge([version(&collection)])?;
        let ids = collection
            .value
            .as_ref()
            .and_then(|row| row["data"]["children"].as_array())
            .ok_or_else(|| err("blocked", "Task 子流程集合尚未封存"))?;
        require(ids.len() <= WORKFLOW_STEP_LIMIT, "Task 子流程集合超出预算")?;
        for id in ids {
            let id = id
                .as_str()
                .ok_or_else(|| err("storage", "Task 子流程身份无效"))?;
            let child = crate::workflow_commits::WorkflowHeaderFacts::read(self, id)?;
            require(
                child.instance.parent_execution_id.as_deref() == Some(parent.id.as_str())
                    && child.instance.status == StepExecutionStatus::Completed,
                "所有 Task 必须完成实施和测试并合入 Feature，才能生成正式 candidates",
            )?;
            let delivery = one(
                self.store.as_ref(),
                "governance",
                &format!("workflow-delivery:{id}"),
            )?;
            let saved = restored(self, &delivery)?;
            require(
                saved.data["synchronized"] == true,
                "Task 合入 Feature 尚未完成本地同步",
            )?;
            plan.basis = plan
                .basis
                .merge([version(&child.header), version(&delivery)])?;
        }
        Ok(())
    }
    pub(super) fn drive_delivery_projection(
        &self,
        effect: &ConfigurationRecord,
        action: &DeliveryAction,
        inspect: bool,
    ) -> Result<Option<Value>> {
        let DeliveryAction::OpenSource {
            context,
            source,
            target,
            workspace,
            operation: op,
            delivery,
            state,
            projection: Some(projection),
        } = action
        else {
            return Err(err("storage", "缺少冻结交付投影"));
        };
        require(
            context.target.owner == context.project
                && projection.source_revision == *source
                && projection.target_revision == *target,
            "交付投影归属或源/目标版本不符",
        )?;
        let stage = |name: &str, action| self.platform_publish_stage(effect, name, action, inspect);
        self.git.fetch_delivery_target(context, target)?;
        let id = crate::material_commands::stable_object_id(
            self.store.as_ref(),
            op,
            "delivery-projection",
        )?;
        let candidate = operation(
            &id,
            context,
            RepositoryAction::ProjectDelivery {
                context: context.clone(),
                projection: projection.clone(),
            },
        );
        let Some(value) = stage(
            "projected-source",
            PublishAction::Repository(Box::new(candidate)),
        )?
        else {
            return Ok(None);
        };
        let RepositoryReceipt::Candidate { revision } =
            serde_json::from_value(value).map_err(|e| err("adapter_contract", e.to_string()))?
        else {
            return Err(err("adapter_contract", "交付投影缺少版本回执"));
        };
        require(
            self.git.commit_parents(context, &revision)? == vec![target.clone()],
            "交付投影未绑定冻结目标",
        )?;
        let previous = state
            .value
            .as_ref()
            .map(|_| restored(self, state))
            .transpose()?;
        let previous = previous.as_ref().map(|row| &row.data["publication"]);
        let branch = self.delivery_candidate_branch(
            effect,
            "projected-ref",
            &context.project,
            delivery,
            inspect,
        )?;
        let id =
            crate::material_commands::stable_object_id(self.store.as_ref(), op, "projected-ref")?;
        let publish = operation(
            &id,
            context,
            RepositoryAction::PublishRef {
                merge: delivery.clone(),
                context: context.clone(),
                branch: branch.clone(),
                revision: revision.clone(),
                expected: previous
                    .filter(|p| p["pull_request"]["head_branch"] == branch)
                    .and_then(|p| p["pull_request"]["head"].as_str())
                    .map(str::to_owned),
                stage: PublishStage::DeliveryProjection,
            },
        );
        if stage(
            "projected-ref",
            PublishAction::Repository(Box::new(publish)),
        )?
        .is_none()
        {
            return Ok(None);
        }
        let request = PullRequestRequest {
            repository: context.workspace.repository.clone(),
            project: context.project.clone(),
            merge: delivery.clone(),
            head_branch: branch.clone(),
            base_branch: context.target.branch.clone(),
            candidate: revision.clone(),
            expected_target: target.clone(),
            number: previous
                .filter(|p| {
                    p["projection"] == json!(projection)
                        && p["pull_request"]["head_branch"] == branch
                })
                .and_then(|p| p["pull_request"]["number"].as_u64()),
            projection: Some(projection.clone()),
        };
        let Some(value) = stage(
            "projected-pr",
            PublishAction::DeliveryPullRequest(request.clone()),
        )?
        else {
            return Ok(None);
        };
        let pull_request: PullRequestReceipt =
            serde_json::from_value(value).map_err(|e| err("adapter_contract", e.to_string()))?;
        require(
            pull_request.head == revision
                && pull_request.head_branch == branch
                && pull_request.base_branch == context.target.branch
                && pull_request.marker == request.marker()
                && pull_request.repository == context.workspace.repository,
            "投影 PR 与冻结交付不符",
        )?;
        Ok(Some(json!(DeliveryPublication {
            source_revision: source.clone(),
            source_branch: context.source.branch.clone(),
            target_revision: target.clone(),
            tree: self.git.commit_tree(context, &revision)?,
            revision,
            workspace_id: workspace.clone(),
            owner_id: context.source.owner.clone(),
            operation_id: op.clone(),
            pull_request,
            projection: Some(projection.clone()),
        })))
    }
}
