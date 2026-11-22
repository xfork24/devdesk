//! Exact-result exceptions. A manager can propose, only Human can sign.
use crate::{
    platform_facts::{one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_facts::FactSelection,
    workflow_resources::{WorkflowResources, version},
    *,
};

pub(crate) fn require_exception_hard_rules(
    rule: &CompletionRule,
    facts: &WorkflowFacts,
) -> Result<()> {
    match rule {
        CompletionRule::All { rules } | CompletionRule::Any { rules } => {
            for rule in rules {
                require_exception_hard_rules(rule, facts)?;
            }
            Ok(())
        }
        CompletionRule::Independent { .. }
        | CompletionRule::Before { .. }
        | CompletionRule::Same { .. }
        | CompletionRule::Outcome { .. } => require(
            rule.evaluate(facts),
            "人工例外不能替代身份独立性、版本、顺序或真实执行结果",
        ),
        _ => Ok(()),
    }
}

impl Engine {
    pub(crate) fn review_recovery(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
    ) -> Result<Value> {
        let e = &resources.execution;
        if !matches!(
            e.status,
            StepExecutionStatus::AwaitingAcceptance | StepExecutionStatus::WaitingConfirmation
        ) {
            return Ok(Value::Null);
        }
        let Some(policy) = root
            .instance
            .plan
            .document
            .review_policies
            .values()
            .find(|p| p.review_step == e.step)
        else {
            return Ok(Value::Null);
        };
        let mut selection = FactSelection::default();
        selection.rule(&policy.rejected_when);
        let facts = resources.selected_facts(self.store.as_ref(), &selection)?;
        commit.witness(facts.reads.versions().iter().cloned())?;
        if !policy.rejected_when.evaluate(&facts.facts) {
            return Ok(Value::Null);
        }
        let (selected, budget) = self.rework_budget_view(root, &policy.rework_policy)?;
        commit.witness([version(&selected)])?;
        let history = self.review_repair_history(root, &policy.rework_policy)?;
        commit.witness([version(&history)])?;
        let exhausted = budget["history_baseline_known"] == true && budget["remaining"] == 0;
        Ok(
            json!({"kind":"review_rework","status":if exhausted {"exhausted"} else {"needs_manager"},
            "operation":"workflow.rework.preview","budget":budget,"remaining":budget["remaining"],
            "human_required":exhausted,"allow_human_approval":policy.allow_human_approval,"reports":e.outputs,"history":history.value,
            "reason":"独立评审未通过，请按原报告定位责任步骤并核对真实返工依据"}),
        )
    }

    pub(crate) fn native_approval_basis(
        &self,
        commit: &mut WorkflowCommit<'_>,
        resources: &WorkflowResources,
    ) -> Result<Value> {
        let e = &resources.execution;
        let doc = &resources.instance.plan.document;
        require(
            doc.review_policies.is_empty()
                || doc
                    .review_policies
                    .values()
                    .any(|p| p.review_step == e.step),
            "只有 Workflow 声明的业务评审可申请人工例外；真实测试和平台回执不能由经理申请放行",
        )?;
        let step = doc
            .steps
            .iter()
            .find(|s| s.key == e.step)
            .ok_or_else(|| err("storage", "评审步骤缺失"))?;
        let mut selection = FactSelection::default();
        selection.rule(&step.completion);
        let facts = resources.selected_facts(self.store.as_ref(), &selection)?;
        commit.witness(facts.reads.versions().iter().cloned())?;
        require_exception_hard_rules(&step.completion, &facts.facts)?;
        if let Some(policy) = doc
            .review_policies
            .values()
            .find(|p| p.review_step == e.step)
        {
            require(
                policy.allow_human_approval,
                "Workflow 未允许此评审申请人工例外",
            )?;
            let mut selection = FactSelection::default();
            selection.rule(&policy.rejected_when);
            let facts = resources.selected_facts(self.store.as_ref(), &selection)?;
            commit.witness(facts.reads.versions().iter().cloned())?;
            require(
                policy.rejected_when.evaluate(&facts.facts),
                "当前评审不符合拒绝条件",
            )?;
        }
        let mut reports = Vec::new();
        for id in e.outputs.values() {
            let selected = one(self.store.as_ref(), "governance", id)?;
            let output = restored(self, &selected)?;
            require(
                output.r#type == "workflow_output"
                    && output.immutable
                    && output.data["execution_id"] == e.id
                    && output.data["input_digest"] == e.input_digest,
                "例外报告来源不符",
            )?;
            commit.witness([version(&selected)])?;
            reports.push(json!({"id":id,"value_digest":configuration_hash(&output.data["value"])}));
        }
        require(!reports.is_empty(), "人工例外需要确切评审报告")?;
        let input_values = e
            .inputs
            .iter()
            .map(|(name, value)| (name.clone(), configuration_hash(value)))
            .collect::<std::collections::BTreeMap<_, _>>();
        fn code_references(value: &Value, out: &mut Vec<Value>) {
            if out.len() >= 64 {
                return;
            }
            match value {
                Value::Object(fields) => {
                    if fields.contains_key("owner_id") && fields.contains_key("revision") {
                        out.push(Value::Object(
                            fields
                                .iter()
                                .filter(|(k, _)| {
                                    [
                                        "owner_id",
                                        "workspace_id",
                                        "revision",
                                        "candidate_id",
                                        "target_revision",
                                    ]
                                    .contains(&k.as_str())
                                })
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect(),
                        ));
                    } else {
                        for value in fields.values() {
                            code_references(value, out);
                        }
                    }
                }
                Value::Array(items) => {
                    for value in items {
                        code_references(value, out);
                    }
                }
                _ => {}
            }
        }
        let mut code = Vec::new();
        for value in e.inputs.values() {
            code_references(value, &mut code);
        }
        Ok(
            json!({"instance_id":resources.record.id,"plan_revision_id":resources.instance.active_plan_revision_id,
            "execution_id":e.id,"input_digest":e.input_digest,"outputs":e.outputs,"reports":reports,
            "input_value_digests":input_values,"code_references":code,"contract_digest":e.contract_digest}),
        )
    }
}
