//! Reconciliation may authorize local sync after positive remote merge proof.
//! This continuation has no operation capable of publishing or merging a PR.
use crate::{
    concurrency::*,
    governance::record,
    platform_delivery::DeliveryAction,
    platform_facts::{PlatformAction, PlatformPlan, one, restored},
    platform_publish::PublishAction,
    workflow_resources::{WorkflowResources, version},
    *,
};

impl Engine {
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
