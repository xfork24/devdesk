//! Recovery of frozen delivery actions under a new reconciliation authorization.
use crate::{
    concurrency::*,
    governance::record,
    platform_delivery::{DeliveryAction, delivery_source_operation},
    platform_facts::{PlatformAction, PlatformPlan, one, restored},
    platform_publish::PublishAction,
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_resources::{WorkflowResources, live_workflow_chain, version},
    *,
};

pub(crate) const SOURCE_PUBLICATION_REWORK: &str = "rework_source_publication";
pub(crate) const SOURCE_PUBLICATION_RETRY: &str = "retry_source_publication";

pub(crate) fn source_publication_recovery(resolution: &Value) -> bool {
    matches!(
        resolution.as_str(),
        Some(SOURCE_PUBLICATION_REWORK | SOURCE_PUBLICATION_RETRY)
    )
}

impl Engine {
    fn source_publication_retry_budget(
        &self,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
    ) -> Result<(ResourceSnapshot, ResourceSnapshot, u64, u64)> {
        let node = &resources.execution.node_id;
        let limit = root
            .instance
            .plan
            .document
            .steps
            .iter()
            .find(|definition| definition.node_id == *node)
            .ok_or_else(|| err("storage", "源发布步骤缺少重试额度"))?
            .budget
            .retry_limit;
        let (retries, physical) = self.logical_retries(resources)?;
        let publication = one(
            self.store.as_ref(),
            "workflow_publication_repair_count",
            &json!([root.instance.project_id, root.record.id, node]).to_string(),
        )?;
        let used = publication
            .value
            .as_ref()
            .and_then(|r| r["count"].as_u64())
            .unwrap_or(0);
        Ok((publication, retries, physical.saturating_add(used), limit))
    }

    pub(crate) fn require_source_publication_retry(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
    ) -> Result<()> {
        let (budget, retries, used, limit) =
            self.source_publication_retry_budget(root, resources)?;
        if used >= limit {
            return Err(err(
                "retry_budget_exhausted",
                "源分支发布重试额度已用尽，请由人类确认返工",
            ));
        }
        commit.witness([version(&budget), version(&retries)])
    }

    pub(crate) fn source_publication_recovery_view(
        &self,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
    ) -> Result<Value> {
        let Some((operation, _)) = self.source_publication_rework_target(root, resources)? else {
            return Ok(Value::Null);
        };
        let RepositoryAction::PublishRef {
            branch, revision, ..
        } = operation.action
        else {
            return Ok(Value::Null);
        };
        let (_, _, used, limit) = self.source_publication_retry_budget(root, resources)?;
        let active = one(
            self.store.as_ref(),
            "workflow_reconciliation_active",
            &resources.execution.id,
        )?;
        let latest = one(
            self.store.as_ref(),
            "workflow_reconciliation_latest",
            &resources.execution.id,
        )?;
        let pending = active.value.is_some();
        let error = latest
            .value
            .as_ref()
            .map(|record| &record["data"]["error"])
            .filter(|error| error.is_object())
            .map(|error| json!({"code":error["code"],"message":error["message"]}));
        Ok(
            json!({"branch":branch,"revision":revision,"attempts":used,"limit":limit,
            "retry_allowed":!pending && used < limit,"human_required":!pending && used >= limit,
            "pending":pending,"error":error}),
        )
    }

    pub(crate) fn source_publication_rework_target(
        &self,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
    ) -> Result<Option<(RepositoryOperation, ReadSet)>> {
        let step = &resources.execution;
        if step.status != StepExecutionStatus::NeedsReconciliation
            || root
                .instance
                .plan
                .document
                .delivery_policy
                .as_ref()
                .is_none_or(|policy| policy.source_publication_step != step.step)
        {
            return Ok(None);
        }
        let Some(call_id) = step.tool_call_id.as_deref() else {
            return Ok(None);
        };
        let selected = one(self.store.as_ref(), "governance", call_id)?;
        let call = restored(self, &selected)?;
        if call.r#type != "platform_call"
            || call.data["request"]["definition"]["qualified_name"] != "git.source.publish"
            || call.data["status"] != "needs_reconciliation"
            || call.data["remote_effect"] != "unknown"
        {
            return Ok(None);
        }
        let request: crate::platform_tools::PlatformToolRequest =
            serde_json::from_value(call.data["request"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        require(
            call.scope == root.record.scope
                && request.instance_id == root.record.id
                && request.project_id == root.instance.project_id
                && request.owner_id == root.instance.owner_id
                && request.step_execution_id() == step.id
                && request.caller.is_none()
                && call.data["cancel_requested"] != true,
            "原源分支发布已取消或与当前执行不符",
        )?;
        let effect_row = one(
            self.store.as_ref(),
            "governance",
            &text(&call.data, "effect_id")?,
        )?;
        let effect = restored(self, &effect_row)?;
        require(
            effect.r#type == "platform_effect_intent"
                && effect.immutable
                && effect.scope == call.scope
                && effect.definition_id == call.id
                && effect.data["request_digest"] == configuration_hash(&request),
            "返工缺少原源分支发布意图",
        )?;
        let plan: PlatformPlan = serde_json::from_value(effect.data["plan"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let PlatformAction::Delivery(action) = &plan.action else {
            return Ok(None);
        };
        let DeliveryAction::Source {
            context,
            revision,
            operation,
        } = action.as_ref()
        else {
            return Ok(None);
        };
        require(
            context.project == request.project_id
                && context.source.owner == request.owner_id
                && context.source.branch != context.target.branch
                && context.source.branch != context.workspace.mainline,
            "返工只能恢复原开发分支",
        )?;
        let operation = delivery_source_operation(operation, context, revision);
        let stage_row = one(
            self.store.as_ref(),
            "governance",
            &format!("platform-publish-stage:{}:source", effect.id),
        )?;
        if stage_row.value.is_none() {
            return Ok(None);
        }
        let stage = restored(self, &stage_row)?;
        require(
            stage.r#type == "platform_publish_stage"
                && stage.immutable
                && stage.scope == effect.scope
                && stage.definition_id == effect.id
                && stage.data["issuer"] == "kernel_platform"
                && stage.data["action"]
                    == json!(PublishAction::Repository(Box::new(operation.clone()))),
            "返工缺少原源分支发布阶段授权",
        )?;
        let fence = plan
            .fence
            .as_ref()
            .ok_or_else(|| err("storage", "源发布缺少原工作区写入授权"))?;
        let fence_row = one(self.store.as_ref(), "governance", &fence.id)?;
        require(
            fence_row
                .value
                .as_ref()
                .is_some_and(|v| v["data"] == fence.data && v["data"]["status"] == "active"),
            "原源分支发布已失去工作区写入授权",
        )?;
        let pending_change = one(
            self.store.as_ref(),
            "workflow_change_pending",
            &root.record.id,
        )?;
        require(pending_change.value.is_none(), "运行计划正在变更")?;
        let (_, chain) = live_workflow_chain(
            self.store.as_ref(),
            &root.record.id,
            &step.id,
            StepExecutionStatus::NeedsReconciliation,
        )?;
        let basis = crate::platform_reconciliation::reconciliation_business_basis(
            &plan,
            &selected.resource,
        )?
        .merge(
            plan.basis
                .versions()
                .iter()
                .filter(|seen| {
                    seen.resource.kind == "workflow_step"
                        && seen.resource != resources.step.resource
                })
                .cloned(),
        )?
        .merge(chain.versions().iter().cloned())?
        .merge([
            version(&selected),
            version(&effect_row),
            version(&stage_row),
            version(&fence_row),
            version(&pending_change),
        ])?;
        Ok(Some((operation, basis)))
    }

    pub(crate) fn recover_reconciled_source_publication(
        &self,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
        attempt: &ConfigurationRecord,
        authority: &ReadSet,
    ) -> Result<RepositoryReceipt> {
        require(
            attempt.r#type == "workflow_reconciliation_attempt"
                && attempt.immutable
                && attempt.data["issuer"] == "kernel_reconciliation"
                && ((attempt.data["actor"]["kind"] == "human"
                    && attempt.data["resolution"] == SOURCE_PUBLICATION_REWORK)
                    || (attempt.data["actor"]["kind"] == "controller"
                        && attempt.data["resolution"] == SOURCE_PUBLICATION_RETRY))
                && attempt.data["call_id"].as_str() == resources.execution.tool_call_id.as_deref(),
            "补发需要对原发布的受限重试或人工返工授权",
        )?;
        let (operation, basis) = self
            .source_publication_rework_target(root, resources)?
            .ok_or_else(|| err("needs_reconciliation", "原步骤缺少可返工的源分支发布授权"))?;
        let key = ResourceKey::new(
            "governance",
            format!("source-publication-rework:{}", attempt.id),
        );
        let previous = one(self.store.as_ref(), "governance", &key.id)?;
        let mut grant = record(
            "source_publication_rework_grant",
            attempt.id.clone(),
            attempt.scope.clone(),
            "仅创建原先锁定的源分支".into(),
            1,
            None,
            json!({"operation": operation, "issuer": "kernel_reconciliation"}),
            true,
        );
        grant.id = key.id.clone();
        if previous.value.is_some() {
            let saved = restored(self, &previous)?;
            require(
                saved.r#type == grant.r#type
                    && saved.immutable
                    && saved.definition_id == attempt.id
                    && saved.data == grant.data,
                "补发授权与原操作不符",
            )?;
            return self.git.recover_source_publication(&operation, false);
        }
        let acquired = self.store.submit_changes(&PreparedChangeSet {
            command_id: key.id.clone(),
            fingerprint: configuration_hash(&grant.data),
            read_set: basis.merge(authority.versions().iter().cloned())?.merge([
                version(&previous),
                ResourceVersion {
                    resource: ResourceKey::new("governance", &attempt.id),
                    revision: attempt.revision,
                },
            ])?,
            changes: vec![ResourceChange {
                resource: key,
                value: self.store.encode_runtime_record(&grant)?,
            }],
            result: json!({"id":grant.id}),
            time: now() as i64,
        })?;
        self.git
            .recover_source_publication(&operation, acquired.applied)
    }

    pub(crate) fn release_failed_delivery_fence(
        &self,
        commit: &mut crate::workflow_commits::WorkflowCommit<'_>,
        resources: &WorkflowResources,
    ) -> Result<()> {
        let Some(id) = resources.execution.tool_call_id.as_deref() else {
            return Ok(());
        };
        let selected = one(self.store.as_ref(), "governance", id)?;
        let call = restored(self, &selected)?;
        let original = one(
            self.store.as_ref(),
            "governance",
            call.data["effect_id"].as_str().unwrap_or(""),
        )?;
        let effect = restored(self, &original)?;
        let plan: PlatformPlan = serde_json::from_value(effect.data["plan"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let Some(fence) = &plan.fence else {
            return Ok(());
        };
        let current = one(self.store.as_ref(), "governance", &fence.id)?;
        let row = restored(self, &current)?;
        commit.witness([version(&selected), version(&original), version(&current)])?;
        if row.data["status"] == "closed" {
            return Ok(());
        }
        let request: PlatformToolRequest = serde_json::from_value(call.data["request"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        require(
            call.r#type == "platform_call"
                && call.scope == resources.record.scope
                && call.data["status"] == "failed"
                && call.data["remote_effect"] == "not_applied"
                && call.data["error"]["code"] == "stale_target"
                && request.execution_id == resources.execution.id
                && request.instance_id == resources.record.id
                && effect.r#type == "platform_effect_intent"
                && effect.immutable
                && effect.definition_id == call.id
                && effect.data["request_digest"] == configuration_hash(&request)
                && matches!(&plan.action,PlatformAction::Delivery(action) if matches!(action.as_ref(),DeliveryAction::Merge{..}))
                && row.r#type == "platform_workspace_fence"
                && row.revision == fence.revision
                && row.data == fence.data,
            "旧合入写入额度缺少确切未应用回执，不能释放或刷新",
        )?;
        commit.witness(
            crate::platform_reconciliation::reconciliation_business_basis(
                &plan,
                &selected.resource,
            )?
            .versions()
            .iter()
            .cloned(),
        )?;
        let mut data = row.data.clone();
        data["status"] = json!("closed");
        data["release_source"] = json!(resources.execution.failure.as_ref().map(|f| &f.source_ref));
        commit.replace_record(&current, row, data)
    }

    pub(crate) fn require_completed_delivery_retry(
        &self,
        request: &PlatformToolRequest,
        resources: &WorkflowResources,
        receipt: &Value,
        plan: &mut PlatformPlan,
    ) -> Result<()> {
        require(
            request.caller.is_none(),
            "只有原工具步骤的明确重试可采用已完成交付",
        )?;
        let mut previous = resources.execution.retry_of.clone();
        let mut count = 0;
        while let Some(id) = previous {
            count += 1;
            require(count <= 32, "交付重试来源超过边界")?;
            let selected = one(self.store.as_ref(), "workflow_history_source", &id)?;
            let row = restored(self, &selected)?;
            let old: StepExecution = row.decode()?;
            require(
                row.r#type == "workflow_execution_history"
                    && row.immutable
                    && row.definition_id == id
                    && row.scope == resources.record.scope
                    && old.id == id
                    && old.workflow_instance_id == resources.record.id
                    && old.node_id == resources.execution.node_id
                    && old.plan_revision_id == resources.execution.plan_revision_id
                    && old.workflow_revision_id == resources.execution.workflow_revision_id
                    && old.contract_digest == resources.execution.contract_digest
                    && old.round == resources.execution.round
                    && old.input_digest == resources.execution.input_digest
                    && old.inputs == resources.execution.inputs,
                "交付重试改变了原节点、计划、合同或输入",
            )?;
            plan.basis = plan.basis.merge([version(&selected)])?;
            let failure = old
                .failure
                .as_ref()
                .ok_or_else(|| err("needs_reconciliation", "原交付缺少副作用分类"))?;
            require(
                matches!(
                    old.status,
                    StepExecutionStatus::Failed | StepExecutionStatus::Cancelled
                ),
                "原交付不是已终结执行",
            )?;
            if failure.effect == ExecutionFailureEffect::NotApplied {
                previous = old.retry_of;
                continue;
            }
            require(
                failure.kind == ExecutionFailureKind::Technical
                    && failure.effect == ExecutionFailureEffect::Applied,
                "原交付副作用仍未知",
            )?;
            let selected = one(
                self.store.as_ref(),
                "governance",
                old.tool_call_id.as_deref().unwrap_or(""),
            )?;
            let call = restored(self, &selected)?;
            let original: PlatformToolRequest =
                serde_json::from_value(call.data["request"].clone())
                    .map_err(|e| err("storage", e.to_string()))?;
            require(
                call.r#type == "platform_call"
                    && call.id == old.id
                    && call.scope.project_id() == Some(request.project_id.as_str())
                    && call.data["handler"] == "git.delivery.merge"
                    && call.data["root_instance_id"] == resources.instance.root_instance_id
                    && call.data["status"] == "succeeded"
                    && call.data["remote_effect"] == "applied"
                    && call.data["receipt"] == *receipt
                    && call.data["receipt_digest"] == configuration_hash(receipt)
                    && original.caller.is_none()
                    && original.execution_id == old.id
                    && original.instance_id == request.instance_id
                    && original.project_id == request.project_id
                    && original.owner_id == request.owner_id
                    && original.arguments == request.arguments
                    && original.definition_ref == request.definition_ref
                    && json!(original.definition) == json!(request.definition)
                    && receipt["synchronized"] == true,
                "原交付没有该确切请求的已核对合入和同步回执",
            )?;
            plan.basis = plan.basis.merge([
                version(&selected),
                self.platform_receipt_proof(
                    &resources.instance.root_instance_id,
                    "git.delivery.merge",
                    receipt,
                )?,
            ])?;
            return Ok(());
        }
        Err(err("needs_reconciliation", "缺少明确重试的原已完成交付"))
    }

    pub(crate) fn recover_reconciled_delivery_sync(
        &self,
        root: &crate::workflow_commits::WorkflowHeaderFacts,
        effect: &ConfigurationRecord,
        action: &DeliveryAction,
        attempt: &ConfigurationRecord,
        authority: &ReadSet,
    ) -> Result<Option<Value>> {
        if root.instance.cancel_requested {
            return Ok(None);
        }
        let DeliveryAction::Merge {
            context,
            publication,
            ..
        } = action
        else {
            return Ok(None);
        };
        let original: PlatformPlan = serde_json::from_value(effect.data["plan"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let stage_key = format!("platform-publish-stage:{}:merge", effect.id);
        let selected = one(self.store.as_ref(), "governance", &stage_key)?;
        if selected.value.is_none() {
            return Ok(None);
        };
        let stage = restored(self, &selected)?;
        let request = crate::platform_delivery::delivery_merge_request(context, publication)?;
        require(
            stage.r#type == "platform_publish_stage"
                && stage.immutable
                && stage.definition_id == effect.id
                && stage.data["issuer"] == "kernel_platform"
                && stage.data["action"] == json!(PublishAction::DeliveryMerge(request.clone())),
            "同步恢复失去原远端合入意图",
        )?;
        let (proof, value) = if let Some((proof, Ok(Some(value)))) =
            self.platform_observed(&stage, "recovery_merge")?
        {
            (proof, value)
        } else {
            let Some(value) = self.platform_publish_stage(
                effect,
                "merge",
                PublishAction::DeliveryMerge(request),
                true,
            )?
            else {
                return Ok(None);
            };
            let proof =
                self.platform_observe(&stage, "recovery_merge", &Ok(Some(value.clone())))?;
            (proof, value)
        };
        let receipt: PullRequestReceipt =
            serde_json::from_value(value).map_err(|e| err("adapter_contract", e.to_string()))?;
        require(
            receipt.merged
                && receipt.repository == context.workspace.repository
                && receipt.head == publication.revision
                && receipt.number == publication.pull_request.number
                && receipt.base_branch == context.target.branch
                && receipt.marker == publication.pull_request.marker,
            "同步恢复缺少确切 PR 的真实合入证明",
        )?;
        let revision = receipt
            .merge_commit
            .as_ref()
            .ok_or_else(|| err("adapter_contract", "远端缺少合入版本"))?;
        if !self
            .git
            .delivery_sync_ready(context, &publication.target_revision, revision)?
        {
            return Ok(None);
        };
        let call = one(self.store.as_ref(), "governance", &effect.definition_id)?;
        require(
            attempt.r#type == "workflow_reconciliation_attempt"
                && attempt.immutable
                && attempt.data["issuer"] == "kernel_reconciliation"
                && attempt.data["call_id"] == effect.definition_id,
            "同步恢复需要原调用的新核对授权",
        )?;
        let mut basis = crate::platform_reconciliation::reconciliation_business_basis(
            &original,
            &call.resource,
        )?
        .merge(authority.versions().iter().cloned())?
        .merge([
            version(&call),
            version(&selected),
            proof.clone(),
            ResourceVersion {
                resource: ResourceKey::new("governance", &effect.id),
                revision: effect.revision,
            },
            ResourceVersion {
                resource: ResourceKey::new("governance", &attempt.id),
                revision: attempt.revision,
            },
        ])?;
        if let Some(head) = &original.head {
            basis = basis.merge([version(head)])?;
        }
        let operation = crate::material_commands::stable_object_id(
            self.store.as_ref(),
            &attempt.id,
            "delivery-sync-continuation",
        )?;
        let action = DeliveryAction::Sync {
            context: context.clone(),
            publication: publication.clone(),
            receipt: Box::new(receipt),
            operation,
        };
        let key = ResourceKey::new(
            "governance",
            format!("delivery-sync-continuation:{}", attempt.id),
        );
        let existing = one(self.store.as_ref(), "governance", &key.id)?;
        let continuation = if existing.value.is_some() {
            let saved = restored(self, &existing)?;
            require(
                saved.r#type == "platform_effect_intent"
                    && saved.immutable
                    && saved.data["reconciliation_attempt"] == attempt.id
                    && saved.data["source_effect"] == effect.id
                    && saved.data["source_observation"] == proof.resource.id
                    && saved.data["plan"]["action"]
                        == json!(PlatformAction::Delivery(Box::new(action.clone()))),
                "同步恢复不能替换原授权或确切合入版本",
            )?;
            saved
        } else {
            let plan = PlatformPlan {
                action: PlatformAction::Delivery(Box::new(action.clone())),
                basis: basis.clone(),
                changes: vec![],
                head: None,
                fence: None,
                permit: None,
            };
            let mut row = record(
                "platform_effect_intent",
                effect.definition_id.clone(),
                effect.scope.clone(),
                "已核对远端合入的本地同步授权".into(),
                1,
                None,
                json!({"status":"granted","call_version":version(&call),"request_digest":effect.data["request_digest"],
                    "plan":plan,"issuer":"kernel_delivery_sync_recovery","source_effect":effect.id,
                    "source_observation":proof.resource.id,"reconciliation_attempt":attempt.id}),
                true,
            );
            row.id = key.id.clone();
            self.store.commit_changes(&PreparedChangeSet {
                command_id: key.id.clone(),
                fingerprint: configuration_hash(&row.data),
                read_set: basis.merge([version(&existing)])?,
                changes: vec![ResourceChange {
                    resource: key,
                    value: self.store.encode_runtime_record(&row)?,
                }],
                result: json!({"id":row.id}),
                time: now() as i64,
            })?;
            row
        };
        self.drive_delivery(&continuation, &action, false)
    }
}
