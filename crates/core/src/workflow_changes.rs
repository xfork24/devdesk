//! Durable plan changes: preview, stop affected executors, atomically activate.
use crate::configured_runtime::active_execution;
use crate::governance::{record, update_record};
use crate::orchestration::{decision, freeze_plan, update_instance};
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize, Deserialize, Clone)]
struct Preview {
    document: WorkflowDefinition,
    plan: ConfiguredWorkflowPlan,
    bindings: BTreeMap<Id, Id>,
    inputs: BTreeMap<String, Value>,
    affected: BTreeSet<String>,
    #[serde(default)]
    budget_updates: BTreeSet<String>,
    bypassed: BTreeSet<String>,
    waivers: Vec<(String, String)>,
    footprint: Value,
    preview_hash: String,
}

fn pending(state: &EngineeringState, instance_id: &str) -> bool {
    let root = state
        .configuration(instance_id)
        .ok()
        .and_then(|r| r.data["root_instance_id"].as_str())
        .unwrap_or(instance_id);
    state.governance.iter().any(|r| {
        r.kind == "workflow_change"
            && ["prepared", "waiting_quiescence"].contains(&r.data["status"].as_str().unwrap_or(""))
            && state
                .configuration(&r.definition_id)
                .is_ok_and(|instance| instance.data["root_instance_id"] == root)
    })
}
pub(crate) fn change_pending(state: &EngineeringState, id: &str) -> bool {
    pending(state, id)
}

impl Engine {
    fn change_preview_in(
        &self,
        state: &EngineeringState,
        row: &ConfigurationRecord,
        changes: &WorkflowChangeSet,
    ) -> Result<Preview> {
        changes.validate()?;
        let old: ConfiguredWorkflowInstance = row.decode()?;
        require(
            changes.plan_revision_id == old.active_plan_revision_id,
            "计划已变化",
        )?;
        let mut document = old.plan.document.clone();
        let mut bindings = old.role_agent_bindings.clone();
        let mut inputs = old.inputs.clone();
        let mut bypassed: BTreeSet<String> = old
            .steps
            .iter()
            .filter(|e| {
                e.status == StepExecutionStatus::Skipped
                    && state.governance.iter().any(|r| {
                        r.kind == "workflow_change"
                            && r.data["status"] == "applied"
                            && r.data["new_plan_revision_id"] == e.plan_revision_id
                            && r.data["preview"]["bypassed"]
                                .as_array()
                                .is_some_and(|keys| keys.iter().any(|key| key == &json!(e.step)))
                    })
            })
            .map(|e| e.step.clone())
            .collect();
        let mut waivers = vec![];
        let mut task_changes = vec![];
        for op in &changes.operations {
            match op {
                WorkflowChangeOperation::ReplacePlan { document: d } => document = d.clone(),
                WorkflowChangeOperation::UpsertStep { step } => {
                    if let Some(slot) = document.steps.iter_mut().find(|s| {
                        (!step.node_id.is_empty() && s.node_id == step.node_id) || s.key == step.key
                    }) {
                        let mut next = step.clone();
                        if next.node_id.is_empty() {
                            next.node_id = slot.node_id.clone();
                        }
                        *slot = next;
                    } else {
                        document.steps.push(step.clone());
                    }
                }
                WorkflowChangeOperation::RemoveStep { node_id } => {
                    require(
                        document.steps.iter().any(|s| &s.node_id == node_id),
                        "删除的节点不存在",
                    )?;
                    document.steps.retain(|s| &s.node_id != node_id);
                }
                WorkflowChangeOperation::UpsertStage { stage } => {
                    if let Some(s) = document.stages.iter_mut().find(|s| {
                        s.key == stage.key
                            || (!stage.node_id.is_empty() && s.node_id == stage.node_id)
                    }) {
                        let mut n = stage.clone();
                        if n.node_id.is_empty() {
                            n.node_id = s.node_id.clone();
                        }
                        *s = n;
                    } else {
                        document.stages.push(stage.clone());
                    }
                }
                WorkflowChangeOperation::BindInput {
                    node_id,
                    input,
                    binding,
                } => {
                    let s = document
                        .steps
                        .iter_mut()
                        .find(|s| &s.node_id == node_id)
                        .ok_or_else(|| err("not_found", "输入绑定节点不存在"))?;
                    s.inputs.insert(input.clone(), binding.clone());
                }
                WorkflowChangeOperation::SetBindings { bindings: b } => bindings = b.clone(),
                WorkflowChangeOperation::SetInputs { inputs: i } => inputs = i.clone(),
                WorkflowChangeOperation::Waive {
                    node_id,
                    requirement,
                } => {
                    require(
                        [
                            "completion",
                            "confirmation",
                            "tests",
                            "independence",
                            "coverage",
                        ]
                        .contains(&requirement.as_str()),
                        "未知业务要求",
                    )?;
                    require(
                        (node_id == "$workflow"
                            && ["completion", "coverage", "independence"]
                                .contains(&requirement.as_str()))
                            || document.steps.iter().any(|s| s.node_id == *node_id),
                        "豁免目标不存在",
                    )?;
                    waivers.push((node_id.clone(), requirement.clone()));
                }
                WorkflowChangeOperation::BypassStage {
                    stage,
                    substitutions,
                } => {
                    let st = document
                        .stages
                        .iter()
                        .find(|s| &s.key == stage || &s.node_id == stage)
                        .ok_or_else(|| err("not_found", "阶段不存在"))?
                        .key
                        .clone();
                    let members: BTreeSet<_> = document
                        .steps
                        .iter()
                        .filter(|s| s.stage == st)
                        .map(|s| s.key.clone())
                        .collect();
                    require(!members.is_empty(), "阶段没有成员")?;
                    bypassed.extend(members.iter().cloned());
                    let output_types: BTreeMap<String, String> = document
                        .steps
                        .iter()
                        .flat_map(|s| {
                            s.outputs.iter().map(move |(name, port)| {
                                (format!("{}.{}", s.key, name), port.delivery_ref.clone())
                            })
                        })
                        .collect();
                    let substitute = |binding: &mut InputBinding| -> Result<()> {
                        if let InputBinding::Source { source } = binding {
                            if members.contains(&source.step) {
                                let key = format!("{}.{}", source.step, source.port);
                                let replacement = substitutions.get(&key).ok_or_else(|| {
                                    err(
                                        "missing_material",
                                        format!("跳过阶段后缺少 {key} 的替代输入"),
                                    )
                                })?;
                                if let InputBinding::Material { material_ref } = replacement {
                                    let material = state.configuration(material_ref)?;
                                    require(
                                        material.data["delivery_ref"] == output_types[&key],
                                        "替代材料的交付类型修订不匹配",
                                    )?;
                                }
                                require(
                                    source.pointer.is_empty(),
                                    "含 JSON Pointer 的来源请先显式重绑为精确替代值",
                                )?;
                                *binding = replacement.clone();
                            }
                        }
                        Ok(())
                    };
                    for step in &mut document.steps {
                        if members.contains(&step.key) {
                            continue;
                        }
                        for binding in step.inputs.values_mut() {
                            substitute(binding)?;
                        }
                        if let StepExecutor::Tool { arguments, .. } = &mut step.executor {
                            for binding in arguments.values_mut() {
                                substitute(binding)?;
                            }
                        }
                    }
                    for source in document.outputs.values() {
                        require(
                            !members.contains(&source.step),
                            "阶段仍提供最终输出，请显式调整最终交付",
                        )?;
                    }
                }
                WorkflowChangeOperation::ReviseTaskScope { task_id, .. }
                | WorkflowChangeOperation::ReviseTaskDependencies { task_id, .. } => {
                    require(
                        old.owner_id == *task_id
                            || old.inputs.get("item").is_some_and(|i| i["id"] == *task_id),
                        "请对该 Task 的确切子流程应用范围修改",
                    )?;
                    let task = state.task(task_id)?;
                    require(
                        state.feature(&task.feature_id)?.project_id == old.project_id,
                        "任务跨项目",
                    )?;
                    task_changes.push(serde_json::to_value(task).unwrap());
                    if let Some(item) = inputs.get_mut("item").and_then(Value::as_object_mut) {
                        match op {
                            WorkflowChangeOperation::ReviseTaskScope { title, scope, .. } => {
                                item.insert("title".into(), json!(title));
                                item.insert("scope".into(), json!(scope));
                            }
                            WorkflowChangeOperation::ReviseTaskDependencies {
                                dependencies,
                                ..
                            } => {
                                item.insert("dependencies".into(), json!(dependencies));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        document.assign_node_ids(&old.workflow_ref);
        document.validate()?;
        let mut seeds = BTreeSet::new();
        let mut budget_updates = BTreeSet::new();
        for step in &old.plan.document.steps {
            let next = document.steps.iter().find(|s| s.node_id == step.node_id);
            let extends_invocation_budget = next.is_some_and(|next| {
                let mut business = next.clone();
                business.budget.timeout_seconds = step.budget.timeout_seconds;
                matches!(step.executor, StepExecutor::Workflow { .. })
                    && next.budget.timeout_seconds > step.budget.timeout_seconds
                    && business == *step
                    && old.steps.iter().any(|e| {
                        e.node_id == step.node_id
                            && e.started.is_some()
                            && state.governance.iter().any(|r| {
                                r.kind == "workflow_instance"
                                    && r.data["parent_execution_id"] == e.id
                            })
                    })
            });
            if extends_invocation_budget {
                budget_updates.insert(step.key.clone());
            }
            if (next != Some(step) && !extends_invocation_budget)
                || bindings != old.role_agent_bindings
                || inputs != old.inputs
                || !task_changes.is_empty()
            {
                seeds.insert(step.key.clone());
            }
        }
        seeds.extend(
            document
                .steps
                .iter()
                .filter(|s| {
                    !old.plan
                        .document
                        .steps
                        .iter()
                        .any(|old| old.node_id == s.node_id)
                })
                .map(|s| s.key.clone()),
        );
        if document.action_requirements != old.plan.document.action_requirements {
            seeds.extend(old.plan.document.steps.iter().map(|s| s.key.clone()));
        }
        for step in &old.plan.document.steps {
            if old
                .plan
                .document
                .stages
                .iter()
                .find(|s| s.key == step.stage)
                != document.stages.iter().find(|s| s.key == step.stage)
            {
                seeds.insert(step.key.clone());
            }
        }
        seeds.extend(bypassed.iter().cloned());
        let mut affected = old.plan.document.impacted_nodes(&seeds);
        affected.extend(document.impacted_nodes(&seeds));
        // Rules depending on bypassed nodes must be explicitly changed or waived.
        for step in document.steps.iter().filter(|s| !bypassed.contains(&s.key)) {
            let text = serde_json::to_string(&step.completion).unwrap();
            let refers = bypassed
                .iter()
                .any(|key| text.contains(&format!("\"{key}\"")));
            require(
                !refers || waivers.contains(&(step.node_id.clone(), "completion".into())),
                format!("{} 的完成规则仍引用绕过节点", step.key),
            )?;
        }
        let overall = serde_json::to_string(&document.completion).unwrap();
        require(
            !bypassed
                .iter()
                .any(|key| overall.contains(&format!("\"{key}\"")))
                || waivers.contains(&("$workflow".into(), "completion".into())),
            "整体完成规则仍引用绕过节点",
        )?;
        for step in &document.steps {
            let mut material_bindings: Vec<_> = step.inputs.values().collect();
            if let StepExecutor::Tool { arguments, .. } = &step.executor {
                material_bindings.extend(arguments.values());
            }
            for binding in material_bindings {
                if let InputBinding::Material { material_ref } = binding {
                    let m = state.configuration(material_ref)?;
                    require(
                        m.kind == "typed_material"
                            && m.data["project_id"] == old.project_id
                            && m.data["root_instance_id"] == old.root_instance_id,
                        "材料来源跨流程",
                    )?;
                }
            }
        }
        let mut candidate = record(
            "runtime_workflow",
            format!("runtime:{}", row.id),
            row.scope.clone(),
            row.name.clone(),
            old.control_version + 1,
            None,
            json!(document),
            true,
        );
        candidate.id = format!("runtime-workflow-{}", configuration_hash(changes));
        candidate.created_at = 0;
        let plan =
            self.compile_configured_workflow(state, &candidate, &row.scope, Some(&bindings))?;
        if let Some(configuration) = state
            .governance
            .iter()
            .find(|r| r.kind == "feature_workflow_configuration" && r.definition_id == old.owner_id)
        {
            if let Some(standard_ref) = configuration.data["configuration"]["standard_ref"].as_str()
            {
                let baseline = if old.parent_execution_id.is_none() {
                    self.check_configured_baseline(state, &plan, standard_ref, &row.scope)
                        .map(|_| ())
                } else {
                    let standard: ConfiguredStandard =
                        state.configuration(standard_ref)?.decode()?;
                    (|| -> Result<()> {
                        for rule in &standard.baseline_rules {
                            let matches = |binding: &&Value| {
                                let deliveries = binding["outputs"]
                                    .as_object()
                                    .into_iter()
                                    .flat_map(|m| m.values())
                                    .filter_map(|p| p["delivery_ref"].as_str())
                                    .collect::<Vec<_>>();
                                let tags = binding["tags"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter_map(|t| t.as_str().map(str::to_owned))
                                    .collect::<Vec<_>>();
                                rule.selector.matches(
                                    binding["role_ref"].as_str(),
                                    &deliveries,
                                    &tags,
                                )
                            };
                            let old_count = old
                                .plan
                                .specification_snapshot
                                .iter()
                                .filter(matches)
                                .count();
                            if old_count == 0 {
                                continue;
                            }
                            let current = plan
                                .specification_snapshot
                                .iter()
                                .filter(matches)
                                .collect::<Vec<_>>();
                            require(
                                current.len() >= old_count.min(rule.min_matches as usize),
                                "子流程缺少原 Standard 覆盖落点",
                            )?;
                            for binding in current {
                                for spec in &rule.required_spec_refs {
                                    require(
                                        binding["spec_refs"]
                                            .as_array()
                                            .is_some_and(|refs| refs.contains(&json!(spec))),
                                        format!("子流程缺少原 Standard 规范 {spec}"),
                                    )?;
                                }
                            }
                        }
                        Ok(())
                    })()
                };
                if let Err(error) = baseline {
                    require(
                        waivers.contains(&("$workflow".into(), "coverage".into())),
                        format!(
                            "原 Standard 要求未满足，需明确 coverage 豁免：{}",
                            error.message
                        ),
                    )?;
                }
            }
        }
        let definitions = |p: &ConfiguredWorkflowPlan| {
            p.definitions
                .iter()
                .filter(|(_, r)| !["workflow", "runtime_workflow"].contains(&r.kind.as_str()))
                .map(|(k, r)| (k.clone(), r.content_hash.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        if plan.roles != old.plan.roles
            || definitions(&plan) != definitions(&old.plan)
            || json!(plan.permission_locks) != json!(old.plan.permission_locks)
        {
            affected.extend(old.plan.document.steps.iter().map(|s| s.key.clone()));
        }
        budget_updates.retain(|key| !affected.contains(key));
        for (name, port) in &document.inputs {
            if port.required {
                require(inputs.contains_key(name), format!("缺少流程输入 {name}"))?;
            }
            if let Some(v) = inputs.get(name) {
                self.validate_configured_value(&plan.definitions[&port.delivery_ref].decode()?, v)?;
            }
        }
        require(
            inputs.keys().all(|k| document.inputs.contains_key(k)),
            "包含未声明流程输入",
        )?;
        let footprint = json!({"executions":old.steps.iter().filter(|s|affected.contains(&s.step)).collect::<Vec<_>>(),"tasks":task_changes,
            "children":state.governance.iter().filter(|r|r.kind=="workflow_instance"&&r.data["root_instance_id"]==old.root_instance_id&&r.id!=row.id).map(|r|(&r.id,&r.data)).collect::<Vec<_>>()});
        let preview_hash = configuration_hash(&(
            changes, &document, &bindings, &inputs, &affected, &footprint,
        ));
        Ok(Preview {
            document,
            plan,
            bindings,
            inputs,
            affected,
            budget_updates,
            bypassed,
            waivers,
            footprint,
            preview_hash,
        })
    }

    pub(crate) fn preview_workflow_change(&self, p: &Value) -> Result<Value> {
        let state = self.store.snapshot()?;
        let row = state.configuration(
            p["id"]
                .as_str()
                .or_else(|| p["instance_id"].as_str())
                .unwrap_or(""),
        )?;
        require(row.kind == "workflow_instance", "不是工作流实例")?;
        let changes: WorkflowChangeSet = serde_json::from_value(p["changeset"].clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        let preview = self.change_preview_in(&state, row, &changes)?;
        let instance: ConfiguredWorkflowInstance = row.decode()?;
        let stop = instance
            .steps
            .iter()
            .filter(|e| preview.affected.contains(&e.step) && active_execution(&state, e))
            .map(|e| e.id.clone())
            .collect::<Vec<_>>();
        Ok(
            json!({"preview_hash":preview.preview_hash,"affected":preview.affected,"budget_updates":preview.budget_updates,"bypassed":preview.bypassed,"waivers":preview.waivers,"must_stop":stop,"document":preview.document,"plan_revision_id":instance.active_plan_revision_id,"changeset":changes}),
        )
    }

    pub(crate) fn apply_workflow_change(
        &self,
        c: &Command,
        state: &mut EngineeringState,
        actor: &WorkflowActor,
    ) -> Result<Value> {
        let row = state
            .configuration(c.target.as_deref().unwrap_or(""))?
            .clone();
        require(!pending(state, &row.id), "已有变更正在应用")?;
        let changes: WorkflowChangeSet = serde_json::from_value(c.payload["changeset"].clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        let preview = self.change_preview_in(state, &row, &changes)?;
        require(
            c.payload["preview_hash"] == preview.preview_hash,
            "预演已过期，请重新预演",
        )?;
        if let WorkflowActor::Controller { controller_id, .. } = actor {
            let controller: WorkflowController = state.configuration(controller_id)?.decode()?;
            for (_, requirement) in &preview.waivers {
                require(
                    controller
                        .policy
                        .waivable_requirements
                        .contains(requirement),
                    "编排策略不允许豁免此要求",
                )?;
            }
            if changes.operations.iter().any(|op| {
                matches!(
                    op,
                    WorkflowChangeOperation::ReviseTaskScope { .. }
                        | WorkflowChangeOperation::ReviseTaskDependencies { .. }
                )
            }) {
                require(
                    controller.policy.allow_task_revision,
                    "编排策略未允许修改 Task 范围",
                )?;
            }
        }
        let change = record(
            "workflow_change",
            row.id.clone(),
            row.scope.clone(),
            changes.reason.clone(),
            1,
            None,
            json!({"status":"prepared","changeset":changes,"preview":preview,"actor":actor}),
            false,
        );
        let id = change.id.clone();
        state.governance.push(change);
        self.advance_workflow_change(state, &id)?;
        Ok(json!({"id":id,"status":state.configuration(&id)?.data["status"]}))
    }

    fn advance_workflow_change(&self, state: &mut EngineeringState, id: &str) -> Result<()> {
        let change = state.configuration(id)?.clone();
        let row = state.configuration(&change.definition_id)?.clone();
        let mut instance: ConfiguredWorkflowInstance = row.decode()?;
        let changes: WorkflowChangeSet =
            serde_json::from_value(change.data["changeset"].clone())
                .map_err(|e| err("invalid_configuration", e.to_string()))?;
        let preview: Preview = serde_json::from_value(change.data["preview"].clone())
            .map_err(|e| err("invalid_configuration", e.to_string()))?;
        require(
            instance.active_plan_revision_id == changes.plan_revision_id,
            "变更基准已变化",
        )?;
        let affected_ids: BTreeSet<_> = instance
            .steps
            .iter()
            .filter(|e| preview.affected.contains(&e.step))
            .map(|e| e.id.clone())
            .collect();
        let mut descendants = BTreeSet::new();
        loop {
            let before = descendants.len();
            for r in state
                .governance
                .iter()
                .filter(|r| r.kind == "workflow_instance")
            {
                if r.data["parent_execution_id"]
                    .as_str()
                    .is_some_and(|parent| {
                        affected_ids.contains(parent)
                            || state.governance.iter().any(|ancestor| {
                                descendants.contains(&ancestor.id)
                                    && ancestor.data["steps"].as_array().is_some_and(|steps| {
                                        steps.iter().any(|s| s["id"] == parent)
                                    })
                            })
                    })
                {
                    descendants.insert(r.id.clone());
                }
            }
            if before == descendants.len() {
                break;
            }
        }
        let mut executing: Vec<StepExecution> = instance
            .steps
            .iter()
            .filter(|e| preview.affected.contains(&e.step))
            .cloned()
            .collect();
        for child in state
            .governance
            .iter()
            .filter(|r| descendants.contains(&r.id))
        {
            let child: ConfiguredWorkflowInstance = child.decode()?;
            executing.extend(child.steps);
        }
        let active: Vec<_> = executing
            .iter()
            .filter(|e| active_execution(state, e))
            .cloned()
            .collect();
        if !active.is_empty() {
            for execution in active {
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
            }
            let mut data = change.data.clone();
            data["status"] = json!("waiting_quiescence");
            update_record(
                state.governance.iter_mut().find(|r| r.id == id).unwrap(),
                data,
            );
            return Ok(());
        }
        for previous in preview.footprint["executions"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let actual = instance
                .steps
                .iter()
                .find(|e| e.id == previous["id"])
                .ok_or_else(|| err("stale_preview", "受影响执行已变化"))?;
            let actual = json!(actual);
            for key in [
                "inputs",
                "input_digest",
                "outputs",
                "contract_digest",
                "node_id",
            ] {
                require(
                    actual[key] == previous[key],
                    "停止期间结果或合同变化，请重新预演",
                )?;
            }
        }
        // Original execution contracts and late observations remain immutable history.
        for e in &executing {
            state.governance.push(record(
                "workflow_execution_history",
                e.id.clone(),
                row.scope.clone(),
                e.step.clone(),
                1,
                None,
                json!(e),
                true,
            ));
        }
        for child in state
            .governance
            .iter_mut()
            .filter(|r| descendants.contains(&r.id))
        {
            let mut d = child.data.clone();
            d["status"] = json!("cancelled");
            d["cancel_requested"] = json!(true);
            update_record(child, d);
        }
        let actor: WorkflowActor = serde_json::from_value(change.data["actor"].clone())
            .map_err(|e| err("invalid_configuration", e.to_string()))?;
        let previous_plan = instance.active_plan_revision_id.clone();
        if let Some(parent_execution) = &instance.parent_execution_id {
            require(
                instance
                    .plan
                    .document
                    .outputs
                    .keys()
                    .eq(preview.document.outputs.keys()),
                "子流程修订必须保持父流程输出端口合同",
            )?;
            for (name, old_source) in &instance.plan.document.outputs {
                let old_port = &instance
                    .plan
                    .document
                    .steps
                    .iter()
                    .find(|s| s.key == old_source.step)
                    .unwrap()
                    .outputs[&old_source.port];
                let new_source = &preview.document.outputs[name];
                let new_port = &preview
                    .document
                    .steps
                    .iter()
                    .find(|s| s.key == new_source.step)
                    .unwrap()
                    .outputs[&new_source.port];
                require(
                    old_port.delivery_ref == new_port.delivery_ref,
                    "子流程修订不能隐式更改父流程交付类型",
                )?;
            }
            state.governance.push(record("workflow_child_override",parent_execution.clone(),row.scope.clone(),changes.reason.clone(),1,None,json!({"child_instance_id":row.id,"from_plan":previous_plan,"change_id":change.id,"actor":actor}),true));
        }
        for e in &executing {
            if let Some(task_id) = state
                .governance
                .iter()
                .find(|r| {
                    r.kind == "workflow_instance"
                        && r.data["steps"]
                            .as_array()
                            .is_some_and(|steps| steps.iter().any(|s| s["id"] == e.id))
                })
                .and_then(|r| r.data["inputs"]["item"]["id"].as_str())
                .map(str::to_owned)
            {
                if let Some(task) = state.tasks.iter_mut().find(|t| t.id == task_id) {
                    if task.status == Status::Completed {
                        task.status = Status::Draft;
                        task.revision += 1;
                    }
                }
            }
        }
        let old_steps = instance.steps.clone();
        instance.plan = preview.plan.clone();
        instance.workflow_ref = preview.plan.workflow_revision_id.clone();
        instance.role_agent_bindings = preview.bindings.clone();
        instance.inputs = preview.inputs.clone();
        instance.steps = preview
            .document
            .steps
            .iter()
            .map(|step| {
                if !preview.affected.contains(&step.key) {
                    if let Some(e) = old_steps.iter().find(|e| e.node_id == step.node_id) {
                        return e.clone();
                    }
                }
                StepExecution {
                    plan_revision_id: String::new(),
                    node_id: step.node_id.clone(),
                    contract_digest: configuration_hash(step),
                    id: id::new_id(),
                    workflow_instance_id: row.id.clone(),
                    workflow_revision_id: instance.workflow_ref.clone(),
                    path: format!(
                        "{}/{}",
                        old_steps
                            .first()
                            .map(|e| e.path.rsplit_once('/').map(|(p, _)| p).unwrap_or(""))
                            .unwrap_or(""),
                        step.key
                    ),
                    step: step.key.clone(),
                    round: old_steps
                        .iter()
                        .find(|e| e.node_id == step.node_id)
                        .map_or(1, |e| e.round + 1),
                    sequence: 1,
                    status: if preview.bypassed.contains(&step.key) {
                        StepExecutionStatus::Skipped
                    } else {
                        StepExecutionStatus::Queued
                    },
                    input_digest: String::new(),
                    budget_wait_started: None,
                    budget_waited_ms: 0,
                    inputs: BTreeMap::new(),
                    outputs: BTreeMap::new(),
                    agent_revision_id: None,
                    session_id: None,
                    turn_id: None,
                    tool_call_id: None,
                    started: None,
                    finished: None,
                    reason: if preview.bypassed.contains(&step.key) {
                        changes.reason.clone()
                    } else {
                        String::new()
                    },
                }
            })
            .collect();
        // Publish the exact private definition inside this instance's history, not the template catalog.
        if let Some(definition) = instance.plan.definitions.get(&instance.workflow_ref) {
            if state.configuration(&definition.id).is_err() {
                state.governance.push(definition.clone());
            }
        }
        freeze_plan(state, &mut instance, &row.scope, &changes.reason)?;
        for execution in &instance.steps {
            if execution.plan_revision_id != instance.active_plan_revision_id {
                state.governance.push(record("workflow_carry_forward",execution.id.clone(),row.scope.clone(),execution.step.clone(),1,None,json!({"from_plan":previous_plan,"to_plan":instance.active_plan_revision_id,"source_plan":execution.plan_revision_id,"execution_id":execution.id,"contract_digest":execution.contract_digest,"input_digest":execution.input_digest,"outputs":execution.outputs,"invocation_timeout_extended":preview.budget_updates.contains(&execution.step)}),true));
            }
        }
        for e in instance
            .steps
            .iter()
            .filter(|e| preview.bypassed.contains(&e.step))
        {
            decision(
                state,
                &row,
                &instance,
                Some(e),
                &actor,
                "bypass",
                &changes.reason,
            );
        }
        for (node_id, requirement) in &preview.waivers {
            state.governance.push(record("workflow_waiver",row.id.clone(),row.scope.clone(),changes.reason.clone(),1,None,json!({"plan_revision_id":instance.active_plan_revision_id,"node_id":node_id,"requirement":requirement,"actor":actor,"change_id":change.id}),true));
        }
        for op in &changes.operations {
            match op {
                WorkflowChangeOperation::ReviseTaskScope {
                    task_id,
                    title,
                    scope,
                } => {
                    let task = state.tasks.iter_mut().find(|t| &t.id == task_id).unwrap();
                    task.title = title.clone();
                    task.scope = scope.clone();
                    task.revision += 1;
                }
                WorkflowChangeOperation::ReviseTaskDependencies {
                    task_id,
                    dependencies,
                } => {
                    let task = state.tasks.iter().find(|t| &t.id == task_id).unwrap();
                    for dep in dependencies {
                        require(
                            state.task(dep)?.feature_id == task.feature_id,
                            "Task 依赖跨 Feature",
                        )?;
                    }
                    let task = state.tasks.iter_mut().find(|t| &t.id == task_id).unwrap();
                    task.dependencies = dependencies.clone();
                    task.revision += 1;
                    valid_dag(
                        &state
                            .tasks
                            .iter()
                            .map(|t| (t.id.clone(), t.dependencies.clone()))
                            .collect::<Vec<_>>(),
                    )?;
                }
                _ => {}
            }
        }
        instance.control_version += 1;
        instance.status = StepExecutionStatus::Running;
        decision(
            state,
            &row,
            &instance,
            None,
            &actor,
            "change",
            &changes.reason,
        );
        update_instance(state, &row, &instance);
        let mut data = change.data.clone();
        data["status"] = json!("applied");
        data["new_plan_revision_id"] = json!(instance.active_plan_revision_id);
        update_record(
            state.governance.iter_mut().find(|r| r.id == id).unwrap(),
            data,
        );
        event(
            state,
            &instance.root_instance_id,
            "plan.changed",
            &instance.active_plan_revision_id,
        );
        Ok(())
    }

    pub fn drive_workflow_changes(&self) -> Result<()> {
        self.store.transact(None, &mut |state| {
            let ids: Vec<_> = state
                .governance
                .iter()
                .filter(|r| {
                    r.kind == "workflow_change"
                        && ["prepared", "waiting_quiescence"]
                            .contains(&r.data["status"].as_str().unwrap_or(""))
                })
                .map(|r| r.id.clone())
                .collect();
            for id in ids {
                let checkpoint = state.clone();
                if let Err(e) = self.advance_workflow_change(state, &id) {
                    *state = checkpoint;
                    let row = state.governance.iter_mut().find(|r| r.id == id).unwrap();
                    let mut data = row.data.clone();
                    data["status"] = json!("conflicted");
                    data["error"] = json!(e);
                    update_record(row, data);
                }
            }
            Ok(Value::Null)
        })?;
        Ok(())
    }

    pub(crate) fn dispatch_workflow_segment(
        &self,
        c: &Command,
        state: &mut EngineeringState,
        actor: &WorkflowActor,
    ) -> Result<Value> {
        let row = state
            .configuration(c.target.as_deref().unwrap_or(""))?
            .clone();
        let nodes: Vec<String> = serde_json::from_value(c.payload["steps"].clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        require(
            !nodes.is_empty() && nodes.len() <= 32,
            "委托需要 1—32 个确切节点",
        )?;
        let instance: ConfiguredWorkflowInstance = row.decode()?;
        for key in &nodes {
            let step = instance
                .plan
                .document
                .steps
                .iter()
                .find(|s| &s.key == key || &s.node_id == key)
                .ok_or_else(|| err("not_found", "委托节点不存在"))?;
            require(
                step.confirmation == ConfirmationPolicy::Automatic,
                "委托不能越过人类门禁",
            )?;
        }
        let r = record(
            "workflow_segment",
            row.id.clone(),
            row.scope.clone(),
            text(&c.payload, "reason")?,
            1,
            None,
            json!({"plan_revision_id":instance.active_plan_revision_id,"steps":nodes,"actor":actor,"status":"active","max_operations":64,"operations":0}),
            false,
        );
        let out = json!({"id":r.id,"status":"active"});
        state.governance.push(r);
        Ok(out)
    }

    pub fn drive_workflow_segments(&self) -> Result<()> {
        self.store.transact(None,&mut|state|{
            let rows:Vec<_>=state.governance.iter().filter(|r|r.kind=="workflow_segment"&&r.data["status"]=="active").cloned().collect();
            for segment in rows {
                let row=state.configuration(&segment.definition_id)?.clone();let instance:ConfiguredWorkflowInstance=row.decode()?;
                let mut data=segment.data.clone();
                let revoked=if data["actor"]["kind"]=="controller" {state.configuration(data["actor"]["controller_id"].as_str().unwrap_or("")).map_or(true,|r|r.data["status"]!="active"||r.data["epoch"]!=data["actor"]["epoch"])}else{false};
                if revoked||instance.active_plan_revision_id!=data["plan_revision_id"]||instance.cancel_requested||pending(state,&row.id){data["status"]=json!("stopped");data["reason"]=json!("计划变化或执行停止");}
                else if state.workflow_instance_suspended(&row.id){continue;}
                else if data["steps"].as_array().unwrap().iter().all(|key|instance.steps.iter().any(|e|(e.step==key.as_str().unwrap_or("")||e.node_id==key.as_str().unwrap_or(""))&&matches!(e.status,StepExecutionStatus::Completed|StepExecutionStatus::Skipped))){data["status"]=json!("completed");}
                else if data["operations"].as_u64().unwrap_or(0)>=data["max_operations"].as_u64().unwrap_or(64){data["status"]=json!("stopped");data["reason"]=json!("委托预算耗尽");}
                else{
                    let mut complete=true;
                    for key in data["steps"].as_array().unwrap(){
                        let Some(e)=instance.steps.iter().find(|e|e.step==key.as_str().unwrap_or("")||e.node_id==key.as_str().unwrap_or(""))else{data["status"]=json!("stopped");break;};
                        if matches!(e.status,StepExecutionStatus::Completed|StepExecutionStatus::Skipped){continue;}
                        complete=false;
                        if matches!(e.status,StepExecutionStatus::Failed|StepExecutionStatus::Cancelled|StepExecutionStatus::WaitingInput|StepExecutionStatus::WaitingConfirmation|StepExecutionStatus::NeedsReconciliation){data["status"]=json!("stopped");data["reason"]=json!(e.reason);break;}
                        let step=instance.plan.document.steps.iter().find(|s|s.key==e.step).unwrap();
                        if e.status==StepExecutionStatus::AwaitingAcceptance&&!crate::orchestration::rule_satisfied(state,&instance,&step.node_id,&step.completion,&crate::configured_runtime::facts(state,&instance)?) {data["status"]=json!("stopped");data["reason"]=json!("交付未满足完成条件，需要重新决策");break;}
                        let method=if e.status==StepExecutionStatus::Queued&&step.dependencies.iter().all(|k|instance.steps.iter().any(|e|&e.step==k&&matches!(e.status,StepExecutionStatus::Completed|StepExecutionStatus::Skipped))){"workflow.step.dispatch"}
                            else if e.status==StepExecutionStatus::AwaitingAcceptance&&crate::orchestration::rule_satisfied(state,&instance,&step.node_id,&step.completion,&crate::configured_runtime::facts(state,&instance)?){"workflow.step.accept"}else{continue;};
                        let c=Command{command_id:format!("segment:{}:{}:{}",segment.id,method,e.id),target:Some(row.id.clone()),expected_revision:Some(row.revision),payload:json!({"plan_revision_id":instance.active_plan_revision_id,"step":e.step,"execution_id":e.id,"input_digest":e.input_digest,"outputs":e.outputs,"reason":"有界委托"})};
                        let checkpoint=state.clone();let actor=WorkflowActor::Segment{authorization_id:segment.id.clone()};
                        match self.workflow_control_command(method,&c,state,&actor){Ok(Some(_))=>{data["operations"]=json!(data["operations"].as_u64().unwrap_or(0)+1);},Ok(None)=>{},Err(error)=>{*state=checkpoint;data["status"]=json!("stopped");data["error"]=json!(error);}}
                        break;
                    }
                    if complete{data["status"]=json!("completed");}
                }
                update_record(state.governance.iter_mut().find(|r|r.id==segment.id).unwrap(),data);
            }Ok(Value::Null)
        })?;
        Ok(())
    }
}

mod id {
    pub fn new_id() -> String {
        devdesk_domain::id()
    }
}
