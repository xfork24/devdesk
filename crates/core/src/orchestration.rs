//! Explicit workflow control. Observing receipts never grants permission to dispatch.
use crate::configured_runtime::{active_execution, facts};
use crate::governance::{record, update_record};
use crate::*;

pub(crate) fn freeze_plan(
    state: &mut EngineeringState,
    instance: &mut ConfiguredWorkflowInstance,
    scope: &Scope,
    reason: &str,
) -> Result<Id> {
    instance
        .plan
        .document
        .assign_node_ids(&instance.workflow_ref);
    // A changed plan is a new source revision, never a persisted compiled view.
    let mut source_document = instance.plan.document.clone();
    for step in &mut source_document.steps {
        step.access.writable_paths.clear();
        step.access.read_only_paths.clear();
        step.access.tool_refs.clear();
    }
    let current_source = state
        .configuration(&instance.plan.workflow_revision_id)?
        .clone();
    let mut original: WorkflowDefinition = current_source.decode()?;
    original.assign_node_ids(&current_source.id);
    for step in &mut original.steps {
        step.access.writable_paths.clear();
        step.access.read_only_paths.clear();
        step.access.tool_refs.clear();
    }
    if current_source.kind != "workflow"
        || serde_json::to_value(&source_document).unwrap()
            != serde_json::to_value(&original).unwrap()
    {
        let data = json!(source_document);
        let source = record(
            "workflow",
            format!(
                "execution-workflow.{}",
                configuration_hash(&canonical_standard_content(&data))
            ),
            crate::standards::source_scope(scope),
            reason.into(),
            1,
            None,
            data,
            true,
        );
        instance.plan.workflow_revision_id = source.id.clone();
        instance.workflow_ref = source.id.clone();
        instance
            .plan
            .definitions
            .insert(source.id.clone(), source.clone());
        if state.configuration(&source.id).is_err() {
            state.governance.push(source);
        }
    }
    let existing = instance.plan.standard_ref.clone();
    let included = existing
        .as_deref()
        .and_then(|r| standard_snapshot(state, r, scope).ok())
        .is_some_and(|s| {
            s.nodes
                .iter()
                .any(|n| n.kind == "workflow" && n.reference == instance.plan.workflow_revision_id)
        });
    if !included {
        let mut roots = existing
            .as_deref()
            .and_then(|r| state.configuration(r).ok())
            .and_then(|r| r.decode::<ConfiguredStandard>().ok())
            .map(|s| s.root_workflow_refs)
            .unwrap_or_default();
        if let Some(old) = instance
            .plan
            .permission_workflow_ref
            .as_deref()
            .and_then(|old| roots.iter().position(|r| r == old))
        {
            roots[old] = instance.plan.workflow_revision_id.clone();
        } else {
            roots = vec![instance.plan.workflow_revision_id.clone()];
        }
        let source = crate::standards::capture_standard(state, roots, scope, reason)?;
        instance.plan.standard_ref = Some(source.id.clone());
        instance.plan.definitions.insert(source.id.clone(), source);
    }
    let plan = record(
        "workflow_plan_revision",
        instance.root_instance_id.clone(),
        scope.clone(),
        reason.into(),
        1,
        None,
        json!({"owner_id":instance.owner_id,"standard_ref":instance.plan.standard_ref,"workflow_ref":instance.plan.workflow_revision_id,"work_configuration_ref":instance.plan.work_configuration_ref,"instance_id":instance.steps.first().map(|e|&e.workflow_instance_id),"plan":instance.plan,"inputs":instance.inputs,"role_agent_bindings":instance.role_agent_bindings,
            "parent_revision_id":if instance.active_plan_revision_id.is_empty(){None}else{Some(&instance.active_plan_revision_id)},"reason":reason}),
        true,
    );
    instance.active_plan_revision_id = plan.id.clone();
    for execution in &mut instance.steps {
        if execution.plan_revision_id.is_empty() {
            if let Some(step) = instance
                .plan
                .document
                .steps
                .iter()
                .find(|s| s.key == execution.step)
            {
                execution.plan_revision_id = plan.id.clone();
                execution.node_id = step.node_id.clone();
                execution.contract_digest = configuration_hash(step);
            }
        }
    }
    let id = plan.id.clone();
    state.governance.push(plan);
    Ok(id)
}

pub(crate) fn update_instance(
    state: &mut EngineeringState,
    row: &ConfigurationRecord,
    instance: &ConfiguredWorkflowInstance,
) {
    update_record(
        state
            .governance
            .iter_mut()
            .find(|r| r.id == row.id)
            .unwrap(),
        json!(instance),
    );
}

pub(crate) fn decision(
    state: &mut EngineeringState,
    row: &ConfigurationRecord,
    instance: &ConfiguredWorkflowInstance,
    execution: Option<&StepExecution>,
    actor: &WorkflowActor,
    operation: &str,
    reason: &str,
) -> Id {
    let d = WorkflowDecision {
        root_instance_id: instance.root_instance_id.clone(),
        instance_id: row.id.clone(),
        plan_revision_id: instance.active_plan_revision_id.clone(),
        execution_id: execution.map(|e| e.id.clone()),
        actor: actor.clone(),
        operation: operation.into(),
        reason: reason.into(),
        input_digest: execution
            .map(|e| e.input_digest.clone())
            .unwrap_or_default(),
        outputs: execution.map(|e| e.outputs.clone()).unwrap_or_default(),
    };
    let r = record(
        "workflow_decision",
        row.id.clone(),
        row.scope.clone(),
        operation.into(),
        1,
        None,
        json!(d),
        true,
    );
    let id = r.id.clone();
    state.governance.push(r);
    id
}

pub(crate) fn waiver(
    state: &EngineeringState,
    instance: &ConfiguredWorkflowInstance,
    node: &str,
    requirement: &str,
) -> bool {
    state.governance.iter().any(|r| {
        r.kind == "workflow_waiver"
            && r.data["plan_revision_id"] == instance.active_plan_revision_id
            && r.data["node_id"] == node
            && r.data["requirement"] == requirement
    })
}

/// Business rule waivers never alter the underlying report or execution facts.
pub(crate) fn rule_satisfied(
    state: &EngineeringState,
    instance: &ConfiguredWorkflowInstance,
    node: &str,
    rule: &CompletionRule,
    facts: &WorkflowFacts,
) -> bool {
    if waiver(state, instance, node, "completion") {
        return true;
    }
    match rule {
        CompletionRule::All { rules } => rules
            .iter()
            .all(|r| rule_satisfied(state, instance, node, r, facts)),
        CompletionRule::Any { rules } => rules
            .iter()
            .any(|r| rule_satisfied(state, instance, node, r, facts)),
        CompletionRule::Independent { .. } => {
            waiver(state, instance, node, "independence") || rule.evaluate(facts)
        }
        CompletionRule::Covers { .. } => {
            waiver(state, instance, node, "coverage") || rule.evaluate(facts)
        }
        _ => rule.evaluate(facts),
    }
}

fn read_chunk(value: &Value, p: &Value) -> Result<Value> {
    let digest = configuration_hash(value);
    if let Some(expected) = p["value_digest"].as_str() {
        require(expected == digest, "读取快照已变化，请重新读取")?;
    }
    let body = value.to_string();
    let mut offset = (p["offset"].as_u64().unwrap_or(0) as usize).min(body.len());
    while !body.is_char_boundary(offset) {
        offset -= 1;
    }
    let budget = p["max_bytes"].as_u64().unwrap_or(16384).clamp(1024, 65536) as usize;
    let mut end = (offset + budget).min(body.len());
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    Ok(
        json!({"value_digest":digest,"value_chunk":&body[offset..end],"offset":offset,"next_offset":if end<body.len(){Some(end)}else{None},"total_bytes":body.len()}),
    )
}

pub(crate) fn record_auxiliary_decision(
    state: &mut EngineeringState,
    method: &str,
    c: &Command,
    actor: &WorkflowActor,
    result: &Value,
) -> Result<()> {
    let id = if method == "workflow.start" {
        result["id"].as_str()
    } else {
        c.target.as_deref()
    }
    .unwrap_or("");
    let Some(row) = state
        .governance
        .iter()
        .find(|r| r.kind == "workflow_instance" && r.id == id)
        .cloned()
    else {
        return Ok(());
    };
    let instance: ConfiguredWorkflowInstance = row.decode()?;
    let key = c.payload["step"]
        .as_str()
        .or_else(|| c.payload["node_id"].as_str());
    let execution = key.and_then(|key| {
        instance
            .steps
            .iter()
            .find(|e| e.step == key || e.node_id == key)
    });
    decision(
        state,
        &row,
        &instance,
        execution,
        actor,
        method.rsplit('.').next().unwrap_or(method),
        c.payload["reason"].as_str().unwrap_or(method),
    );
    Ok(())
}

impl Engine {
    pub(crate) fn workflow_action_permit(
        &self,
        s: &EngineeringState,
        request: &PlatformToolRequest,
    ) -> Result<WorkflowActionPermit> {
        let instance: ConfiguredWorkflowInstance =
            s.configuration(&request.instance_id)?.decode()?;
        let candidate = &request.arguments["candidate"];
        let revision = text(candidate, "revision")?;
        let step = instance
            .steps
            .iter()
            .find(|e| {
                e.id == request
                    .caller
                    .as_ref()
                    .map_or(request.execution_id.as_str(), |c| c.execution_id.as_str())
            })
            .and_then(|e| {
                instance
                    .plan
                    .document
                    .steps
                    .iter()
                    .find(|step| step.key == e.step)
            })
            .ok_or_else(|| err("scope_violation", "操作没有当前步骤来源"))?;
        let mut requirements = std::collections::BTreeMap::new();
        let mut refs = vec![];
        for requirement in instance
            .plan
            .document
            .action_requirements
            .get(&request.definition.qualified_name)
            .into_iter()
            .flatten()
        {
            let satisfied = match requirement.as_str() {
                "tests" => {
                    let test = &request.arguments["test_receipt"];
                    test["candidate"] == revision
                        && test["status"] == "succeeded"
                        && test["expected_failure"] == false
                        && s.governance.iter().any(|r| {
                            r.kind == "platform_call"
                                && r.data["request"]["instance_id"] == request.instance_id
                                && r.data["status"] == "succeeded"
                                && r.data["request"]["definition"]["qualified_name"] == "tests.run"
                                && r.data["receipt"] == *test
                        })
                }
                "confirmation" => instance.steps.iter().any(|e| {
                    e.inputs.values().any(|v| v == candidate)
                        && s.governance.iter().any(|r| {
                            r.kind == "workflow_confirmation"
                                && r.definition_id == e.id
                                && r.data["outputs"] == json!(e.outputs)
                                && r.data["input_digest"] == e.input_digest
                        })
                }),
                _ => {
                    return Err(err(
                        "unsupported",
                        format!("未知平台业务要求 {requirement}"),
                    ));
                }
            };
            if satisfied {
                requirements.insert(requirement.clone(), "satisfied".into());
            } else if let Some(w) = s.governance.iter().find(|r| {
                r.kind == "workflow_waiver"
                    && r.data["plan_revision_id"] == instance.active_plan_revision_id
                    && r.data["node_id"] == step.node_id
                    && r.data["requirement"] == *requirement
            }) {
                requirements.insert(requirement.clone(), "waived".into());
                refs.push(w.id.clone());
            } else {
                return Err(err(
                    "business_requirement_missing",
                    format!("操作要求未满足：{requirement}"),
                ));
            }
        }
        Ok(WorkflowActionPermit {
            instance_id: request.instance_id.clone(),
            plan_revision_id: instance.active_plan_revision_id,
            candidate_digest: configuration_hash(candidate),
            target_revision: text(candidate, "target_revision")?,
            requirements,
            decision_refs: refs,
        })
    }
    pub(crate) fn orchestration_command(
        &self,
        method: &str,
        c: &Command,
        state: &mut EngineeringState,
    ) -> Result<Option<Value>> {
        if method == "workflow.decision.resolve" {
            let row = state
                .configuration(c.target.as_deref().unwrap_or(""))?
                .clone();
            require(
                row.kind == "workflow_pending_decision"
                    && row.data["status"] == "pending"
                    && c.expected_revision == Some(row.revision),
                "决定已结束或版本变化",
            )?;
            let instance = state.configuration(&row.definition_id)?;
            require(
                instance.data["active_plan_revision_id"] == row.data["plan_revision_id"],
                "决定对应的计划已变化",
            )?;
            let reason = text(&c.payload, "reason")?;
            let approved = c.payload["approved"]
                .as_bool()
                .ok_or_else(|| err("invalid_params", "需要明确批准或拒绝"))?;
            let operation = row.data["request"]["operation"].as_str().unwrap_or("");
            let arguments = &row.data["request"]["arguments"];
            if approved && ["workflow.step.confirm", "workflow.step.accept"].contains(&operation) {
                let owner = state.configuration(&row.definition_id)?.clone();
                let runtime: ConfiguredWorkflowInstance = owner.decode()?;
                let key = text(arguments, "step")?;
                let execution = runtime
                    .steps
                    .iter()
                    .find(|e| e.step == key)
                    .ok_or_else(|| err("not_found", "决定对应步骤不存在"))?;
                require(
                    arguments["execution_id"] == execution.id,
                    "决定对应执行已变化",
                )?;
                let definition = runtime
                    .plan
                    .document
                    .steps
                    .iter()
                    .find(|s| s.key == key)
                    .unwrap();
                if operation == "workflow.step.confirm"
                    || definition.confirmation == ConfirmationPolicy::Human
                {
                    let mut payload = arguments.clone();
                    payload["reason"] = json!(reason);
                    let command = Command {
                        command_id: format!("{}:business-confirmation", c.command_id),
                        target: Some(owner.id),
                        expected_revision: Some(owner.revision),
                        payload,
                    };
                    self.configured_runtime_command("workflow.step.confirm", &command, state)?
                        .ok_or_else(|| err("invalid_params", "决定不是业务确认"))?;
                }
            }
            let d = record(
                "workflow_human_decision",
                row.id.clone(),
                row.scope.clone(),
                reason.clone(),
                1,
                None,
                json!({"approved":approved,"actor":{"kind":"human"},"plan_revision_id":row.data["plan_revision_id"],"request":row.data["request"],"reason":reason}),
                true,
            );
            let mut data = row.data.clone();
            data["status"] = json!(if approved { "approved" } else { "rejected" });
            data["decision_ref"] = json!(d.id);
            update_record(
                state
                    .governance
                    .iter_mut()
                    .find(|r| r.id == row.id)
                    .unwrap(),
                data,
            );
            state.governance.push(d);
            event(state, &row.definition_id, "workflow.decision", &row.id);
            return Ok(Some(
                json!({"id":row.id,"status":state.configuration(&row.id)?.data["status"]}),
            ));
        }
        if let Some(result) = self.controller_command(method, c, state)? {
            return Ok(Some(result));
        }
        if let Some(result) =
            self.workflow_control_command(method, c, state, &WorkflowActor::Human)?
        {
            return Ok(Some(result));
        }
        if [
            "workflow.start",
            "workflow.pause",
            "workflow.resume",
            "workflow.cancel",
            "workflow.step.input",
            "workflow.step.retry",
            "workflow.step.rework",
            "workflow.step.reconcile",
            "workflow.execution.reconcile",
            "workflow.step.confirm",
            "workflow.step.approve",
        ]
        .contains(&method)
        {
            let result = self.configured_runtime_command(
                if method == "workflow.execution.reconcile" {
                    "workflow.step.reconcile"
                } else {
                    method
                },
                c,
                state,
            )?;
            if let Some(result) = &result {
                record_auxiliary_decision(state, method, c, &WorkflowActor::Human, result)?;
            }
            return Ok(result);
        }
        Ok(None)
    }

    pub(crate) fn workflow_control_command(
        &self,
        method: &str,
        c: &Command,
        state: &mut EngineeringState,
        actor: &WorkflowActor,
    ) -> Result<Option<Value>> {
        if ![
            "workflow.step.dispatch",
            "workflow.step.accept",
            "workflow.complete",
            "workflow.execution.stop",
            "workflow.materials.import",
            "workflow.decision.request",
            "workflow.segment.dispatch",
            "workflow.change.apply",
        ]
        .contains(&method)
        {
            return Ok(None);
        }
        let row = state
            .configuration(c.target.as_deref().unwrap_or(""))?
            .clone();
        require(row.kind == "workflow_instance", "目标不是流程实例")?;
        let mut instance: ConfiguredWorkflowInstance = row.decode()?;
        require(
            c.expected_revision == Some(row.revision)
                || (c
                    .expected_revision
                    .is_some_and(|v| v > 0 && v <= row.revision)
                    && ([
                        "workflow.step.accept",
                        "workflow.complete",
                        "workflow.change.apply",
                        "workflow.materials.import",
                    ]
                    .contains(&method)
                        || c.payload["control_version"].as_u64()
                            == Some(instance.control_version))),
            "流程状态已变化，请重新查询",
        )?;
        require(
            c.payload["plan_revision_id"] == instance.active_plan_revision_id,
            "活动计划已变化，请重新预演",
        )?;
        require(
            !instance.cancel_requested
                && instance.status != StepExecutionStatus::Completed
                && instance.status != StepExecutionStatus::Cancelled,
            "流程已结束或取消",
        )?;
        if method == "workflow.change.apply" {
            return self.apply_workflow_change(c, state, actor).map(Some);
        }
        if method != "workflow.change.apply"
            && method != "workflow.execution.stop"
            && crate::workflow_changes::change_pending(state, &row.id)
        {
            return Err(err("change_pending", "运行计划正在变更"));
        }
        let reason = opt(&c.payload, "reason").unwrap_or_else(|| method.into());
        if method == "workflow.materials.import" {
            let delivery = state.configuration(&text(&c.payload, "delivery_ref")?)?;
            require(
                delivery.kind == "delivery_type" && delivery.scope.visible_from(&row.scope),
                "交付类型不可用",
            )?;
            let definition: DeliveryTypeDefinition = delivery.decode()?;
            require(
                !matches!(
                    definition.producer,
                    Some(DeliveryProducer::Platform { .. } | DeliveryProducer::Workflow)
                ),
                "平台或子流程产出必须引用实际执行，不能通过导入材料创建",
            )?;
            let value = c
                .payload
                .get("value")
                .ok_or_else(|| err("invalid_params", "缺少材料正文"))?;
            self.validate_configured_value(&delivery.decode()?, value)?;
            let artifact = self.artifacts.seal_scoped(
                &MaterialOwner::Project {
                    id: instance.project_id.clone(),
                },
                "materials",
                &serde_json::to_string(value).unwrap(),
            )?;
            let material = TypedMaterial {
                project_id: instance.project_id.clone(),
                root_instance_id: instance.root_instance_id.clone(),
                delivery_ref: delivery.id.clone(),
                value: value.clone(),
                value_digest: configuration_hash(value),
                artifact_id: artifact.id.clone(),
                source: text(&c.payload, "source")?,
            };
            let r = record(
                "typed_material",
                instance.root_instance_id.clone(),
                row.scope.clone(),
                reason,
                1,
                None,
                json!(material),
                true,
            );
            let out = json!({"id":r.id,"material":material});
            state.artifacts.push(artifact);
            state.governance.push(r);
            return Ok(Some(out));
        }
        if method == "workflow.decision.request" {
            text(&c.payload, "reason")?;
            let operation = text(&c.payload, "operation")?;
            require(
                crate::controllers::CONTROL_TOOLS.contains(&operation.as_str())
                    || operation == "workflow.step.confirm",
                "不是可请求的工作流操作",
            )?;
            let arguments = &c.payload["arguments"];
            require(
                arguments.is_object()
                    && arguments["plan_revision_id"] == instance.active_plan_revision_id,
                "请求必须绑定当前计划和确切参数",
            )?;
            if let Some(id) = arguments["instance_id"]
                .as_str()
                .or_else(|| arguments["id"].as_str())
            {
                require(id == row.id, "请求目标与决定归属不一致")?;
            }
            if ["workflow.step.confirm", "workflow.step.accept"].contains(&operation.as_str()) {
                let key = text(arguments, "step")?;
                let execution = instance
                    .steps
                    .iter()
                    .find(|e| e.step == key)
                    .ok_or_else(|| err("not_found", "步骤不存在"))?;
                require(
                    matches!(
                        execution.status,
                        StepExecutionStatus::AwaitingAcceptance
                            | StepExecutionStatus::WaitingConfirmation
                    ) && arguments["execution_id"] == execution.id
                        && arguments["input_digest"] == execution.input_digest
                        && arguments["outputs"] == json!(execution.outputs),
                    "决定必须绑定当前可接纳的执行与产物",
                )?;
            }
            let r = record(
                "workflow_pending_decision",
                row.id.clone(),
                row.scope.clone(),
                reason,
                1,
                None,
                json!({"status":"pending","plan_revision_id":instance.active_plan_revision_id,"request":c.payload,"actor":actor}),
                false,
            );
            let out = json!({"id":r.id,"status":"pending"});
            state.governance.push(r);
            return Ok(Some(out));
        }
        if method == "workflow.complete" {
            require(
                instance.steps.iter().all(|e| {
                    matches!(
                        e.status,
                        StepExecutionStatus::Completed | StepExecutionStatus::Skipped
                    )
                }),
                "仍有未处置的步骤",
            )?;
            let current = facts(state, &instance)?;
            require(
                rule_satisfied(
                    state,
                    &instance,
                    "$workflow",
                    &instance.plan.document.completion,
                    &current,
                ) || waiver(state, &instance, "$workflow", "completion"),
                "整体完成条件未满足",
            )?;
            for source in instance.plan.document.outputs.values() {
                require(current.value(source).is_some(), "缺少最终交付输出")?;
            }
            let d = decision(state, &row, &instance, None, actor, "complete", &reason);
            state.governance.push(record(
                "workflow_completion",
                row.id.clone(),
                row.scope.clone(),
                reason,
                1,
                None,
                json!({"plan_revision_id":instance.active_plan_revision_id,"decision_id":d}),
                true,
            ));
            instance.status = StepExecutionStatus::Completed;
            instance.control_version += 1;
            if instance.parent_execution_id.is_none() {
                if let Some(f) = state
                    .features
                    .iter_mut()
                    .find(|f| f.id == instance.owner_id)
                {
                    f.status = Status::Completed;
                    f.revision += 1;
                }
            }
            update_instance(state, &row, &instance);
            return Ok(Some(json!({"completed":true,"decision_id":d})));
        }
        if method == "workflow.segment.dispatch" {
            return self.dispatch_workflow_segment(c, state, actor).map(Some);
        }
        let key = c.payload["step"]
            .as_str()
            .or_else(|| c.payload["node_id"].as_str())
            .ok_or_else(|| err("invalid_params", "需要步骤或节点身份"))?;
        let index = instance
            .steps
            .iter()
            .position(|e| e.step == key || e.node_id == key || e.id == key)
            .ok_or_else(|| err("not_found", "步骤不存在"))?;
        let execution = instance.steps[index].clone();
        let step = instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == execution.step)
            .unwrap()
            .clone();
        match method {
            "workflow.step.dispatch" => {
                require(
                    !crate::workflow_changes::change_pending(state, &row.id),
                    "计划变更正在停止受影响执行者",
                )?;
                require(!state.workflow_instance_suspended(&row.id), "流程暂停中")?;
                require(
                    execution.status == StepExecutionStatus::Queued,
                    "步骤已派发或已结束",
                )?;
                require(
                    step.dependencies.iter().all(|k| {
                        instance.steps.iter().any(|e| {
                            &e.step == k
                                && matches!(
                                    e.status,
                                    StepExecutionStatus::Completed | StepExecutionStatus::Skipped
                                )
                        })
                    }),
                    "依赖尚未接纳",
                )?;
                if let Some(parent) = &instance.parent_execution_id {
                    let group = state
                        .governance
                        .iter()
                        .find(|r| r.kind == "workflow_children" && r.definition_id == *parent)
                        .ok_or_else(|| err("invalid_workflow", "子流程来源缺失"))?;
                    let others = group.data["children"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(Value::as_str)
                        .filter(|id| *id != row.id)
                        .filter_map(|id| state.configuration(id).ok())
                        .filter(|r| r.data["status"] == "running")
                        .count();
                    require(
                        others < (group.data["max_concurrency"].as_u64().unwrap_or(1) as usize),
                        "子流程并发已满",
                    )?;
                }
                let d = decision(
                    state,
                    &row,
                    &instance,
                    Some(&execution),
                    actor,
                    "dispatch",
                    &reason,
                );
                state.governance.push(record(
                    "workflow_dispatch",
                    execution.id.clone(),
                    row.scope.clone(),
                    reason,
                    1,
                    None,
                    json!({"plan_revision_id":instance.active_plan_revision_id,"decision_id":d}),
                    true,
                ));
                self.advance_configured_step(state, &row, &mut instance, &step)?;
                require(
                    instance.steps[index].status != StepExecutionStatus::Queued,
                    "步骤输入或资源未就绪",
                )?;
            }
            "workflow.step.accept" => {
                require(
                    matches!(
                        execution.status,
                        StepExecutionStatus::AwaitingAcceptance
                            | StepExecutionStatus::WaitingConfirmation
                    ),
                    "步骤没有可接纳的结果",
                )?;
                require(!active_execution(state, &execution), "执行者尚未停止")?;
                require(
                    c.payload["execution_id"] == execution.id
                        && c.payload["input_digest"] == execution.input_digest
                        && c.payload["outputs"] == json!(execution.outputs),
                    "接纳结果版本不匹配",
                )?;
                let current = facts(state, &instance)?;
                let overridden = state.governance.iter().any(|r| {
                    r.kind == "workflow_confirmation"
                        && r.definition_id == execution.id
                        && r.data["override_completion"] == true
                        && r.data["input_digest"] == execution.input_digest
                        && r.data["outputs"] == json!(execution.outputs)
                });
                require(
                    rule_satisfied(state, &instance, &step.node_id, &step.completion, &current)
                        || overridden
                        || waiver(state, &instance, &step.node_id, "completion"),
                    "完成条件未满足",
                )?;
                require(
                    step.confirmation != ConfirmationPolicy::Human
                        || current.approvals.contains(&execution.step)
                        || waiver(state, &instance, &step.node_id, "confirmation"),
                    "等待人类决定",
                )?;
                decision(
                    state,
                    &row,
                    &instance,
                    Some(&execution),
                    actor,
                    "accept",
                    &reason,
                );
                instance.steps[index].status = StepExecutionStatus::Completed;
            }
            "workflow.execution.stop" => {
                require(
                    !matches!(
                        execution.status,
                        StepExecutionStatus::Completed
                            | StepExecutionStatus::Skipped
                            | StepExecutionStatus::Cancelled
                    ),
                    "该执行已经结束",
                )?;
                let children: Vec<_> = state
                    .governance
                    .iter()
                    .filter(|r| {
                        r.kind == "workflow_instance"
                            && r.data["parent_execution_id"] == execution.id
                    })
                    .map(|r| r.id.clone())
                    .collect();
                for child in children {
                    let descendants =
                        crate::configured_runtime::descendant_instances(state, &child);
                    for row in state
                        .governance
                        .iter_mut()
                        .filter(|r| descendants.contains(&r.id))
                    {
                        let mut data = row.data.clone();
                        data["cancel_requested"] = json!(true);
                        data["paused"] = json!(false);
                        update_record(row, data);
                    }
                }
                state.governance.push(record(
                    "workflow_stop",
                    execution.id.clone(),
                    row.scope.clone(),
                    reason.clone(),
                    1,
                    None,
                    json!({"actor":actor,"reason":reason}),
                    true,
                ));
                for turn in state.turns.iter_mut().filter(|t| {
                    Some(&t.session_id) == execution.session_id.as_ref() && t.status.active()
                }) {
                    turn.cancel_requested = true;
                    turn.revision += 1;
                    if turn.status == TurnStatus::Queued {
                        turn.status = TurnStatus::Cancelled;
                        turn.finished = Some(now());
                    }
                }
                if let Some(call) = execution
                    .tool_call_id
                    .as_ref()
                    .and_then(|id| state.governance.iter_mut().find(|r| r.id == *id))
                {
                    if !["succeeded", "failed", "cancelled"]
                        .contains(&call.data["status"].as_str().unwrap_or(""))
                    {
                        let mut data = call.data.clone();
                        data["cancel_requested"] = json!(true);
                        update_record(call, data);
                    }
                }
                if !active_execution(state, &execution)
                    && !matches!(
                        execution.status,
                        StepExecutionStatus::Failed | StepExecutionStatus::NeedsReconciliation
                    )
                {
                    instance.steps[index].status = StepExecutionStatus::Cancelled;
                    instance.steps[index].finished = Some(now());
                }
                decision(
                    state,
                    &row,
                    &instance,
                    Some(&execution),
                    actor,
                    "stop",
                    &reason,
                );
            }
            _ => unreachable!(),
        }
        instance.control_version += 1;
        update_instance(state, &row, &instance);
        Ok(Some(
            json!({"id":row.id,"execution":instance.steps[index],"plan_revision_id":instance.active_plan_revision_id,"control_version":instance.control_version}),
        ))
    }

    pub(crate) fn workflow_control_query(&self, method: &str, p: &Value) -> Option<Result<Value>> {
        if method == "workflow.change.preview" || method == "query.workflow_change_preview" {
            return Some(self.preview_workflow_change(p));
        }
        if ![
            "workflow.inspect",
            "query.workflow_control",
            "workflow.outputs.read",
            "workflow.events.read",
            "workflow.execution.inspect",
        ]
        .contains(&method)
        {
            return None;
        }
        Some((|| {
            let s = self.store.snapshot()?;
            let row = s.configuration(
                p["id"]
                    .as_str()
                    .or_else(|| p["instance_id"].as_str())
                    .unwrap_or(""),
            )?;
            let instance: ConfiguredWorkflowInstance = row.decode()?;
            if let Some(pid) = p["project_id"].as_str() {
                require(instance.project_id == pid, "项目不匹配")?;
            }
            if method == "workflow.inspect" {
                if let Some(id) = p["decision_ref"].as_str() {
                    let decision = s.configuration(id)?;
                    let pending = if decision.kind == "workflow_human_decision" {
                        s.configuration(&decision.definition_id)?
                    } else {
                        decision
                    };
                    require(
                        pending.kind == "workflow_pending_decision"
                            && s.configuration(&pending.definition_id)?.data["root_instance_id"]
                                == instance.root_instance_id,
                        "决定不属于当前根实例",
                    )?;
                    let mut out = read_chunk(&decision.data["request"], p)?;
                    out["decision_ref"] = json!(decision.id);
                    out["status"] = pending.data["status"].clone();
                    out["approved"] = decision.data["approved"].clone();
                    return Ok(out);
                }
            }
            if method == "workflow.outputs.read" {
                let ids: Vec<String> =
                    serde_json::from_value(p.get("ids").cloned().unwrap_or(json!([])))
                        .map_err(|e| err("invalid_params", e.to_string()))?;
                require(ids.len() <= 100, "最多读取 100 项产物")?;
                let mut out = vec![];
                let count = ids.len().max(1);
                for id in ids {
                    let r = s.configuration(&id)?;
                    let same_root = if r.kind == "typed_material" {
                        r.data["root_instance_id"] == instance.root_instance_id
                    } else {
                        s.governance.iter().any(|owner| {
                            owner.kind == "workflow_instance"
                                && owner.data["root_instance_id"] == instance.root_instance_id
                                && owner.data["steps"].as_array().is_some_and(|steps| {
                                    steps.iter().any(|e| e["id"] == r.data["execution_id"])
                                })
                        }) || s.governance.iter().any(|e| {
                            e.kind == "workflow_execution_history"
                                && e.data["id"] == r.data["execution_id"]
                                && s.configuration(
                                    e.data["workflow_instance_id"].as_str().unwrap_or(""),
                                )
                                .is_ok_and(|owner| {
                                    owner.data["root_instance_id"] == instance.root_instance_id
                                })
                        })
                    };
                    require(
                        r.scope.project() == Some(instance.project_id.as_str())
                            && same_root
                            && ["workflow_output", "typed_material"].contains(&r.kind.as_str()),
                        "产物不属于当前流程",
                    )?;
                    let value = r.data["value"]
                        .pointer(p["pointer"].as_str().unwrap_or(""))
                        .ok_or_else(|| err("invalid_params", "产物字段不存在"))?;
                    let serialized = serde_json::to_string(value).unwrap();
                    let budget = p["max_bytes"].as_u64().unwrap_or(16384).clamp(1024, 65536)
                        as usize
                        / count;
                    let mut offset =
                        (p["offset"].as_u64().unwrap_or(0) as usize).min(serialized.len());
                    while !serialized.is_char_boundary(offset) {
                        offset -= 1;
                    }
                    if serialized.len() > budget || offset > 0 {
                        let mut end = (offset + budget).min(serialized.len());
                        while !serialized.is_char_boundary(end) {
                            end -= 1;
                        }
                        let mut data = r.data.clone();
                        data.as_object_mut().unwrap().remove("value");
                        out.push(json!({"id":r.id,"data":data,"value_chunk":&serialized[offset..end],"offset":offset,"next_offset":if end<serialized.len(){Some(end)}else{None},"total_bytes":serialized.len()}));
                    } else {
                        let mut scoped = json!(r);
                        scoped["data"]["value"] = value.clone();
                        out.push(scoped);
                    }
                }
                return Ok(json!({"outputs":out}));
            }
            if method == "workflow.events.read" {
                let cursor = p["cursor"].as_u64().unwrap_or(0);
                let limit = p["limit"].as_u64().unwrap_or(100).clamp(1, 100) as usize;
                let events = s
                    .events
                    .iter()
                    .filter(|e| {
                        e.id > cursor
                            && (e.target == instance.root_instance_id
                                || s.configuration(&e.target).is_ok_and(|r| {
                                    r.kind == "workflow_instance"
                                        && r.data["root_instance_id"] == instance.root_instance_id
                                }))
                    })
                    .take(limit)
                    .collect::<Vec<_>>();
                let next_cursor = if events.len() == limit {
                    events.last().unwrap().id
                } else {
                    s.events.last().map(|e| e.id).unwrap_or(cursor)
                };
                return Ok(json!({"events":events,"next_cursor":next_cursor}));
            }
            if method == "workflow.execution.inspect" {
                let id = text(p, "execution_id")?;
                let execution = instance
                    .steps
                    .iter()
                    .find(|e| e.id == id)
                    .cloned()
                    .or_else(|| {
                        s.governance
                            .iter()
                            .find(|r| {
                                r.kind == "workflow_execution_history"
                                    && r.definition_id == id
                                    && r.data["workflow_instance_id"] == row.id
                            })
                            .and_then(|r| r.decode::<StepExecution>().ok())
                    })
                    .ok_or_else(|| err("not_found", "执行不存在"))?;
                if let Some(part) = p["part"].as_str() {
                    let value=match part {
                        "inputs"=>json!(execution.inputs),
                        "contract"=>s.configuration(&execution.plan_revision_id)?.data["plan"]["document"]["steps"].as_array().and_then(|steps|steps.iter().find(|step|step["node_id"]==execution.node_id)).cloned().ok_or_else(||err("not_found","原始步骤合同不存在"))?,
                        _=>return Err(err("invalid_params","未知执行读取部分")),
                    };
                    let mut out = read_chunk(&value, p)?;
                    out["execution_id"] = json!(id);
                    out["plan_revision_id"] = json!(execution.plan_revision_id);
                    out["part"] = json!(part);
                    return Ok(out);
                }
                return Ok(json!(execution));
            }
            let current = facts(&s, &instance)?;
            let pending = crate::workflow_changes::change_pending(&s, &row.id);
            let terminal = matches!(
                instance.status,
                StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
            );
            let steps=instance.plan.document.steps.iter().map(|step|{
                let e=instance.steps.iter().find(|e|e.step==step.key).unwrap();
                let dependencies=step.dependencies.iter().all(|k|instance.steps.iter().any(|e|&e.step==k&&matches!(e.status,StepExecutionStatus::Completed|StepExecutionStatus::Skipped)));
                let inputs=if e.status==StepExecutionStatus::Queued&&dependencies&&step.condition.evaluate(&current){crate::configured_runtime::material_inputs(step,&current).map(|_|()).map_err(|e|e.message)}else{Ok(())};
                json!({"step":step.key,"node_id":step.node_id,"execution":e,"dispatchable":!terminal&&!pending&&inputs.is_ok()&&e.status==StepExecutionStatus::Queued&&dependencies&&!s.workflow_instance_suspended(&row.id),"input_gap":inputs.err(),"completion_satisfied":rule_satisfied(&s,&instance,&step.node_id,&step.completion,&current),"completion_overridden":s.governance.iter().any(|r|r.kind=="workflow_confirmation"&&r.definition_id==e.id&&r.data["override_completion"]==true&&r.data["input_digest"]==e.input_digest&&r.data["outputs"]==json!(e.outputs)),"human_satisfied":step.confirmation!=ConfirmationPolicy::Human||current.approvals.contains(&step.key)||waiver(&s,&instance,&step.node_id,"confirmation")})
            }).collect::<Vec<_>>();
            let mut allowed = vec!["inspect"];
            if !terminal {
                allowed.extend(["preview", "apply", "stop", "cancel"]);
                allowed.push(if instance.paused { "resume" } else { "pause" });
                if !pending && steps.iter().any(|s| s["dispatchable"] == true) {
                    allowed.push("dispatch");
                }
                if !pending
                    && instance.steps.iter().any(|e| {
                        matches!(
                            e.status,
                            StepExecutionStatus::AwaitingAcceptance
                                | StepExecutionStatus::WaitingConfirmation
                        )
                    })
                {
                    allowed.push("accept");
                }
                if instance.steps.iter().all(|e| {
                    matches!(
                        e.status,
                        StepExecutionStatus::Completed | StepExecutionStatus::Skipped
                    )
                }) {
                    allowed.push("complete");
                }
            }
            let children=s.governance.iter().filter(|r|r.kind=="workflow_instance"&&r.data["root_instance_id"]==instance.root_instance_id&&r.id!=row.id).map(|r|json!({"id":r.id,"parent_execution_id":r.data["parent_execution_id"],"plan_revision_id":r.data["active_plan_revision_id"],"status":r.data["status"],"item_id":r.data["inputs"]["item"]["id"]})).collect::<Vec<_>>();
            let offset = p["offset"].as_u64().unwrap_or(0) as usize;
            let limit = p["limit"].as_u64().unwrap_or(32).clamp(1, 64) as usize;
            let total = instance.plan.document.steps.len();
            let mut runtime = json!(instance);
            runtime["plan"] =
                json!({"document":instance.plan.document,"roles":instance.plan.roles});
            let selected_keys = instance
                .plan
                .document
                .steps
                .iter()
                .skip(offset)
                .take(limit)
                .map(|s| s.key.as_str())
                .collect::<std::collections::BTreeSet<_>>();
            runtime["plan"]["document"]["steps"] = json!(
                instance
                    .plan
                    .document
                    .steps
                    .iter()
                    .skip(offset)
                    .take(limit)
                    .collect::<Vec<_>>()
            );
            runtime["steps"] = json!(
                instance
                    .steps
                    .iter()
                    .filter(|e| selected_keys.contains(e.step.as_str()))
                    .collect::<Vec<_>>()
            );
            Ok(
                json!({"record":{"id":row.id,"revision":row.revision},"page":{"offset":offset,"limit":limit,"total":total,"next_offset":if offset+limit<total {Some(offset+limit)}else{None},"document_complete":offset==0&&limit>=total},"runtime":runtime,"steps":steps.into_iter().skip(offset).take(limit).collect::<Vec<_>>(),"allowed_actions":allowed,"children_page":{"offset":offset,"total":children.len(),"next_offset":if offset+limit<children.len(){Some(offset+limit)}else{None}},"delivery_types":instance.plan.definitions.values().filter(|r|r.kind=="delivery_type").collect::<Vec<_>>(),"children":children.iter().skip(offset).take(limit).collect::<Vec<_>>()}),
            )
        })())
    }
}
