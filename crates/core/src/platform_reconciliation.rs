//! A new explicit inspection may adopt a positive original physical receipt.
//! It cannot dispatch the old operation or overwrite newer business resources.
use crate::{
    concurrency::*,
    governance::update_record,
    platform_facts::{PlatformAction, PlatformPlan, one, restored},
    workflow_commits::WorkflowCommit,
    workflow_resources::version,
    *,
};

pub(crate) fn reconciliation_business_basis(
    plan: &PlatformPlan,
    call: &ResourceKey,
) -> Result<ReadSet> {
    ReadSet::new(
        plan.basis
            .versions()
            .iter()
            .filter(|v| {
                !matches!(
                    v.resource.kind.as_str(),
                    "workflow_header"
                        | "workflow_step"
                        | "workflow_run_authority"
                        | "workflow_step_authority"
                        | "tool_turn"
                        | "turn"
                        | "execution"
                        | "session"
                        | "session_live_workers"
                        | "session_input_state"
                ) && v.resource != *call
            })
            .cloned()
            .collect(),
    )
}

impl Engine {
    pub(crate) fn inspect_platform_reconciliation(
        &self,
        effect: &ConfigurationRecord,
        action: &PlatformAction,
    ) -> Result<Option<Value>> {
        require(
            effect.r#type == "platform_effect_intent" && effect.immutable,
            "核对需要原不可变执行意图",
        )?;
        match action {
            PlatformAction::ReusedPublication { receipt, .. } => Ok(Some(receipt.clone())),
            PlatformAction::Value(value) => Ok(Some(value.clone())),
            PlatformAction::WorkspaceCurrent { context, workspace } => Ok(Some(
                json!({"workspace_id":workspace.id,"revision":self.git.workspace_revision(context)?,"owner_id":workspace.owner_id}),
            )),
            PlatformAction::Test(request) => match self.tests.inspect_test(request)? {
                ports::TestExecutionObservation::Completed { result } => Ok(Some(result)),
                ports::TestExecutionObservation::Running { .. } => Ok(None),
                _ => Err(err(
                    "needs_reconciliation",
                    "原测试缺少实际完成回执；核对不重新启动测试",
                )),
            },
            PlatformAction::Publish { .. } => self.inspect_platform_publication(effect, action),
            PlatformAction::Delivery(action) => self.drive_delivery(effect, action, true),
            _ => self.perform_platform_action(action, false),
        }
    }
    pub(crate) fn reconciled_platform_record(
        &self,
        call: &ConfigurationRecord,
        request: &ConfigurationRecord,
        observed: &ResourceVersion,
        receipt: &Value,
    ) -> ConfigurationRecord {
        let mut row = call.clone();
        let mut data = row.data.clone();
        data["status"] = json!("succeeded");
        data["receipt"] = receipt.clone();
        data["receipt_digest"] = json!(configuration_hash(receipt));
        data["remote_effect"] = json!("applied");
        data["reconciliation_id"] = json!(request.id);
        data["reconciliation_observation_id"] = json!(observed.resource.id);
        data["finished"] = json!(now());
        update_record(&mut row, data);
        row
    }
    pub(crate) fn prepare_platform_reconciliation_admission(
        &self,
        commit: &mut WorkflowCommit<'_>,
        selected: &ResourceSnapshot,
        call: &ConfigurationRecord,
        request: &ConfigurationRecord,
        observed: &ResourceVersion,
        receipt: &Value,
    ) -> Result<()> {
        require(
            call.r#type == "platform_call"
                && call.scope == request.scope
                && request.r#type == "workflow_reconciliation"
                && call.data["request"]["instance_id"] == request.data["instance_id"],
            "显式核对不能换原调用或作用域",
        )?;
        let frozen: PlatformToolRequest = serde_json::from_value(call.data["request"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        require(
            frozen.step_execution_id() == request.data["execution_id"].as_str().unwrap_or(""),
            "显式核对不能替换原执行",
        )?;
        self.tool_executor
            .as_ref()
            .ok_or_else(|| err("unsupported", "缺少输出校验器"))?
            .validate_arguments(&frozen.definition.output_schema, receipt)?;
        commit.witness([version(selected), observed.clone()])?;
        if call.data["status"] == "succeeded" {
            require(
                call.data["receipt"] == *receipt,
                "核对不能替换已接纳的原回执",
            )?;
            return Ok(());
        }
        let effect_snapshot = one(
            self.store.as_ref(),
            "governance",
            &text(&call.data, "effect_id")?,
        )?;
        let effect = restored(self, &effect_snapshot)?;
        require(
            effect.r#type == "platform_effect_intent"
                && effect.immutable
                && effect.definition_id == call.id
                && effect.data["request_digest"] == configuration_hash(&frozen),
            "核对失去原冻结意图",
        )?;
        let plan: PlatformPlan = serde_json::from_value(effect.data["plan"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        self.validate_platform_result(&plan.action, receipt)?;
        // The new reconciliation grant supplies its own explicit workflow and
        // lifecycle witnesses. Every original business/source/target version is
        // retained: adopting a physical receipt never rebases a Task or head.
        commit.witness(
            reconciliation_business_basis(&plan, &selected.resource)?
                .versions()
                .iter()
                .cloned(),
        )?;
        commit.witness([version(&effect_snapshot)])?;
        if let PlatformAction::Delivery(action) = &plan.action
            && let Some(change) = self.delivery_admission_change(action, receipt)?
        {
            commit.change(change)?;
        }
        for change in &plan.changes {
            commit.change(change.clone())?;
        }
        if let Some(head) = &plan.head {
            let mut next = head
                .value
                .clone()
                .ok_or_else(|| err("not_found", "原代码头缺失"))?;
            next["revision"] = json!(text(receipt, "revision")?);
            commit.change(ResourceChange {
                resource: head.resource.clone(),
                value: next,
            })?;
        }
        if let Some(fence) = &plan.fence {
            let mut closed = fence.clone();
            let mut data = closed.data.clone();
            data["status"] = json!("closed");
            data["reconciliation_id"] = json!(request.id);
            data["observation_id"] = json!(observed.resource.id);
            update_record(&mut closed, data);
            commit.change(ResourceChange {
                resource: ResourceKey::new("governance", &closed.id),
                value: closed.to_storage(),
            })?;
        }
        let next = self.reconciled_platform_record(call, request, observed, receipt);
        commit.change(ResourceChange {
            resource: selected.resource.clone(),
            value: self.store.encode_runtime_record(&next)?,
        })?;
        if let PlatformAction::Test(test) = &plan.action {
            self.prepare_platform_test_cleanup(&effect, test, observed)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renewed_reconciliation_authority_never_rebases_business_or_writer_guards() {
        let call = ResourceKey::new("governance", "original-call");
        let versions = [
            ("workflow_header", "run", 2),
            ("workflow_step", "step", 3),
            ("governance", "original-call", 4),
            ("head", "target", 17),
            ("governance", "workspace-fence", 1),
            ("conversation_writers", "project", 8),
            ("catalog_clock", "configuration", 12),
        ]
        .map(|(kind, id, revision)| ResourceVersion {
            resource: ResourceKey::new(kind, id),
            revision,
        });
        let plan = PlatformPlan {
            domain_checks: Vec::new(),
            action: PlatformAction::Value(Value::Null),
            basis: ReadSet::new(versions.to_vec()).unwrap(),
            changes: vec![],
            head: None,
            fence: None,
            permit: None,
        };
        let business = reconciliation_business_basis(&plan, &call).unwrap();
        assert_eq!(business.versions().len(), 4);
        assert!(
            business
                .merge([
                    ResourceVersion {
                        resource: ResourceKey::new("workflow_header", "run"),
                        revision: 5
                    },
                    ResourceVersion {
                        resource: call,
                        revision: 6
                    },
                ])
                .is_ok()
        );
        for (kind, id, revision) in [
            ("head", "target", 18),
            ("governance", "workspace-fence", 2),
            ("conversation_writers", "project", 9),
            ("catalog_clock", "configuration", 13),
        ] {
            assert!(
                business
                    .merge([ResourceVersion {
                        resource: ResourceKey::new(kind, id),
                        revision
                    }])
                    .is_err()
            );
        }
    }
}
