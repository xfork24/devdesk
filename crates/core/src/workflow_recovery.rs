//! A manager can reopen the producer of an independently observed test failure
//! inside its original frozen plan. Model prose is never a test receipt.
use crate::{
    platform_facts::{one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_facts::FactSelection,
    workflow_resources::{WorkflowResources, version},
    *,
};
use std::collections::BTreeSet;

pub(crate) struct TestRecovery {
    producer: String,
    report: Id,
    kind: &'static str,
    failed_test: Id,
    preserve_red: Option<String>,
    original_implementation: Option<StepExecution>,
}

impl Engine {
    pub(crate) fn test_recovery(
        &self,
        commit: &mut WorkflowCommit<'_>,
        resources: &WorkflowResources,
    ) -> Result<Option<TestRecovery>> {
        let instance = &resources.instance;
        let execution = &resources.execution;
        if execution.status != StepExecutionStatus::AwaitingAcceptance {
            return Ok(None);
        }
        let document = &instance.plan.document;
        let Some(step) = document.steps.iter().find(|s| s.key == execution.step) else {
            return Ok(None);
        };
        let approvals = one(self.store.as_ref(), "workflow_confirmations", &execution.id)?;
        commit.witness([version(&approvals)])?;
        if approvals
            .value
            .as_ref()
            .and_then(Value::as_array)
            .is_some_and(|rows| !rows.is_empty())
        {
            return Ok(None);
        }
        let StepExecutor::Tool {
            tool_ref,
            arguments,
            ..
        } = &step.executor
        else {
            return Ok(None);
        };
        if step.confirmation == ConfirmationPolicy::Human
            || !instance.plan.definitions.get(tool_ref).is_some_and(|tool| {
                tool.data["implementation"] == "platform"
                    && tool.data["qualified_name"] == "tests.run"
            })
        {
            return Ok(None);
        }
        let Some(call_id) = &execution.tool_call_id else {
            return Ok(None);
        };
        let call = one(self.store.as_ref(), "governance", call_id)?;
        if call.value.is_none() {
            return Ok(None);
        }
        let row = restored(self, &call)?;
        let receipt = &row.data["receipt"];
        if row.r#type != "platform_call"
            || row.data["status"] != "succeeded"
            || row.data["request"]["execution_id"] != execution.id
            || row.data["request"]["instance_id"] != resources.record.id
            || row.data["request"]["project_id"] != instance.project_id
            || receipt["status"] != "failed"
        {
            return Ok(None);
        }
        let independent_green = document
            .delivery_policy
            .as_ref()
            .is_some_and(|p| p.independent_green_step.as_deref() == Some(step.key.as_str()));
        let (producer_key, kind) = match receipt["failure_type"].as_str() {
            Some("environment") if !independent_green => {
                let Some(InputBinding::Source { source }) = arguments.get("recipe") else {
                    return Ok(None);
                };
                (&source.step, "test_support")
            }
            Some("assertion" | "environment")
                if independent_green
                    && (receipt["assertion_failed"] == true
                        || receipt["failure_type"] == "environment") =>
            {
                let Some(InputBinding::Source { source }) = arguments.get("workspace") else {
                    return Ok(None);
                };
                let Some(snapshot) = document.steps.iter().find(|s| s.key == source.step) else {
                    return Ok(None);
                };
                let StepExecutor::Tool { tool_ref, .. } = &snapshot.executor else {
                    return Ok(None);
                };
                if snapshot.dependencies.len() != 1
                    || !instance.plan.definitions.get(tool_ref).is_some_and(|tool| {
                        matches!(
                            tool.data["qualified_name"].as_str(),
                            Some("workspace.checkpoint" | "workspace.snapshot")
                        )
                    })
                {
                    return Ok(None);
                }
                (&snapshot.dependencies[0], "implementation")
            }
            _ => return Ok(None),
        };
        let Some(producer) = document.steps.iter().find(|s| &s.key == producer_key) else {
            return Ok(None);
        };
        if !matches!(producer.executor, StepExecutor::Agent { .. })
            || producer.confirmation == ConfirmationPolicy::Human
            || producer.access.writable_paths.is_empty()
            || !document.ancestors(step).contains(producer_key.as_str())
        {
            return Ok(None);
        }
        for report in execution.outputs.values() {
            let fact = one(self.store.as_ref(), "governance", report)?;
            let output = restored(self, &fact)?;
            if output.r#type == "workflow_output"
                && output.immutable
                && output.definition_id == execution.id
                && output.data["input_digest"] == execution.input_digest
                && output.data["provenance"]["kind"] == "platform"
                && output.data["provenance"]["tool"] == "tests.run"
                && output.data["provenance"]["call_id"] == *call_id
                && output.data["value"] == *receipt
            {
                commit.witness([version(&call), version(&fact)])?;
                return Ok(Some(TestRecovery {
                    producer: producer_key.clone(),
                    report: report.clone(),
                    kind,
                    failed_test: execution.id.clone(),
                    preserve_red: None,
                    original_implementation: None,
                }));
            }
        }
        Ok(None)
    }

    pub(crate) fn test_fixture_recovery(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        diagnosed: &WorkflowResources,
    ) -> Result<Option<TestRecovery>> {
        let execution = &diagnosed.execution;
        let Some(rework) = &execution.rework else {
            return Ok(None);
        };
        if execution.status != StepExecutionStatus::Failed
            || !execution
                .failure
                .as_ref()
                .is_some_and(|f| f.code == "needs_manager")
            || rework.assessment["kind"] != "implementation"
        {
            return Ok(None);
        }
        let document = &root.instance.plan.document;
        let Some(green) = document
            .delivery_policy
            .as_ref()
            .and_then(|p| p.independent_green_step.as_ref())
            .and_then(|key| document.steps.iter().find(|s| s.key == *key))
        else {
            return Ok(None);
        };
        let StepExecutor::Tool { arguments, .. } = &green.executor else {
            return Ok(None);
        };
        let Some(InputBinding::Source { source: recipe }) = arguments.get("recipe") else {
            return Ok(None);
        };
        let Some(producer) = document.steps.iter().find(|s| s.key == recipe.step) else {
            return Ok(None);
        };
        if !matches!(producer.executor, StepExecutor::Agent { .. })
            || producer.confirmation == ConfirmationPolicy::Human
            || producer.access.writable_paths.is_empty()
            || producer
                .access
                .writable_paths
                .iter()
                .any(|path| !std::path::Path::new(path).starts_with("tests"))
            || !document.ancestors(green).contains(execution.step.as_str())
        {
            return Ok(None);
        }
        let failed_test = text(&rework.assessment, "failed_execution_id")?;
        let report_id = text(&rework.assessment, "failed_test_report_id")?;
        let historic = one(self.store.as_ref(), "workflow_history_source", &failed_test)?;
        let historic_row = restored(self, &historic)?;
        let report = one(self.store.as_ref(), "governance", &report_id)?;
        let output = restored(self, &report)?;
        if historic_row.r#type != "workflow_execution_history"
            || historic_row.data["workflow_instance_id"] != root.record.id
            || historic_row.data["step"] != green.key
            || historic_row.data["plan_revision_id"] != root.instance.active_plan_revision_id
            || output.r#type != "workflow_output"
            || !output.immutable
            || output.definition_id != failed_test
            || !historic_row.data["outputs"]
                .as_object()
                .is_some_and(|ports| ports.values().any(|id| id == &report_id))
            || output.scope != root.record.scope
            || output.data["provenance"]["tool"] != "tests.run"
            || output.data["value"]["failure_type"] != "assertion"
        {
            return Ok(None);
        }
        let implementation = document
            .steps
            .iter()
            .find(|s| s.key == execution.step)
            .ok_or_else(|| err("storage", "缺少原实现合同"))?;
        let original = one(
            self.store.as_ref(),
            "workflow_history_source",
            &rework.previous_execution_id,
        )?;
        let original_row = restored(self, &original)?;
        let original_implementation: StepExecution = original_row.decode()?;
        let head = one(self.store.as_ref(), "head", &root.instance.owner_id)?;
        if original_row.r#type != "workflow_execution_history"
            || original_row.scope != root.record.scope
            || original_implementation.status != StepExecutionStatus::Completed
            || original_implementation.id != rework.previous_execution_id
            || original_implementation.node_id != execution.node_id
            || original_implementation.contract_digest != execution.contract_digest
            || original_implementation.plan_revision_id != execution.plan_revision_id
            || original_implementation.workflow_instance_id != execution.workflow_instance_id
            || !head
                .value
                .as_ref()
                .is_some_and(|r| r["revision"] == output.data["value"]["candidate"])
        {
            return Ok(None);
        }
        commit.witness([version(&original), version(&head)])?;
        for definition in &document.steps {
            let StepExecutor::Tool {
                tool_ref,
                arguments,
                ..
            } = &definition.executor
            else {
                continue;
            };
            if !root
                .instance
                .plan
                .definitions
                .get(tool_ref)
                .is_some_and(|tool| tool.data["qualified_name"] == "tests.run")
                || !matches!(arguments.get("expect_assertion_failure"), Some(InputBinding::Constant{value}) if value==true)
                || !matches!(arguments.get("recipe"), Some(InputBinding::Source{source}) if source.step==recipe.step && source.port==recipe.port && source.pointer==recipe.pointer)
                || !document
                    .ancestors(implementation)
                    .contains(definition.key.as_str())
            {
                continue;
            }
            let red = root.step(self, &definition.key)?;
            if red.execution.status != StepExecutionStatus::Completed {
                continue;
            }
            let Some(call_id) = red.execution.tool_call_id.as_deref() else {
                continue;
            };
            let selected = one(self.store.as_ref(), "governance", call_id)?;
            let row = restored(self, &selected)?;
            let receipt = &row.data["receipt"];
            if row.r#type != "platform_call"
                || row.data["status"] != "succeeded"
                || row.data["request"]["instance_id"] != root.record.id
                || row.data["request"]["execution_id"] != red.execution.id
                || receipt["status"] != "succeeded"
                || receipt["expected_failure"] != true
                || receipt["assertion_failed"] != true
            {
                continue;
            }
            commit.witness([
                version(&historic),
                version(&report),
                version(&selected),
                version(&red.step),
            ])?;
            return Ok(Some(TestRecovery {
                producer: recipe.step.clone(),
                report: report_id,
                kind: "test_support",
                failed_test,
                preserve_red: Some(definition.key.clone()),
                original_implementation: Some(original_implementation),
            }));
        }
        Ok(None)
    }

    pub(crate) fn prepare_test_recovery(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        failed: &WorkflowResources,
        recovery: &TestRecovery,
    ) -> Result<StepExecution> {
        let document = &root.instance.plan.document;
        let failed_definition = document
            .steps
            .iter()
            .find(|s| s.key == failed.execution.step)
            .ok_or_else(|| err("storage", "失败步骤合同缺失"))?;
        let mut selection = FactSelection::default();
        selection.rule(&failed_definition.completion);
        let facts = failed.selected_facts(self.store.as_ref(), &selection)?;
        commit.witness(facts.reads.versions().iter().cloned())?;
        require(
            recovery.preserve_red.is_some()
                || !commit.rule(
                    &root.instance,
                    &failed_definition.node_id,
                    &failed_definition.completion,
                    &facts.facts,
                )?,
            "测试结果已满足冻结完成条件",
        )?;
        let mut impacted = document.impacted_nodes(&BTreeSet::from([recovery.producer.clone()]));
        if let Some(red) = &recovery.preserve_red {
            impacted.remove(red);
        }
        if recovery.original_implementation.is_some() {
            impacted.remove(&failed.execution.step);
        }
        let downstream = document.impacted_nodes(&BTreeSet::from([failed.execution.step.clone()]));
        for current in root
            .instance
            .steps
            .iter()
            .filter(|e| impacted.contains(&e.step))
        {
            let definition = document
                .steps
                .iter()
                .find(|s| s.key == current.step)
                .ok_or_else(|| err("storage", "恢复步骤合同缺失"))?;
            require(
                current.id == failed.execution.id
                    || current.started.is_none()
                    || (definition.confirmation != ConfirmationPolicy::Human
                        && !downstream.contains(&current.step)),
                "技术恢复不能使后续已执行的操作或人工确认失效",
            )?;
            if recovery.kind == "test_support"
                && recovery.preserve_red.is_none()
                && current.started.is_some()
                && current.id != failed.execution.id
            {
                require(
                    current.step == recovery.producer
                        || matches!(definition.executor, StepExecutor::Tool { .. }),
                    "测试支持恢复不能重做已执行的实现或评审",
                )?;
            }
            let selected = root.step(self, &current.id)?;
            self.require_logical_retry_budget(commit, &selected, definition)?;
            if current.started.is_some() && current.id != failed.execution.id {
                let confirmations =
                    one(self.store.as_ref(), "workflow_confirmations", &current.id)?;
                commit.witness([version(&confirmations)])?;
                require(
                    !confirmations
                        .value
                        .as_ref()
                        .and_then(Value::as_array)
                        .is_some_and(|rows| !rows.is_empty()),
                    "技术恢复不能重做已经人工确认的执行",
                )?;
            }
        }
        let producer = root.step(self, &recovery.producer)?;
        let assessment = json!({
            "kind":recovery.kind,
            "failed_execution_id":recovery.failed_test,
            "failed_test_report_id":recovery.report,
            "preserved_red_step":recovery.preserve_red,
            "instructions":if recovery.preserve_red.is_some() {
                "A previous implementation attempt diagnosed the independent Green fixture. Verify the exact report and original test contract. Correct only fixture dependencies, setup, or transport in the original test directory. Preserve all assertions, expected results, case selection, interfaces and the recipe. The original accepted Red stays available as history and the platform will run a fresh Green on the corrected fixture."
            } else if recovery.kind=="test_support" {
                "Read the exact independent test report. Repair test dependencies, setup or support within your original writable paths. Preserve all test cases, expected outcomes, declared interfaces, test selection, and isolation. Do not change product behavior. The platform will rerun the independent test."
            } else {
                "Read the exact independent Green report. Repair only the implementation within the confirmed scope and the original writable paths. Preserve the tests and acceptance criteria. The platform will seal and independently retest the repaired implementation."
            },
        });
        let replacement = self.prepare_native_retry_with_assessment(
            commit,
            root,
            &producer,
            true,
            Some(&impacted),
            Some(&assessment),
        )?;
        if let Some(original) = &recovery.original_implementation {
            self.carry_forward_fixture_implementation(commit, root, failed, original, recovery)?;
        }
        let decisions = one(
            self.store.as_ref(),
            "workflow_root_decisions",
            &root.instance.root_instance_id,
        )?;
        commit.witness([version(&decisions)])?;
        for candidate in decisions
            .value
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if candidate["status"] != "pending"
                || candidate["operation"] != "workflow.change.apply"
                || candidate["instance_id"] != root.record.id
            {
                continue;
            }
            let selected = one(self.store.as_ref(), "governance", &text(candidate, "id")?)?;
            let pending = restored(self, &selected)?;
            let request = &pending.data["request"];
            let interaction = &request["interaction"];
            let references_failure = ["evidence", "materials"].into_iter().any(|list| {
                interaction[list]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|item| {
                        item["execution_id"] == recovery.failed_test
                            && item["output_id"] == recovery.report
                            && item["instance_id"] == root.record.id
                    })
            });
            if !references_failure
                || pending.r#type != "workflow_pending_decision"
                || pending.definition_id != root.record.id
                || pending.data["status"] != "pending"
                || !pending.data["decision_ref"].is_null()
                || pending.data["plan_revision_id"] != root.instance.active_plan_revision_id
                || request["operation"] != "workflow.change.apply"
            {
                continue;
            }
            let mut data = pending.data.clone();
            data["status"] = json!("withdrawn");
            data["resolution_reason"] = json!("原测试报告已由冻结计划内的经理恢复接手");
            data["recovery_execution_id"] = json!(replacement.id);
            commit.replace_record(&selected, pending, data)?;
        }
        Ok(replacement)
    }

    fn carry_forward_fixture_implementation(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        failed: &WorkflowResources,
        original: &StepExecution,
        recovery: &TestRecovery,
    ) -> Result<()> {
        use crate::concurrency::{ResourceChange, ResourceKey, ResourceVersion};
        require(
            !commit.execution_active(failed)? && !commit.archived_execution_active(original)?,
            "测试夹具恢复不能接续活动写者或未知副作用",
        )?;
        let target = one(
            self.store.as_ref(),
            "workflow_step",
            &json!([root.record.id, original.id]).to_string(),
        )?;
        require(
            target.value.is_none() && target.revision > 0,
            "原实现已不属于可接续的归档执行",
        )?;
        let key = ResourceKey::new("workflow_step_restore", failed.step.resource.id.clone());
        let (attempts, _) = self.logical_retries(failed)?;
        commit.witness(failed.reads.versions().iter().cloned())?;
        commit.witness([
            version(&attempts),
            version(&target),
            ResourceVersion {
                resource: key.clone(),
                revision: failed.step.revision,
            },
        ])?;
        commit.change(ResourceChange {
            resource: key,
            value: self.store.encode_runtime_value(&json!(original))?,
        })?;
        let mut history = crate::governance::record(
            "workflow_execution_history",
            failed.execution.id.clone(),
            root.record.scope.clone(),
            failed.execution.step.clone(),
            1,
            None,
            json!(failed.execution),
            true,
        );
        history.refs.insert(
            "physically_started".into(),
            json!(
                attempts
                    .value
                    .as_ref()
                    .is_some_and(|v| v["current_started"] == true)
            ),
        );
        history.content_hash = content_digest(&history.refs, &history.data);
        commit.new_record(
            history,
            &format!("workflow-history:{}", failed.execution.id),
        )?;
        commit.new_record(crate::governance::record("workflow_carry_forward",original.id.clone(),root.record.scope.clone(),
            original.step.clone(),1,None,json!({"from_plan":root.instance.active_plan_revision_id,"to_plan":root.instance.active_plan_revision_id,
                "source_plan":original.plan_revision_id,"execution_id":original.id,"contract_digest":original.contract_digest,
                "input_digest":original.input_digest,"outputs":original.outputs,"fixture_recovery_report":recovery.report,
                "failed_diagnosis_execution_id":failed.execution.id}),true), &format!("carry-forward:{}",original.id))?;
        Ok(())
    }

    pub(crate) fn native_test_recovery_evidence(
        &self,
        commit: &mut WorkflowCommit<'_>,
        execution: &StepExecution,
    ) -> Result<Value> {
        let Some(rework) = &execution.rework else {
            return Ok(Value::Null);
        };
        let Some(id) = rework.assessment["failed_test_report_id"].as_str() else {
            return Ok(Value::Null);
        };
        if !matches!(
            rework.assessment["kind"].as_str(),
            Some("test_support" | "implementation")
        ) {
            return Ok(Value::Null);
        }
        let source = one(self.store.as_ref(), "governance", id)?;
        let report = restored(self, &source)?;
        require(
            report.r#type == "workflow_output"
                && report.immutable
                && report.data["execution_id"] == rework.assessment["failed_execution_id"]
                && report.data["provenance"]["tool"] == "tests.run",
            "无法核对恢复步骤的原始测试报告",
        )?;
        commit.witness([version(&source)])?;
        let value = &report.data["value"];
        let mut details = Vec::new();
        for line in value["report"].as_str().unwrap_or("").lines() {
            let Ok(event) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let diagnostic = &event["diagnostic"];
            let message = match event["event"].as_str() {
                Some("fail") => diagnostic
                    .pointer("/details/error/message")
                    .and_then(Value::as_str),
                Some("diagnostic") => diagnostic["message"]
                    .as_str()
                    .filter(|s| s.contains("Error") || s.contains("MODULE_NOT_FOUND")),
                _ => None,
            };
            if let Some(message) = message {
                details.push(json!({"test":diagnostic["name"],"file":diagnostic["file"],"message":message.chars().take(512).collect::<String>()}));
                if details.len() == 8 {
                    break;
                }
            }
        }
        let mut evidence = json!({"report_id":id,"candidate":value["candidate"],"kind":value["failure_type"],
            "counts":value["counts"],"diagnostics":details});
        if rework.assessment["kind"] == "test_support" {
            for output in rework.outputs.values() {
                let selected = one(self.store.as_ref(), "governance", output)?;
                let original = restored(self, &selected)?;
                let value = &original.data["value"];
                if original.r#type == "workflow_output"
                    && original.immutable
                    && original.definition_id == rework.previous_execution_id
                    && value["recipe"].is_object()
                {
                    let contract = json!({"cases":value["cases"],"interfaces":value["interfaces"],"recipe":value["recipe"]});
                    let usage = if contract.to_string().len() <= 16384 {
                        contract
                    } else {
                        Value::Null
                    };
                    evidence["original_test_contract"] =
                        json!({"output_id":output,"contract":usage});
                    commit.witness([version(&selected)])?;
                    break;
                }
            }
        }
        Ok(evidence)
    }

    pub(crate) fn test_recovery_view(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
        definition: &StepDefinition,
    ) -> Result<Value> {
        let Some(recovery) = self.test_recovery(commit, resources)? else {
            return Ok(Value::Null);
        };
        let producer = root.step(self, &recovery.producer)?;
        let producer_step = root
            .instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == recovery.producer)
            .ok_or_else(|| err("storage", "恢复执行者合同缺失"))?;
        let used = self.logical_retries(resources)?.1;
        let producer_used = self.logical_retries(&producer)?.1;
        let remaining = definition.budget.retry_limit.saturating_sub(used).min(
            producer_step
                .budget
                .retry_limit
                .saturating_sub(producer_used),
        );
        Ok(json!({
            "status":if remaining==0 {"exhausted"} else {"needs_manager"},
            "kind":recovery.kind,"producer":recovery.producer,"report_id":recovery.report,
            "operation":"workflow.step.rework","remaining":remaining,
            "human_required":remaining==0,"escalation_id":if remaining==0 {Some(&resources.execution.id)}else{None}
        }))
    }
}
