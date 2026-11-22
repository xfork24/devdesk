//! Bounded control queries and readiness use actual selected workflow facts.
//! Display eligibility is advisory; every command repeats authoritative CAS.
use crate::{
    platform_facts::{one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_eligibility::{WorkflowActionEligibility as Eligibility, WorkflowStepActions},
    workflow_facts::FactSelection,
    workflow_resources::WorkflowResources,
    *,
};
impl Engine {
    pub(crate) fn native_step_readiness(
        &self,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
    ) -> Result<Value> {
        let c = Command {
            command_id: "readiness-only".into(),
            target: Some(root.record.id.clone()),
            expected_revision: Some(root.record.revision),
            payload: json!({}),
        };
        let context = InvocationContext::workflow(
            WorkflowActor::Coordinator {
                source_id: root.record.id.clone(),
            },
            Some(root.instance.project_id.clone()),
        );
        let mut selected = WorkflowCommit::new(self, "query.workflow.readiness", &c, &context)?;
        let e = &resources.execution;
        let step = root
            .instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.node_id == e.node_id)
            .ok_or_else(|| err("storage", "原步骤合同缺失"))?;
        let pending = one(
            self.store.as_ref(),
            "workflow_change_pending",
            &root.record.id,
        )?;
        let common = if root.instance.cancel_requested
            || matches!(
                root.instance.status,
                StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
            ) {
            Some(Eligibility::blocked("terminal", "流程已结束或取消"))
        } else if pending.value.is_some() {
            Some(Eligibility::blocked("change_pending", "运行计划正在变更"))
        } else if let Err(error) = self.require_execution_time(resources) {
            Some(Eligibility::blocked(&error.code, error.message))
        } else {
            None
        };
        let mut selection = FactSelection::default();
        selection.rule(&step.condition);
        selection.rule(&step.completion);
        selection.approval(&step.key);
        for binding in step.inputs.values() {
            selection.binding(binding);
        }
        let facts = resources.selected_facts(self.store.as_ref(), &selection)?;
        let (attempts, retry_count) = self.logical_retries(resources)?;
        let advisory = |result: Result<()>| match result {
            Ok(()) => Eligibility::ready(),
            Err(error) => Eligibility::blocked(&error.code, error.message),
        };
        let mut input_gap = None;
        let dispatch = if let Some(blocked) = &common {
            blocked.clone()
        } else if e.status != StepExecutionStatus::Queued {
            Eligibility::blocked("execution_state", "步骤已派发或结束")
        } else {
            advisory((|| {
                selected.require_dispatch(root, resources)?;
                if step.condition.evaluate(&facts.facts)
                    && let Err(error) =
                        crate::configured_runtime::material_inputs(step, &facts.facts)
                {
                    input_gap = Some(error.message.clone());
                    return Err(error);
                }
                Ok(())
            })())
        };
        let accept = if let Some(blocked) = &common {
            blocked.clone()
        } else {
            advisory(selected.require_acceptance(resources, &facts.facts))
        };
        let mut retry = if let Some(blocked) = &common {
            blocked.clone()
        } else if !matches!(
            e.status,
            StepExecutionStatus::Failed | StepExecutionStatus::Cancelled
        ) {
            Eligibility::blocked("execution_state", "步骤不在技术失败或取消状态")
        } else if attempts
            .value
            .as_ref()
            .is_some_and(|v| v["started"].as_u64().unwrap_or(0) > 0)
            && retry_count >= step.budget.retry_limit
        {
            Eligibility::blocked("retry_budget", "技术重试额度已满")
        } else if e
            .failure
            .as_ref()
            .is_some_and(|f| f.effect == ExecutionFailureEffect::Unknown)
        {
            Eligibility::blocked("needs_reconciliation", "未知副作用需先核对")
        } else {
            advisory((|| {
                require(
                    !selected.execution_active(resources)?,
                    "原执行仍有活动写者或未核对副作用",
                )
            })())
        };
        let completion = selected.rule(
            &root.instance,
            &step.node_id,
            &step.completion,
            &facts.facts,
        )?;
        let approvals = one(self.store.as_ref(), "workflow_confirmations", &e.id)?;
        let matching = approvals
            .value
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|r| {
                r["data"]["input_digest"] == e.input_digest
                    && r["data"]["outputs"] == json!(e.outputs)
            })
            .collect::<Vec<_>>();
        let overridden = matching
            .iter()
            .any(|r| r["data"]["override_completion"] == true);
        let waiver = one(
            self.store.as_ref(),
            "workflow_waiver",
            &json!([
                root.instance.active_plan_revision_id,
                step.node_id,
                "confirmation"
            ])
            .to_string(),
        )?;
        let waived = selected.waiver(&root.instance, &step.node_id, "confirmation")?;
        let confirmation = matching.first().map(|row| &row["id"]);
        let human =
            step.confirmation != ConfirmationPolicy::Human || confirmation.is_some() || waived;
        let disposition = if confirmation.is_some() {
            "confirmed"
        } else if waived {
            "waived"
        } else if step.confirmation == ConfirmationPolicy::Human {
            "pending"
        } else {
            "not_required"
        };
        let mut delivery_refresh = Value::Null;
        if let Some(policy) = &root.instance.plan.document.delivery_policy
            && e.failure
                .as_ref()
                .is_some_and(|f| f.code == "stale_target" && f.automatic_retry_safe())
        {
            let published = root.step(self, &policy.source_publication_step)?;
            if published.execution.status == StepExecutionStatus::Completed {
                let reviewed = root.step(self, &policy.code_review_step)?;
                let source = text(&reviewed.execution.inputs["workspace"], "revision")?;
                let budget = one(
                    self.store.as_ref(),
                    "governance",
                    &format!("delivery-refresh:{}:{source}", root.record.id),
                )?;
                let used = budget
                    .value
                    .as_ref()
                    .and_then(|r| r["data"]["used"].as_u64())
                    .unwrap_or(0);
                let remaining = step.budget.retry_limit.saturating_sub(used);
                if remaining == 0 {
                    retry = Eligibility::blocked(
                        "delivery_refresh_budget",
                        "当前源版本的目标刷新额度已耗尽",
                    );
                }
                delivery_refresh = json!({"operation":"workflow.step.retry","source_revision":source,"remaining":remaining,"retains_source_review":true,"renews_merge_regression_and_confirmation":true});
            }
        }
        let actions = WorkflowStepActions {
            dispatch,
            accept,
            retry,
        };
        let mut result = json!({"step":step.key,"node_id":step.node_id,"execution":e,"step_revision":resources.step.revision,"dispatchable":actions.dispatch.allowed,"actions":actions,"retry_count":retry_count,"input_gap":input_gap,"completion_satisfied":completion,"completion_overridden":overridden,"human_satisfied":human,"confirmation_status":disposition,"confirmation_ref":confirmation,"confirmation_waiver_ref":if waived{waiver.value.as_ref().map(|r|&r["id"])}else{None}});
        result["retry_limit"] = json!(step.budget.retry_limit);
        if !completion && !overridden {
            result["recovery"] = self.test_recovery_view(&mut selected, root, resources, step)?;
        }
        if e.failure
            .as_ref()
            .is_some_and(|failure| failure.code == "needs_manager")
        {
            let fixture = self.test_fixture_recovery(&mut selected, root, resources)?;
            let may_continue = actions.retry.allowed
                && e.session_id.is_some()
                && e.turn_id.is_some()
                && e.sequence < step.budget.max_turns;
            let exhausted = actions.retry.code == "retry_budget" && fixture.is_none();
            result["recovery"] = json!({
                "status":if actions.retry.allowed || fixture.is_some() {"needs_manager"} else if exhausted {"exhausted"} else {"blocked"},
                "kind":"agent_diagnosis",
                "operation":if !actions.retry.allowed && fixture.is_some() {"workflow.step.rework"} else {"workflow.step.retry"},
                "remaining":step.budget.retry_limit.saturating_sub(retry_count),
                "human_required":exhausted,
                "continue_operation":if may_continue {Some("workflow.step.input")} else {None},
                "fixture_repair":fixture.as_ref().map(|_|"workflow.step.rework"),
            });
        }
        result["delivery_refresh"] = delivery_refresh;
        if e.status == StepExecutionStatus::NeedsReconciliation
            && let Ok(recovery) = self.source_publication_recovery_view(root, resources)
            && !recovery.is_null()
        {
            result["source_publication_recovery"] = recovery;
        }
        if matches!(
            e.status,
            StepExecutionStatus::Running | StepExecutionStatus::NeedsReconciliation
        ) && let Some(progress) = self.publication_read_progress(
            &root.instance.project_id,
            &root.instance.root_instance_id,
            e,
        )? {
            result["publication"] = progress;
        }
        Ok(result)
    }
    pub(crate) fn native_completion_ready(&self, root: &WorkflowHeaderFacts) -> Result<bool> {
        if root.instance.cancel_requested
            || matches!(
                root.instance.status,
                StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
            )
            || root.instance.steps.iter().any(|e| {
                !matches!(
                    e.status,
                    StepExecutionStatus::Completed | StepExecutionStatus::Skipped
                )
            })
        {
            return Ok(false);
        }
        if one(
            self.store.as_ref(),
            "workflow_change_pending",
            &root.record.id,
        )?
        .value
        .is_some()
        {
            return Ok(false);
        }
        if root.instance.plan.document.delivery_policy.is_some() {
            let delivery = one(
                self.store.as_ref(),
                "governance",
                &format!("workflow-delivery:{}", root.record.id),
            )?;
            if !delivery.value.as_ref().is_some_and(|r| {
                r["data"]["synchronized"] == true
                    && r["data"]["completion"]["pull_request"]["merged"] == true
            }) {
                return Ok(false);
            }
        }
        let first = root
            .instance
            .steps
            .first()
            .ok_or_else(|| err("storage", "流程没有步骤"))?;
        let resources = root.step(self, &first.id)?;
        let mut selection = FactSelection::default();
        selection.rule(&root.instance.plan.document.completion);
        for source in root.instance.plan.document.outputs.values() {
            selection.source(source);
        }
        let facts = resources.selected_facts(self.store.as_ref(), &selection)?;
        let c = Command {
            command_id: "completion-readiness".into(),
            target: Some(root.record.id.clone()),
            expected_revision: Some(root.record.revision),
            payload: json!({}),
        };
        let context = InvocationContext::workflow(
            WorkflowActor::Coordinator {
                source_id: root.record.id.clone(),
            },
            Some(root.instance.project_id.clone()),
        );
        let mut selected = WorkflowCommit::new(self, "query.workflow.completion", &c, &context)?;
        Ok(selected.rule(
            &root.instance,
            "$workflow",
            &root.instance.plan.document.completion,
            &facts.facts,
        )? && root
            .instance
            .plan
            .document
            .outputs
            .values()
            .all(|s| facts.facts.value(s).is_some()))
    }
    pub(crate) fn native_workflow_control_query(&self, method: &str, p: &Value) -> Result<Value> {
        if let Some(result) = self.finite_workflow_control_read(method, p) {
            return result;
        }
        let _scope = self.store.advisory_read_scope();
        let root = WorkflowHeaderFacts::read(
            self,
            p["id"]
                .as_str()
                .or_else(|| p["instance_id"].as_str())
                .ok_or_else(|| err("invalid_params", "缺少流程"))?,
        )?;
        if let Some(project) = p["project_id"].as_str() {
            require(project == root.instance.project_id, "项目不匹配")?;
        }
        if method == "workflow.outputs.read" {
            return self.native_root_outputs(
                &root.instance.project_id,
                &root.instance.root_instance_id,
                p,
            );
        }
        if method == "workflow.events.read" {
            let cursor = p["cursor"].as_u64().unwrap_or(0);
            let limit = p["limit"].as_u64().unwrap_or(100).clamp(1, 100) as usize;
            let rows = self.store.events_after(cursor, limit)?;
            let mut out = vec![];
            for row in &rows {
                let actual = one(self.store.as_ref(), "workflow_run_authority", &row.target)?;
                if row.target == root.instance.root_instance_id
                    || actual
                        .value
                        .as_ref()
                        .is_some_and(|v| v["root_instance_id"] == root.instance.root_instance_id)
                {
                    out.push(row);
                }
            }
            return Ok(
                json!({"events":out,"next_cursor":rows.last().map(|e|e.id).unwrap_or(cursor)}),
            );
        }
        if let Some(id) = p["decision_ref"].as_str() {
            let selected = one(self.store.as_ref(), "governance", id)?;
            let decision = restored(self, &selected)?;
            let pending = if decision.r#type == "workflow_human_decision" {
                restored(
                    self,
                    &one(self.store.as_ref(), "governance", &decision.definition_id)?,
                )?
            } else {
                decision.clone()
            };
            require(
                pending.r#type == "workflow_pending_decision",
                "不是决定请求",
            )?;
            let owner = WorkflowHeaderFacts::read(self, &pending.definition_id)?;
            require(
                owner.instance.root_instance_id == root.instance.root_instance_id,
                "决定不属于当前流程根",
            )?;
            let human = if decision.r#type == "workflow_human_decision" {
                Some(decision.clone())
            } else if let Some(reference) = pending.data["decision_ref"].as_str() {
                Some(restored(
                    self,
                    &one(self.store.as_ref(), "governance", reference)?,
                )?)
            } else {
                None
            };
            if let Some(human) = &human {
                require(
                    human.r#type == "workflow_human_decision"
                        && human.immutable
                        && human.definition_id == pending.id
                        && human.scope == pending.scope
                        && human.data["actor"]["kind"] == "human",
                    "人类决定与原申请不符",
                )?;
            }
            // A Human record contains the actual signed reason, not a request
            // field. Both reference forms expose the signature and preserve
            // chunked access to its complete immutable text.
            let body = if decision.r#type == "workflow_human_decision" {
                let mut signed = decision.data.clone();
                // Preserve the legacy request fields while exposing the actual
                // signature. Its reason/actor always win over request prose.
                for (key, value) in pending.data["request"].as_object().into_iter().flatten() {
                    signed
                        .as_object_mut()
                        .unwrap()
                        .entry(key.clone())
                        .or_insert_with(|| value.clone());
                }
                signed
            } else {
                pending.data["request"].clone()
            };
            let mut out = crate::orchestration::read_chunk(&body, p)?;
            out["decision_ref"] = json!(decision.id);
            out["request_ref"] = json!(pending.id);
            out["status"] = pending.data["status"].clone();
            out["human_decision_ref"] = json!(human.as_ref().map(|r| &r.id));
            out["approved"] = human
                .as_ref()
                .map(|r| r.data["approved"].clone())
                .unwrap_or(Value::Null);
            if let Some(human) = human {
                let reason = human.data["reason"].as_str().unwrap_or("");
                out["reason"] = json!(reason.chars().take(1024).collect::<String>());
                out["reason_truncated"] = json!(reason.chars().count() > 1024);
                out["actor"] = human.data["actor"].clone();
            }
            return Ok(out);
        }
        if method == "workflow.execution.inspect" {
            let id = text(p, "execution_id")?;
            let e = if let Some(e) = root.instance.steps.iter().find(|e| e.id == id) {
                e.clone()
            } else {
                let source = one(self.store.as_ref(), "workflow_history_source", &id)?;
                let row = restored(self, &source)?;
                require(
                    row.r#type == "workflow_execution_history"
                        && row.data["workflow_instance_id"] == root.record.id,
                    "原执行历史不属于当前流程",
                )?;
                row.decode()?
            };
            if let Some(part) = p["part"].as_str() {
                let value = match part {
                    "inputs" => json!(e.inputs),
                    "contract" => {
                        let row = restored(
                            self,
                            &one(self.store.as_ref(), "governance", &e.plan_revision_id)?,
                        )?;
                        row.data["plan"]["document"]["steps"]
                            .as_array()
                            .and_then(|steps| steps.iter().find(|s| s["node_id"] == e.node_id))
                            .cloned()
                            .ok_or_else(|| err("not_found", "原合同缺失"))?
                    }
                    _ => return Err(err("invalid_params", "未知执行读取部分")),
                };
                let mut out = crate::orchestration::read_chunk(&value, p)?;
                out["execution_id"] = json!(id);
                out["plan_revision_id"] = json!(e.plan_revision_id);
                out["part"] = json!(part);
                return Ok(out);
            }
            return Ok(json!(e));
        }
        let offset = p["offset"].as_u64().unwrap_or(0) as usize;
        let limit = p["limit"].as_u64().unwrap_or(32).clamp(1, 64) as usize;
        let mut steps = vec![];
        // Select this page's native executions in one committed read. The
        // advisory scope may reuse these rows during rule/action projection;
        // command admission never uses this scope or its disposable values.
        let keys = root
            .instance
            .steps
            .iter()
            .skip(offset)
            .take(limit)
            .map(|e| {
                crate::concurrency::ResourceKey::new(
                    "workflow_step",
                    json!([root.record.id, e.id]).to_string(),
                )
            })
            .collect::<Vec<_>>();
        self.store.read_resources(&keys)?;
        for e in root.instance.steps.iter().skip(offset).take(limit) {
            steps.push(self.native_step_readiness(&root, &root.step(self, &e.id)?)?);
        }
        let terminal = root.instance.cancel_requested
            || matches!(
                root.instance.status,
                StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
            );
        let mut allowed = vec!["inspect"];
        if !terminal {
            allowed.extend([
                "preview",
                "apply",
                "stop",
                "cancel",
                if root.instance.paused {
                    "resume"
                } else {
                    "pause"
                },
            ]);
            if steps.iter().any(|s| s["dispatchable"] == true) {
                allowed.push("dispatch");
            }
            if steps
                .iter()
                .any(|s| s["actions"]["accept"]["allowed"] == true)
            {
                allowed.push("accept");
            }
            if self.native_completion_ready(&root)? {
                allowed.push("complete");
            }
        }
        // Select display fields before serialization. Serializing the complete
        // compiled plan merely to replace it copies every frozen definition
        // and off-page execution on each lightweight native inspection.
        let mut runtime = Value::Object(
            root.record
                .data
                .as_object()
                .ok_or_else(|| err("storage", "流程头不是对象"))?
                .iter()
                .filter(|(key, _)| !matches!(key.as_str(), "plan" | "steps"))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
        if p["view"] == "summary" {
            for item in &mut steps {
                let execution = item["execution"].as_object_mut().unwrap();
                execution.remove("inputs");
                execution.remove("runtime_context");
            }
            runtime["steps"] = json!(steps.iter().map(|s| &s["execution"]).collect::<Vec<_>>());
            let contracts = root.instance.plan.document.steps.iter().skip(offset).take(limit).map(|s| json!({"key":s.key,"node_id":s.node_id,"name":s.name,"dependencies":s.dependencies,"executor":s.executor,"confirmation":s.confirmation})).collect::<Vec<_>>();
            runtime["plan"] = json!({"workflow_revision_id":root.instance.plan.workflow_revision_id,"standard_ref":root.instance.plan.standard_ref,"document":{"steps":contracts},"summary":true,"contract_parts":"workflow.execution.inspect: part=contract"});
            let detail = self
                .store
                .workflow_detail(&crate::read_models::WorkflowDetailQuery::parse(
                    &json!({"id":root.record.id,"view":"status","child_offset":offset}),
                )?)?
                .ok_or_else(|| err("not_found", "流程摘要缺失"))?;
            let total = root.instance.steps.len();
            return Ok(
                json!({"record":{"id":root.record.id,"revision":root.record.revision},"runtime":runtime,"steps":steps,"allowed_actions":allowed,"human_gates":self.controller_human_gates(&root.instance.root_instance_id)?,"page":{"offset":offset,"limit":limit,"total":total,"next_offset":if offset+limit<total{Some(offset+limit)}else{None},"document_complete":false,"summary":true},"children":detail.children,"children_page":{"offset":offset,"more":detail.children_more,"next_offset":detail.next_child_offset}}),
            );
        }
        let mut document = Value::Object(
            root.record.data["plan"]["document"]
                .as_object()
                .ok_or_else(|| err("storage", "冻结计划正文缺失"))?
                .iter()
                .filter(|(key, _)| key.as_str() != "steps")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
        document["steps"] = json!(
            root.instance
                .plan
                .document
                .steps
                .iter()
                .skip(offset)
                .take(limit)
                .collect::<Vec<_>>()
        );
        runtime["plan"] = json!({"workflow_revision_id":root.instance.plan.workflow_revision_id,"standard_ref":root.instance.plan.standard_ref,"work_configuration_ref":root.instance.plan.work_configuration_ref,"harness_permission_ref":root.instance.plan.harness_permission_ref,"document":document,"roles":root.instance.plan.roles});
        runtime["steps"] = json!(
            root.instance
                .steps
                .iter()
                .skip(offset)
                .take(limit)
                .collect::<Vec<_>>()
        );
        let detail = self
            .store
            .workflow_detail(&crate::read_models::WorkflowDetailQuery::parse(
                &json!({"id":root.record.id,"view":"status","child_offset":offset}),
            )?)?
            .ok_or_else(|| err("not_found", "流程摘要缺失"))?;
        let total = root.instance.steps.len();
        let budgets = root
            .instance
            .plan
            .document
            .rework_policies
            .keys()
            .map(|key| self.rework_budget_view(&root, key).map(|(_, value)| value))
            .collect::<Result<Vec<_>>>()?;
        Ok(
            json!({"rework_budgets":budgets,"human_gates":self.controller_human_gates(&root.instance.root_instance_id)?,"record":{"id":root.record.id,"revision":root.record.revision},"page":{"offset":offset,"limit":limit,"total":total,"next_offset":if offset+limit<total{Some(offset+limit)}else{None},"document_complete":offset==0&&limit>=total},"runtime":runtime,"steps":steps,"allowed_actions":allowed,"children_page":{"offset":offset,"more":detail.children_more,"next_offset":detail.next_child_offset},"delivery_types":root.instance.plan.definitions.values().filter(|r|r.r#type=="delivery_type").collect::<Vec<_>>(),"children":detail.children}),
        )
    }
}
