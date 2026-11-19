//! Plan preview over a complete configuration catalog and explicitly selected
//! actual runtime facts. No whole-project snapshot or synthetic runtime state.
use crate::{
    concurrency::*,
    governance::record,
    platform_facts::{one, restored},
    workflow_commits::WorkflowHeaderFacts,
    workflow_resources::version,
    *,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct NativePreview {
    pub plan: ConfiguredWorkflowPlan,
    pub bindings: BTreeMap<Id, Id>,
    pub inputs: BTreeMap<String, Value>,
    pub affected: BTreeSet<String>,
    #[serde(default)]
    pub budget_updates: BTreeSet<String>,
    #[serde(default)]
    pub aggregation_updates: BTreeSet<String>,
    pub bypassed: BTreeSet<String>,
    pub waivers: Vec<(String, String)>,
    pub footprint: Value,
    pub preview_hash: String,
    #[serde(default)]
    pub standard_diff: Value,
    #[serde(default)]
    pub restored_executions: Vec<StepExecution>,
    #[serde(default)]
    pub reverted_change_id: Option<Id>,
}
pub(crate) struct ChangeFacts {
    pub catalog: Arc<ConfigurationCatalogSnapshot>,
    pub records: BTreeMap<Id, ConfigurationRecord>,
    pub tasks: BTreeMap<Id, Task>,
    pub features: BTreeMap<Id, Feature>,
    pub reads: ReadSet,
}
impl ChangeFacts {
    fn record(&self, id: &str) -> Result<&ConfigurationRecord> {
        self.records
            .get(id)
            .or_else(|| self.catalog.record(id))
            .ok_or_else(|| err("not_found", format!("计划变更来源缺失: {id}")))
    }
    fn task(&self, id: &str) -> Result<&Task> {
        self.tasks
            .get(id)
            .ok_or_else(|| err("not_found", "Task 事实缺失"))
    }
    fn feature(&self, id: &str) -> Result<&Feature> {
        self.features
            .get(id)
            .ok_or_else(|| err("not_found", "Feature 事实缺失"))
    }
    fn row(
        &mut self,
        engine: &Engine,
        kind: &str,
        id: &str,
    ) -> Result<Option<ConfigurationRecord>> {
        let row = one(engine.store.as_ref(), kind, id)?;
        self.reads = self.reads.merge([version(&row)])?;
        if row.value.is_none() {
            return Ok(None);
        }
        let actual = restored(engine, &row)?;
        self.records.insert(actual.id.clone(), actual.clone());
        Ok(Some(actual))
    }
}
impl Engine {
    pub(crate) fn unchanged_change_executions(
        &self,
        root: &WorkflowHeaderFacts,
        change_id: &str,
    ) -> Result<(Vec<StepExecution>, ReadSet)> {
        let selected = one(self.store.as_ref(), "governance", change_id)?;
        let change = restored(self, &selected)?;
        require(
            change.r#type == "workflow_change"
                && change.definition_id == root.record.id
                && change.data["status"] == "applied"
                && change.data["new_plan_revision_id"] == root.instance.active_plan_revision_id,
            "只能核对当前计划的已应用变更",
        )?;
        let changes: WorkflowChangeSet = serde_json::from_value(change.data["changeset"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        require(
            changes.operations.iter().all(|op| {
                matches!(
                    op,
                    WorkflowChangeOperation::ReplacePlan { .. }
                        | WorkflowChangeOperation::UpsertStep { .. }
                        | WorkflowChangeOperation::UpsertStage { .. }
                )
            }),
            "只能恢复节点修订中未受影响的执行，不能撤销输入、范围或外部作用",
        )?;
        let mut facts = self.native_change_facts(root, &changes)?;
        facts.reads = facts.reads.merge([version(&selected)])?;
        let previous = facts
            .row(self, "governance", &changes.plan_revision_id)?
            .ok_or_else(|| err("not_found", "原冻结计划缺失"))?;
        require(
            previous.r#type == "workflow_plan_revision"
                && previous.data["instance_id"] == root.record.id,
            "原冻结计划归属不符",
        )?;
        let source: NativePreview = serde_json::from_value(change.data["preview"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        require(
            source.plan.document == root.instance.plan.document
                && previous.data["inputs"] == json!(root.instance.inputs)
                && previous.data["role_agent_bindings"] == json!(root.instance.role_agent_bindings),
            "应用后的合同、输入或绑定已变化",
        )?;
        let archived: Vec<StepExecution> =
            serde_json::from_value(source.footprint["executions"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        for execution in &archived {
            let history = facts
                .row(self, "workflow_history_source", &execution.id)?
                .ok_or_else(|| err("not_found", "不可变执行历史缺失"))?;
            require(
                history.immutable
                    && history.r#type == "workflow_execution_history"
                    && history.definition_id == execution.id
                    && history.data == json!(execution),
                "原执行与不可变历史不符",
            )?;
        }
        let mut original = root.instance.as_ref().clone();
        original.plan = serde_json::from_value(previous.data["plan"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        original.workflow_ref = original.plan.workflow_revision_id.clone();
        original.active_plan_revision_id = changes.plan_revision_id.clone();
        original.control_version = change.data["control_version"]
            .as_u64()
            .ok_or_else(|| err("storage", "原控制版本缺失"))?;
        original.steps = original
            .plan
            .document
            .steps
            .iter()
            .map(|step| {
                archived
                    .iter()
                    .chain(root.instance.steps.iter())
                    .find(|e| e.node_id == step.node_id)
                    .cloned()
                    .ok_or_else(|| err("storage", "原执行集合不完整"))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut row = root.record.as_ref().clone();
        row.data = json!(original);
        let corrected = self.native_change_preview_in(&facts, &row, &changes)?;
        let restored = archived
            .into_iter()
            .filter(|execution| {
                !corrected.affected.contains(&execution.step)
                    && matches!(
                        execution.status,
                        StepExecutionStatus::Completed | StepExecutionStatus::Skipped
                    )
                    && root.instance.plan.document.steps.iter().any(|step| {
                        step.node_id == execution.node_id
                            && !matches!(step.executor, StepExecutor::Workflow { .. })
                            && configuration_hash(step) == execution.contract_digest
                    })
            })
            .collect::<Vec<_>>();
        require(!restored.is_empty(), "没有可恢复的未受影响执行")?;
        Ok((restored, facts.reads))
    }

    pub(crate) fn native_change_facts(
        &self,
        root: &WorkflowHeaderFacts,
        change: &WorkflowChangeSet,
    ) -> Result<ChangeFacts> {
        let catalog = self.store.configuration_snapshot()?;
        let mut facts = ChangeFacts {
            reads: ReadSet::new(vec![
                version(&root.header),
                ResourceVersion {
                    resource: ResourceKey::new("catalog_clock", "configuration"),
                    revision: catalog.revision,
                },
            ])?,
            catalog,
            records: BTreeMap::new(),
            tasks: BTreeMap::new(),
            features: BTreeMap::new(),
        };
        let mut todo = vec![(root.record.id.clone(), root.instance.clone(), 0)];
        let mut seen = BTreeSet::new();
        while let Some((id, instance, depth)) = todo.pop() {
            require(
                depth <= WORKFLOW_DEPTH_LIMIT
                    && seen.insert(id.clone())
                    && seen.len() <= WORKFLOW_STEP_LIMIT,
                "实际子树超出预算或有环",
            )?;
            if id != root.record.id {
                let row = WorkflowHeaderFacts::read(self, &id)?;
                facts.reads = facts.reads.merge([version(&row.header)])?;
                facts
                    .records
                    .insert(id.clone(), row.record.as_ref().clone());
            }
            if let Some(task_id) = instance.inputs.get("item").and_then(|v| v["id"].as_str()) {
                let selected = one(self.store.as_ref(), "task", task_id)?;
                facts.reads = facts.reads.merge([version(&selected)])?;
                if let Some(raw) = selected.value {
                    let task: Task =
                        serde_json::from_value(raw).map_err(|e| err("storage", e.to_string()))?;
                    facts.tasks.insert(task.id.clone(), task);
                }
            }
            for e in &instance.steps {
                if matches!(
                    instance
                        .plan
                        .document
                        .steps
                        .iter()
                        .find(|s| s.node_id == e.node_id)
                        .map(|s| &s.executor),
                    Some(StepExecutor::Workflow { .. })
                ) {
                    facts.row(self, "workflow_stop_source", &e.id)?;
                }
                if let Some(children) = facts.row(self, "workflow_children_source", &e.id)? {
                    for id in children.data["children"]
                        .as_array()
                        .ok_or_else(|| err("storage", "子流程集合缺失"))?
                    {
                        let child = WorkflowHeaderFacts::read(
                            self,
                            id.as_str()
                                .ok_or_else(|| err("storage", "子流程 ID 无效"))?,
                        )?;
                        require(
                            child.instance.parent_execution_id.as_deref() == Some(e.id.as_str()),
                            "子流程实际归属不符",
                        )?;
                        todo.push((child.record.id.clone(), child.instance.clone(), depth + 1));
                    }
                }
                if e.status == StepExecutionStatus::Skipped {
                    facts.row(
                        self,
                        "workflow_change_for_plan",
                        &json!([id, e.plan_revision_id]).to_string(),
                    )?;
                }
            }
        }
        for op in &change.operations {
            match op {
                WorkflowChangeOperation::AdoptReworkBaseline { policy, .. } => {
                    let id = crate::workflow_rework::budget_id(&root.instance, policy);
                    require(
                        facts.row(self, "governance", &id)?.is_none(),
                        "已有业务返工计数不能重新起算",
                    )?;
                }
                WorkflowChangeOperation::RevertChange { change_id } => {
                    let prior = facts
                        .row(self, "governance", change_id)?
                        .ok_or_else(|| err("not_found", "原变更缺失"))?;
                    let old = text(&prior.data["changeset"], "plan_revision_id")?;
                    facts.row(self, "governance", &old)?;
                    for e in prior.data["preview"]["footprint"]["executions"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        facts.row(self, "workflow_history_source", &text(e, "id")?)?;
                    }
                }
                WorkflowChangeOperation::ReviseTaskScope { task_id, .. }
                | WorkflowChangeOperation::ReviseTaskDependencies { task_id, .. } => {
                    let selected = one(self.store.as_ref(), "task", task_id)?;
                    facts.reads = facts.reads.merge([version(&selected)])?;
                    let task: Task = serde_json::from_value(
                        selected
                            .value
                            .ok_or_else(|| err("not_found", "原 Task 缺失"))?,
                    )
                    .map_err(|e| err("storage", e.to_string()))?;
                    let feature = one(self.store.as_ref(), "feature", &task.feature_id)?;
                    facts.reads = facts.reads.merge([version(&feature)])?;
                    let feature: Feature = serde_json::from_value(
                        feature
                            .value
                            .ok_or_else(|| err("not_found", "原 Feature 缺失"))?,
                    )
                    .map_err(|e| err("storage", e.to_string()))?;
                    facts.features.insert(feature.id.clone(), feature);
                    facts.tasks.insert(task.id.clone(), task);
                }
                _ => {}
            }
        }
        fn materials(value: &Value, ids: &mut BTreeSet<Id>) {
            match value {
                Value::Object(fields) => {
                    if fields.get("type").and_then(Value::as_str) == Some("material")
                        && let Some(id) = fields.get("material_ref").and_then(Value::as_str)
                    {
                        ids.insert(id.into());
                    }
                    for value in fields.values() {
                        materials(value, ids);
                    }
                }
                Value::Array(values) => {
                    for value in values {
                        materials(value, ids);
                    }
                }
                _ => {}
            }
        }
        let mut ids = BTreeSet::new();
        materials(&json!(change), &mut ids);
        materials(&json!(root.instance.plan.document), &mut ids);
        require(ids.len() <= WORKFLOW_STEP_LIMIT, "材料集合超过预算")?;
        for id in ids {
            facts.row(self, "governance", &id)?;
        }
        Ok(facts)
    }
    pub(crate) fn prepare_native_change_preview(
        &self,
        root: &WorkflowHeaderFacts,
        change: &WorkflowChangeSet,
    ) -> Result<(NativePreview, ChangeFacts)> {
        let facts = self.native_change_facts(root, change)?;
        let preview = self.native_change_preview_in(&facts, &root.record, change)?;
        Ok((preview, facts))
    }
    pub(crate) fn preview_native_workflow_change(&self, p: &Value) -> Result<Value> {
        let root = WorkflowHeaderFacts::read(
            self,
            p["id"]
                .as_str()
                .or_else(|| p["instance_id"].as_str())
                .ok_or_else(|| err("invalid_params", "缺少流程"))?,
        )?;
        let change: WorkflowChangeSet = serde_json::from_value(p["changeset"].clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        let (preview, _) = self.prepare_native_change_preview(&root, &change)?;
        let c = Command {
            command_id: "preview-only".into(),
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
        let mut selection = crate::workflow_commits::WorkflowCommit::new(
            self,
            "query.workflow.change.preview",
            &c,
            &context,
        )?;
        let mut stop = vec![];
        for e in root
            .instance
            .steps
            .iter()
            .filter(|e| preview.affected.contains(&e.step))
        {
            if selection.execution_active(&root.step(self, &e.id)?)? {
                stop.push(e.id.clone());
            }
        }
        Ok(
            json!({"preview_hash":preview.preview_hash,"standard_ref":preview.plan.standard_ref,"standard_diff":preview.standard_diff,"invalidation_causes":preview.footprint["invalidation_causes"],"before_standard_ref":root.instance.plan.standard_ref,"affected":preview.affected,"budget_updates":preview.budget_updates,"aggregation_updates":preview.aggregation_updates,"bypassed":preview.bypassed,"waivers":preview.waivers,"must_stop":stop,"document":preview.plan.document,"plan_revision_id":root.instance.active_plan_revision_id,"changeset":change,"reverted_change_id":preview.reverted_change_id,"restored_execution_ids":preview.restored_executions.iter().map(|e|&e.id).collect::<Vec<_>>()}),
        )
    }
    fn revert_native_preview(
        &self,
        facts: &ChangeFacts,
        row: &ConfigurationRecord,
        old: &ConfiguredWorkflowInstance,
        changes: &WorkflowChangeSet,
        change_id: &str,
    ) -> Result<NativePreview> {
        let change = facts.record(change_id)?;
        require(
            change.r#type == "workflow_change"
                && change.definition_id == row.id
                && change.data["status"] == "applied"
                && change.data["new_plan_revision_id"] == old.active_plan_revision_id,
            "只能回退本实例最近已应用的变更",
        )?;
        let source_changes: WorkflowChangeSet =
            serde_json::from_value(change.data["changeset"].clone())
                .map_err(|e| err("invalid_configuration", e.to_string()))?;
        require(
            source_changes
                .operations
                .iter()
                .all(|op| matches!(op, WorkflowChangeOperation::UpsertStep { .. })),
            "只支持回退节点修订，不撤销输入、范围、豁免或外部作用",
        )?;
        let previous = facts.record(&source_changes.plan_revision_id)?;
        require(
            previous.r#type == "workflow_plan_revision" && previous.data["instance_id"] == row.id,
            "原计划不属于此实例",
        )?;
        let source: NativePreview = serde_json::from_value(change.data["preview"].clone())
            .map_err(|e| err("invalid_configuration", e.to_string()))?;
        let plan: ConfiguredWorkflowPlan = serde_json::from_value(previous.data["plan"].clone())
            .map_err(|e| err("invalid_configuration", e.to_string()))?;
        require(
            json!(old.plan) == json!(source.plan)
                && json!(old.inputs) == previous.data["inputs"]
                && json!(old.role_agent_bindings) == previous.data["role_agent_bindings"],
            "当前合同或输入已变化",
        )?;
        // Child calls could have changed descendants even without tool outputs.
        require(
            plan.document
                .steps
                .iter()
                .chain(source.plan.document.steps.iter())
                .all(|s| {
                    !source.affected.contains(&s.key)
                        || !matches!(s.executor, StepExecutor::Workflow { .. })
                }),
            "回退不能恢复子流程调用",
        )?;
        let executions: Vec<_> = old
            .steps
            .iter()
            .filter(|e| source.affected.contains(&e.step))
            .cloned()
            .collect();
        require(
            executions.iter().all(|e| {
                e.status == StepExecutionStatus::Queued
                    && e.inputs.is_empty()
                    && e.input_digest.is_empty()
                    && e.outputs.is_empty()
                    && e.session_id.is_none()
                    && e.turn_id.is_none()
                    && e.tool_call_id.is_none()
                    && e.started.is_none()
                    && e.finished.is_none()
            }),
            "替代执行已经启动，不能回退",
        )?;
        let restored: Vec<StepExecution> =
            serde_json::from_value(source.footprint["executions"].clone())
                .map_err(|e| err("invalid_configuration", e.to_string()))?;
        require(!restored.is_empty(), "原变更没有可恢复执行")?;
        for execution in &restored {
            require(
                execution.workflow_instance_id == row.id
                    && facts.records.values().any(|r| {
                        r.r#type == "workflow_execution_history"
                            && r.definition_id == execution.id
                            && r.data == json!(execution)
                    })
                    && plan.document.steps.iter().any(|s| {
                        s.node_id == execution.node_id
                            && configuration_hash(s) == execution.contract_digest
                    }),
                "原执行与不可变历史或原合同不符",
            )?;
        }
        let footprint = json!({"executions":executions,"tasks":[],"children":[]});
        let preview_hash = configuration_hash(&(changes, &plan, &restored, &footprint));
        Ok(NativePreview {
            standard_diff: json!({"restored_standard_ref":plan.standard_ref}),
            plan,
            bindings: old.role_agent_bindings.clone(),
            inputs: old.inputs.clone(),
            affected: source.affected,
            budget_updates: BTreeSet::new(),
            aggregation_updates: BTreeSet::new(),
            bypassed: BTreeSet::new(),
            waivers: vec![],
            footprint,
            preview_hash,
            restored_executions: restored,
            reverted_change_id: Some(change_id.to_owned()),
        })
    }

    fn native_change_preview_in(
        &self,
        facts: &ChangeFacts,
        row: &ConfigurationRecord,
        changes: &WorkflowChangeSet,
    ) -> Result<NativePreview> {
        changes.validate()?;
        let old: ConfiguredWorkflowInstance = row.decode()?;
        require(
            changes.plan_revision_id == old.active_plan_revision_id,
            "计划已变化",
        )?;
        if let Some(change_id) = changes.operations.iter().find_map(|op| match op {
            WorkflowChangeOperation::RevertChange { change_id } => Some(change_id),
            _ => None,
        }) {
            require(changes.operations.len() == 1, "回退必须单独预演和应用")?;
            return self.revert_native_preview(facts, row, &old, changes, change_id);
        }
        let mut document = old.plan.document.clone();
        let mut bindings = old.role_agent_bindings.clone();
        let mut inputs = old.inputs.clone();
        let mut bypassed: BTreeSet<String> = old
            .steps
            .iter()
            .filter(|e| {
                e.status == StepExecutionStatus::Skipped
                    && facts.records.values().any(|r| {
                        r.r#type == "workflow_change"
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
        let mut adopted: Option<(Id, Id)> = None;
        let mut permission_selection = old.plan.harness_permission_ref.clone();
        let mut execution_selection = old.plan.execution_selection.clone();
        for op in &changes.operations {
            match op {
                WorkflowChangeOperation::SetReviewPolicies {
                    rework_policies,
                    review_policies,
                } => {
                    document.rework_policies = rework_policies.clone();
                    document.review_policies = review_policies.clone();
                }
                WorkflowChangeOperation::AdoptReworkBaseline { used, evidence, .. } => {
                    require(
                        *used <= 100_000 && !evidence.trim().is_empty() && evidence.len() <= 8192,
                        "采用历史返工基准需要明确次数及有界依据说明",
                    )?;
                }
                WorkflowChangeOperation::AdoptStandard {
                    standard_ref,
                    workflow_ref,
                    harness_permission_ref,
                } => {
                    if let Some(context) = harness_permission_ref {
                        crate::governance::reference(
                            &facts.catalog.state,
                            context,
                            "permission_profile",
                            &row.scope,
                        )?;
                        permission_selection = Some(context.clone());
                        execution_selection.harness = PermissionSelection::Profile {
                            permission_revision_id: context.clone(),
                        };
                    }
                    require(adopted.is_none(), "一次变更只能选择一份目标 Standard")?;
                    standard_entry(&facts.catalog.state, standard_ref, workflow_ref, &row.scope)?;
                    document = facts.catalog.state.resolve_workflow(workflow_ref)?;
                    document.assign_node_ids(workflow_ref);
                    adopted = Some((standard_ref.clone(), workflow_ref.clone()));
                }
                WorkflowChangeOperation::RevertChange { .. } => unreachable!(),
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
                    if node_id == "$workflow" && requirement == "coverage" {
                        fn has_covers(rule: &CompletionRule) -> bool {
                            match rule {
                                CompletionRule::Covers { .. } => true,
                                CompletionRule::All { rules } | CompletionRule::Any { rules } => {
                                    rules.iter().any(has_covers)
                                }
                                _ => false,
                            }
                        }
                        require(
                            has_covers(&document.completion),
                            "Standard 覆盖规则已移除；仅可豁免明确的 Workflow 完成要求",
                        )?;
                    }
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
                        if let InputBinding::Source { source } = binding
                            && members.contains(&source.step)
                        {
                            let key = format!("{}.{}", source.step, source.port);
                            let replacement = substitutions.get(&key).ok_or_else(|| {
                                err(
                                    "missing_material",
                                    format!("跳过阶段后缺少 {key} 的替代输入"),
                                )
                            })?;
                            if let InputBinding::Material { material_ref } = replacement {
                                let material = facts.record(material_ref)?;
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
                    let task = facts.task(task_id)?;
                    require(
                        facts.feature(&task.feature_id)?.project_id == old.project_id,
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
        for op in &changes.operations {
            if let WorkflowChangeOperation::AdoptReworkBaseline { policy, .. } = op {
                require(
                    document.rework_policies.contains_key(policy),
                    "历史额度基准必须绑定已采用的返工策略",
                )?;
            }
        }
        let mut seeds = BTreeSet::new();
        let mut budget_updates = BTreeSet::new();
        let mut aggregation_updates = BTreeSet::new();
        for step in &old.plan.document.steps {
            let next = document.steps.iter().find(|s| s.node_id == step.node_id);
            require(
                !old.steps
                    .iter()
                    .any(|e| e.node_id == step.node_id && e.time_budget.is_some())
                    || next.is_none_or(|next| next.budget.time_limits == step.budget.time_limits),
                "已开始的逻辑执行不能替换或移除原时间政策；请保留原期限或明确启动后继工作",
            )?;
            // Tool arguments/permissions/receipts do not depend on their display
            // stage. Regrouping an otherwise identical tool must not rerun it.
            // Agent stage context and inherited Roles remain business changes.
            let regroups_tool = next.is_some_and(|next| {
                let mut business = next.clone();
                business.stage = step.stage.clone();
                matches!(step.executor, StepExecutor::Tool { .. }) && business == *step
            });
            let extends_invocation_budget = next.is_some_and(|next| {
                let mut business = next.clone();
                business.budget.timeout_seconds = step.budget.timeout_seconds;
                matches!(step.executor, StepExecutor::Workflow { .. })
                    && next.budget.timeout_seconds > step.budget.timeout_seconds
                    && business == *step
                    && old.steps.iter().any(|e| {
                        e.node_id == step.node_id
                            && e.started.is_some()
                            && facts.records.values().any(|r| {
                                r.r#type == "workflow_instance"
                                    && r.data["parent_execution_id"] == e.id
                            })
                    })
            });
            if extends_invocation_budget {
                budget_updates.insert(step.key.clone());
            }
            // An output-only amendment can revalidate the original completed
            // children. It cannot restart, replace or relabel any child effect.
            if next.is_some_and(|next| {
                let mut business = next.clone();
                business.outputs = step.outputs.clone();
                next.outputs != step.outputs
                    && business == *step
                    && matches!(step.executor, StepExecutor::Workflow { .. })
                    && old.steps.iter().any(|e| {
                        e.node_id == step.node_id
                            && e.started.is_some()
                            && e.outputs.is_empty()
                            && matches!(
                                e.status,
                                StepExecutionStatus::Running
                                    | StepExecutionStatus::Failed
                                    | StepExecutionStatus::NeedsReconciliation
                            )
                            && !facts.records.values().any(|r| {
                                matches!(
                                    r.r#type.as_str(),
                                    "workflow_stop" | "workflow_stop_reassertion"
                                ) && r.definition_id == e.id
                            })
                            && facts
                                .records
                                .values()
                                .find(|r| {
                                    r.r#type == "workflow_children" && r.definition_id == e.id
                                })
                                .is_some_and(|group| {
                                    group.data["children"].as_array().is_some_and(|ids| {
                                        !ids.is_empty()
                                            && ids.iter().all(|id| {
                                                facts
                                                    .records
                                                    .get(id.as_str().unwrap_or_default())
                                                    .is_some_and(|child| {
                                                        child.r#type == "workflow_instance"
                                                            && child.data["parent_execution_id"]
                                                                == e.id
                                                            && child.data["status"] == "completed"
                                                    })
                                            })
                                    })
                                })
                    })
            }) {
                aggregation_updates.insert(step.key.clone());
            }
            if (next != Some(step) && !extends_invocation_budget && !regroups_tool)
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
        if document.action_requirements != old.plan.document.action_requirements
            || document.delivery_policy != old.plan.document.delivery_policy
        {
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
                    let m = facts.record(material_ref)?;
                    require(
                        m.r#type == "typed_material"
                            && m.data["project_id"] == old.project_id
                            && m.data["root_instance_id"] == old.root_instance_id,
                        "材料来源跨流程",
                    )?;
                }
            }
        }
        let base_source = facts.record(&old.plan.workflow_revision_id)?;
        document.assign_node_ids(&base_source.definition_id);
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
        candidate.definition_id = format!("runtime-workflow.{}", configuration_hash(changes));
        candidate.version = 1;
        candidate.id = format!("{}:1", candidate.definition_id);
        candidate.created_at = 0;
        if adopted.is_none() && document == old.plan.document {
            candidate = base_source.clone();
        }
        if let Some((_, workflow)) = &adopted {
            let target = facts.record(workflow)?;
            let mut comparison = facts.catalog.state.resolve_workflow(workflow)?;
            comparison.assign_node_ids(workflow);
            if comparison == document {
                candidate = target.clone();
            }
        }
        let mut context = crate::frozen_views::selection_configuration(&execution_selection);
        context["workflow_ref"] = json!(candidate.id);
        context["base_workflow_ref"] = json!(if adopted.is_some()
            && candidate.id != old.plan.workflow_revision_id
        {
            Some(candidate.id.clone())
        } else {
            old.plan.base_workflow_ref.clone()
        });
        let mut selected = crate::standards::selection_state(&facts.catalog.state, &context);
        crate::frozen_views::pin_permission_defaults(&mut selected, &old.plan.permission_locks)?;
        let mut business_document = document.clone();
        business_document.stages = old.plan.document.stages.clone();
        for step in &mut business_document.steps {
            if matches!(step.executor, StepExecutor::Tool { .. })
                && let Some(previous) = old
                    .plan
                    .document
                    .steps
                    .iter()
                    .find(|s| s.node_id == step.node_id)
            {
                step.stage = previous.stage.clone();
            }
        }
        let layout_only = changes.operations.iter().all(|operation| {
            matches!(
                operation,
                WorkflowChangeOperation::ReplacePlan { .. }
                    | WorkflowChangeOperation::UpsertStep { .. }
                    | WorkflowChangeOperation::UpsertStage { .. }
            )
        }) && business_document == old.plan.document
            && old.plan.document.stages.iter().all(|stage| {
                document.stages.iter().find(|next| next.key == stage.key) == Some(stage)
            });
        let mut contracts_only = document.clone();
        contracts_only.rework_policies = old.plan.document.rework_policies.clone();
        contracts_only.review_policies = old.plan.document.review_policies.clone();
        let policy_only = changes.operations.iter().all(|op| {
            matches!(
                op,
                WorkflowChangeOperation::SetReviewPolicies { .. }
                    | WorkflowChangeOperation::AdoptReworkBaseline { .. }
                    | WorkflowChangeOperation::ReplacePlan { .. }
            )
        }) && contracts_only == old.plan.document
            && adopted.is_none()
            && bindings == old.role_agent_bindings
            && inputs == old.inputs;
        let mut plan = if layout_only || policy_only {
            // Display regrouping must not resolve today's project configuration
            // into yesterday's execution. Keep all original frozen assets.
            let mut plan = old.plan.clone();
            plan.document = document.clone();
            plan.workflow_revision_id = candidate.id.clone();
            plan
        } else {
            self.compile_configured_workflow(&selected, &candidate, &row.scope, Some(&bindings))?
        };
        plan.standard_ref = adopted
            .as_ref()
            .map(|(s, _)| s.clone())
            .or_else(|| old.plan.standard_ref.clone());
        plan.work_configuration_ref = old.plan.work_configuration_ref.clone();
        plan.harness_permission_ref = permission_selection;
        let definitions = |p: &ConfiguredWorkflowPlan| {
            p.definitions
                .iter()
                .filter(|(_, r)| {
                    !["workflow", "runtime_workflow", "standard"].contains(&r.r#type.as_str())
                })
                .map(|(k, r)| (k.clone(), r.content_hash.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        // Children inherit their parent's compiled closure. Dropping unused parent
        // resources does not change any child execution contract.
        let old_definitions = definitions(&old.plan);
        // Root Step locks are addressed by Workflow reference + Step key.
        // A new immutable Workflow namespace is not a permission change.
        let root_lock_prefix = format!("{}:", plan.workflow_revision_id);
        let old_permission = |key: &str| {
            let previous = key
                .strip_prefix(&root_lock_prefix)
                .map(|step| format!("{}:{step}", old.plan.workflow_revision_id))
                .unwrap_or_else(|| key.to_owned());
            old.plan.permission_locks.get(&previous)
        };
        let closure_changed = plan
            .roles
            .iter()
            .any(|(k, v)| old.plan.roles.get(k).is_some_and(|old| old != v))
            || definitions(&plan)
                .iter()
                .any(|(k, v)| old_definitions.get(k).is_some_and(|old| old != v))
            || plan
                .permission_locks
                .iter()
                .any(|(k, v)| old_permission(k).is_some_and(|old| json!(old) != json!(v)))
            || plan.integration_locks.iter().any(|(k, v)| {
                old.plan
                    .integration_locks
                    .get(k)
                    .is_some_and(|old| json!(old) != json!(v))
            });
        // Assets introduced by a new node are not changes to an old execution.
        // Its own contract and dependency closure are already included in seeds.
        // Only changes to previously frozen assets invalidate the whole plan.
        if closure_changed {
            affected.extend(old.plan.document.steps.iter().map(|s| s.key.clone()));
        }
        budget_updates.retain(|key| !affected.contains(key));
        // Preserve an invocation only when its output contract is the sole
        // changed business input. Its consumers still require fresh execution.
        if closure_changed
            || bindings != old.role_agent_bindings
            || inputs != old.inputs
            || !task_changes.is_empty()
            || seeds.iter().any(|key| !aggregation_updates.contains(key))
        {
            aggregation_updates.clear();
        }
        for key in &aggregation_updates {
            affected.remove(key);
        }
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
        let mut reset_tasks = BTreeSet::new();
        let mut affected_executions = old
            .steps
            .iter()
            .filter(|e| affected.contains(&e.step))
            .map(|e| e.id.clone())
            .collect::<BTreeSet<_>>();
        if !affected_executions.is_empty()
            && let Some(id) = old.inputs.get("item").and_then(|v| v["id"].as_str())
        {
            reset_tasks.insert(id.to_owned());
        }
        loop {
            let before = affected_executions.len();
            for child in facts
                .records
                .values()
                .filter(|r| r.r#type == "workflow_instance")
            {
                if child.data["parent_execution_id"]
                    .as_str()
                    .is_some_and(|id| affected_executions.contains(id))
                {
                    if let Some(id) = child.data["inputs"]["item"]["id"].as_str() {
                        reset_tasks.insert(id.to_owned());
                    }
                    for e in child.data["steps"].as_array().into_iter().flatten() {
                        if let Some(id) = e["id"].as_str() {
                            affected_executions.insert(id.into());
                        }
                    }
                }
            }
            if before == affected_executions.len() {
                break;
            }
        }
        for id in &reset_tasks {
            if let Some(task) = facts.tasks.get(id)
                && !task_changes.iter().any(|v| v["id"] == *id)
            {
                task_changes.push(json!(task));
            }
        }
        let invalidation_causes = json!({"seeds":seeds,"roles":plan.roles.iter().filter(|(k,v)|old.plan.roles.get(*k).is_some_and(|old|old!=*v)).map(|(k,_)|k).collect::<Vec<_>>(),
            "definitions":definitions(&plan).iter().filter(|(k,v)|old_definitions.get(*k).is_some_and(|old|old!=*v)).map(|(k,_)|k).collect::<Vec<_>>(),
            "permissions":plan.permission_locks.iter().filter(|(k,v)|old_permission(k).is_some_and(|old|json!(old)!=json!(v))).map(|(k,_)|k).collect::<Vec<_>>(),
            "integrations":plan.integration_locks.iter().filter(|(k,v)|old.plan.integration_locks.get(*k).is_some_and(|old|json!(old)!=json!(v))).map(|(k,_)|k).collect::<Vec<_>>()});
        let footprint = json!({"invalidation_causes":invalidation_causes,"reset_tasks":reset_tasks,"executions":old.steps.iter().filter(|s|affected.contains(&s.step) || aggregation_updates.contains(&s.step)).collect::<Vec<_>>(),"tasks":task_changes,
            "children":facts.records.values().filter(|r|r.r#type=="workflow_instance"&&r.data["root_instance_id"]==old.root_instance_id&&r.id!=row.id).map(|r|(&r.id,&r.data)).collect::<Vec<_>>()});
        plan.base_workflow_ref =
            if adopted.is_some() && plan.workflow_revision_id != old.plan.workflow_revision_id {
                Some(plan.workflow_revision_id.clone())
            } else {
                old.plan.base_workflow_ref.clone()
            };
        // Resolve the candidate source and Standard in a disposable state. Preview publishes nothing.
        let mut candidate_state = facts.catalog.state.as_ref().clone();
        if facts.record(&candidate.id).is_err() {
            candidate_state.governance.push(candidate.clone());
        }
        let mut candidate_instance = old.clone();
        candidate_instance.plan = plan;
        candidate_instance.workflow_ref = candidate.id.clone();
        candidate_instance.inputs = inputs.clone();
        candidate_instance.role_agent_bindings = bindings.clone();
        crate::orchestration::freeze_plan_record(
            &mut candidate_state,
            &mut candidate_instance,
            &row.scope,
            &changes.reason,
        )?;
        let plan = candidate_instance.plan;
        let standard_diff =
            if let (Some(before), Some(after)) = (&old.plan.standard_ref, &plan.standard_ref) {
                diff_standard_snapshots(
                    &standard_snapshot(&facts.catalog.state, before, &row.scope)?,
                    &standard_snapshot(&candidate_state, after, &row.scope)?,
                )
            } else {
                Value::Null
            };
        if old.parent_execution_id.is_some() {
            crate::standards::check_invocation_contract(&old.plan.document, &plan.document)?;
        }
        let preview_hash = configuration_hash(&(
            changes,
            &document,
            &bindings,
            &inputs,
            &affected,
            &footprint,
            &plan.standard_ref,
        ));
        Ok(NativePreview {
            plan,
            bindings,
            inputs,
            affected,
            budget_updates,
            aggregation_updates,
            bypassed,
            waivers,
            footprint,
            preview_hash,
            standard_diff,
            restored_executions: vec![],
            reverted_change_id: None,
        })
    }
}
