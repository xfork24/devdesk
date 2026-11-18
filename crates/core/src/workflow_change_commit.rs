//! Durable plan activation owns only the selected plan, executions and Tasks.
//! Stop requests and immutable observations precede the final optimistic commit.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    material_commands::stable_object_id,
    platform_facts::{one, restored},
    workflow_change_preview::NativePreview,
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_resources::{WorkflowResources, version},
    workflow_start::queued_execution,
    *,
};
use std::collections::{BTreeMap, BTreeSet};
impl Engine {
    pub(crate) fn native_workflow_change_command(
        &self,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Value> {
        if let Some(receipt) = self.store.replay_commit(&c.command_id, fingerprint)? {
            return Ok(receipt.result);
        }
        let root = WorkflowHeaderFacts::read(
            self,
            c.target
                .as_deref()
                .ok_or_else(|| err("invalid_params", "缺少原流程"))?,
        )?;
        root.active()?;
        root.guard_request(c, None)?;
        let changes: WorkflowChangeSet = serde_json::from_value(c.payload["changeset"].clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        let (preview, facts) = self.prepare_native_change_preview(&root, &changes)?;
        require(
            c.payload["preview_hash"] == preview.preview_hash,
            "预演已过期，请保留草稿并重新预演",
        )?;
        let mut commit = WorkflowCommit::new(self, "workflow.change.apply", c, context)?;
        commit.authorize(&root)?;
        for key in root.instance.plan.document.rework_policies.keys() {
            require(
                preview.plan.document.rework_policies.contains_key(key),
                "计划后继必须保留返工预算身份；调整额度不能删除或重命名策略",
            )?;
        }
        if (root.instance.plan.document.rework_policies != preview.plan.document.rework_policies
            || root.instance.plan.document.review_policies != preview.plan.document.review_policies
            || changes
                .operations
                .iter()
                .any(|op| matches!(op, WorkflowChangeOperation::AdoptReworkBaseline { .. })))
            && !matches!(context.actor(), WorkflowActor::Human)
        {
            commit.require_signed_change(&root)?;
        }
        commit.require_no_change(&root.record.id)?;
        commit.witness(facts.reads.versions().iter().cloned())?;
        if let WorkflowActor::Controller { controller_id, .. } = context.actor() {
            let actual = one(self.store.as_ref(), "governance", controller_id)?;
            let row = restored(self, &actual)?;
            let controller: WorkflowController = row.decode()?;
            for (_, requirement) in &preview.waivers {
                require(
                    controller
                        .policy
                        .waivable_requirements
                        .contains(requirement),
                    "原控制策略不允许此豁免",
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
                    "原控制策略未授权 Task 修订",
                )?;
            }
            commit.witness([version(&actual)])?;
        }
        // Publish only the exact immutable candidate closure with the durable
        // request. Activation remains a separate original-version admission.
        let mut source_state = facts.catalog.state.as_ref().clone();
        for source in preview
            .plan
            .definitions
            .values()
            .filter(|r| r.is_definition())
        {
            if let Ok(old) = source_state.configuration(&source.id) {
                require(old == source, "候选源身份内容冲突")?;
            } else {
                source_state.governance.push(source.clone());
            }
        }
        let candidate = ConfigurationCatalogSnapshot::new(
            source_state,
            facts.catalog.revision,
            facts.catalog.repository_ready,
        )?;
        let publication =
            prepare_configuration_publication(self.store.as_ref(), &facts.catalog, &candidate)?;
        let own_source_inserts = publication
            .changes
            .iter()
            .filter(|c| c.resource.kind == "source")
            .count() as u64;
        let basis = ReadSet::new(
            commit
                .reads
                .versions()
                .iter()
                .filter(|v| {
                    !matches!(
                        v.resource.kind.as_str(),
                        "workflow_header" | "workflow_step" | "workflow_change_pending"
                    )
                })
                .cloned()
                .map(|mut v| {
                    if v.resource.kind == "catalog_clock" {
                        v.revision += own_source_inserts;
                    }
                    v
                })
                .collect(),
        )?;
        commit.witness(publication.read_set.versions().iter().cloned())?;
        for change in publication.changes {
            commit.change(change)?;
        }
        let mut row = record(
            "workflow_change",
            root.record.id.clone(),
            root.record.scope.clone(),
            changes.reason.clone(),
            1,
            None,
            json!({"status":"prepared","changeset":changes,"preview":preview,"actor":context.actor(),"basis":basis,"control_version":root.instance.control_version,"root_instance_id":root.instance.root_instance_id,"origin_command":c}),
            false,
        );
        row.id = commit.object_id("workflow-change")?;
        let id = row.id.clone();
        let key = ResourceKey::new("governance", &id);
        commit.witness([ResourceVersion {
            resource: key.clone(),
            revision: 0,
        }])?;
        commit.change(ResourceChange {
            resource: key,
            value: self.store.encode_configuration_record(&candidate, &row)?,
        })?;
        commit.decision(&root, None, "change.prepare", &changes.reason)?;
        commit.finish(fingerprint, json!({"id":id,"status":"prepared"}))
    }
    pub fn drive_workflow_changes(&self) -> Result<()> {
        let began = std::time::Instant::now();
        let mut visited = 0;
        for status in ["prepared", "waiting_quiescence"] {
            for selected in self.pending_runtime_records("workflow_change", status)? {
                if visited > 0 && began.elapsed() >= std::time::Duration::from_millis(25) {
                    return Ok(());
                }
                visited += 1;
                match self.advance_native_workflow_change(&selected) {
                    Ok(()) => {}
                    Err(error)
                        if matches!(
                            error.code.as_str(),
                            "storage_busy" | "workflow_tool_suspended"
                        ) =>
                    {
                        continue;
                    }
                    Err(error) => {
                        let current =
                            one(self.store.as_ref(), "governance", &selected.resource.id)?;
                        if current.revision != selected.revision {
                            continue;
                        }
                        let mut row = restored(self, &current)?;
                        let mut data = row.data.clone();
                        data["status"] = json!("conflicted");
                        data["error"] = json!(error);
                        update_record(&mut row, data);
                        self.store.commit_changes(&PreparedChangeSet {
                            command_id: format!(
                                "workflow-change-conflict:{}:{}",
                                row.id, current.revision
                            ),
                            fingerprint: configuration_hash(&row.data),
                            read_set: ReadSet::new(vec![version(&current)])?,
                            changes: vec![ResourceChange {
                                resource: current.resource,
                                value: self.store.encode_runtime_record(&row)?,
                            }],
                            result: json!({"id":row.id}),
                            time: now() as i64,
                        })?;
                    }
                }
            }
        }
        Ok(())
    }
    fn native_change_subtree(
        &self,
        root: &WorkflowHeaderFacts,
        preview: &NativePreview,
    ) -> Result<Vec<WorkflowResources>> {
        let mut result = vec![];
        let mut todo = vec![];
        for e in root
            .instance
            .steps
            .iter()
            .filter(|e| preview.affected.contains(&e.step))
        {
            todo.push((root.step(self, &e.id)?, 0));
        }
        let mut seen = BTreeSet::new();
        while let Some((resources, depth)) = todo.pop() {
            require(
                depth <= WORKFLOW_DEPTH_LIMIT
                    && seen.insert(resources.execution.id.clone())
                    && seen.len() <= WORKFLOW_STEP_LIMIT,
                "实际变更子树超过预算或有环",
            )?;
            let group = one(
                self.store.as_ref(),
                "workflow_children_source",
                &resources.execution.id,
            )?;
            if let Some(raw) = &group.value {
                for id in raw["data"]["children"]
                    .as_array()
                    .ok_or_else(|| err("storage", "原子流程集合缺失"))?
                {
                    let child = WorkflowHeaderFacts::read(
                        self,
                        id.as_str()
                            .ok_or_else(|| err("storage", "原子流程 ID 无效"))?,
                    )?;
                    require(
                        child.instance.parent_execution_id.as_deref()
                            == Some(resources.execution.id.as_str()),
                        "原子流程归属不符",
                    )?;
                    for e in &child.instance.steps {
                        todo.push((child.step(self, &e.id)?, depth + 1));
                    }
                }
            }
            result.push(resources);
        }
        Ok(result)
    }
    fn advance_native_workflow_change(&self, selected: &ResourceSnapshot) -> Result<()> {
        let change = restored(self, selected)?;
        require(change.r#type == "workflow_change", "不是运行计划变更")?;
        let root = WorkflowHeaderFacts::read(self, &change.definition_id)?;
        let changes: WorkflowChangeSet = serde_json::from_value(change.data["changeset"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let preview: NativePreview = serde_json::from_value(change.data["preview"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        require(
            !root.instance.cancel_requested
                && root.instance.active_plan_revision_id == changes.plan_revision_id
                && root.instance.control_version == change.data["control_version"],
            "变更的原计划或控制决定已变化",
        )?;
        let c = Command {
            command_id: format!("workflow-change-stage:{}:{}", change.id, selected.revision),
            target: Some(root.record.id.clone()),
            expected_revision: Some(root.record.revision),
            payload: json!({"change_id":change.id}),
        };
        let context = InvocationContext::workflow(
            WorkflowActor::Coordinator {
                source_id: change.id.clone(),
            },
            Some(root.instance.project_id.clone()),
        );
        let mut stop = WorkflowCommit::new(self, "runtime.workflow.change.quiesce", &c, &context)?;
        stop.coordinator_origin =
            Some(json!({"kind":"runtime_coordinator","authorization_id":change.id}));
        stop.witness([version(selected), version(&root.header)])?;
        let subtree = self.native_change_subtree(&root, &preview)?;
        let mut waiting = false;
        for resources in &subtree {
            if stop.execution_active(resources)? {
                waiting = true;
                if !matches!(
                    resources.execution.status,
                    StepExecutionStatus::Completed
                        | StepExecutionStatus::Skipped
                        | StepExecutionStatus::Cancelled
                ) {
                    stop.request_execution_stop(resources, &changes.reason)?;
                }
            }
            if resources
                .execution
                .failure
                .as_ref()
                .is_some_and(|f| f.effect == ExecutionFailureEffect::Unknown)
            {
                waiting = true;
            }
        }
        if waiting {
            if change.data["status"] != "waiting_quiescence" {
                let mut data = change.data.clone();
                data["status"] = json!("waiting_quiescence");
                stop.replace_record(selected, change.clone(), data)?;
            }
            if stop.has_changes() {
                let fingerprint = configuration_hash(&(
                    "workflow-change-quiesce/v1",
                    &change.id,
                    selected.revision,
                    &stop.reads,
                ));
                stop.finish(
                    &fingerprint,
                    json!({"id":change.id,"status":"waiting_quiescence"}),
                )?;
            }
            return Ok(());
        }
        // All stop witnesses are kept, while the original business/source
        // versions remain unchanged throughout the wait.
        let actor: WorkflowActor = serde_json::from_value(change.data["actor"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        require(
            !matches!(
                actor,
                WorkflowActor::Coordinator { .. } | WorkflowActor::Segment { .. }
            ),
            "计划修改缺少实际原决定者",
        )?;
        let context =
            InvocationContext::workflow(actor.clone(), Some(root.instance.project_id.clone()));
        let c = Command {
            command_id: format!("workflow-change-activate:{}", change.id),
            target: Some(root.record.id.clone()),
            expected_revision: Some(root.record.revision),
            payload: json!({"change_id":change.id,"plan_revision_id":root.instance.active_plan_revision_id,"control_version":root.instance.control_version}),
        };
        let mut commit =
            WorkflowCommit::new(self, "runtime.workflow.change.activate", &c, &context)?;
        let basis: ReadSet = serde_json::from_value(change.data["basis"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        commit.witness(basis.versions().iter().cloned())?;
        commit.witness(stop.reads.versions().iter().cloned())?;
        for previous in preview.footprint["executions"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let e = root
                .instance
                .steps
                .iter()
                .find(|e| e.id == previous["id"])
                .ok_or_else(|| err("stale_preview", "原受影响执行已替换"))?;
            let actual = json!(e);
            for key in [
                "inputs",
                "input_digest",
                "outputs",
                "contract_digest",
                "node_id",
            ] {
                require(
                    actual[key] == previous[key],
                    "停止期间原输入、产物或合同变化，需要重新预演",
                )?;
            }
        }
        let before = self.store.configuration_snapshot()?;
        let mut catalog = before.state.as_ref().clone();
        for source in preview
            .plan
            .definitions
            .values()
            .filter(|r| ["workflow", "standard"].contains(&r.r#type.as_str()))
        {
            if catalog.configuration(&source.id).is_err() {
                catalog.governance.push(source.clone());
            }
        }
        self.consume_native_rework_budgets(&mut commit, &root, &preview.affected)?;
        for op in &changes.operations {
            if let WorkflowChangeOperation::AdoptReworkBaseline {
                policy,
                used,
                evidence,
            } = op
            {
                require(
                    preview.affected.is_empty(),
                    "采用历史额度基准须保留原执行；后续返工使用独立预演",
                )?;
                let id = crate::workflow_rework::budget_id(&root.instance, policy);
                let selected = one(self.store.as_ref(), "governance", &id)?;
                require(selected.value.is_none(), "已有业务返工计数不能重新起算")?;
                let mut row = record(
                    "workflow_rework_budget",
                    root.instance.owner_id.clone(),
                    root.record.scope.clone(),
                    policy.clone(),
                    1,
                    None,
                    json!({"used":used,"history_baseline_known":true,"evidence":evidence,"actor":actor,"adopted_from_plan":root.instance.active_plan_revision_id}),
                    false,
                );
                row.id = id.clone();
                commit.insert_record(&row)?;
                commit.new_record(record("workflow_rework_baseline",id.clone(),root.record.scope.clone(),policy.clone(),1,None,json!({"used":used,"evidence":evidence,"actor":actor,"change_id":change.id}),true),&format!("rework-baseline:{id}"))?;
            }
        }
        let mut next = root.instance.as_ref().clone();
        next.plan = preview.plan.clone();
        next.workflow_ref = preview.plan.workflow_revision_id.clone();
        next.inputs = preview.inputs.clone();
        next.role_agent_bindings = preview.bindings.clone();
        next.control_version += 1;
        next.status = StepExecutionStatus::Running;
        let mut steps = vec![];
        for step in &preview.plan.document.steps {
            if let Some(restored) = preview
                .restored_executions
                .iter()
                .find(|e| e.node_id == step.node_id)
            {
                require(
                    matches!(
                        restored.status,
                        StepExecutionStatus::Completed
                            | StepExecutionStatus::Skipped
                            | StepExecutionStatus::Cancelled
                            | StepExecutionStatus::AwaitingAcceptance
                            | StepExecutionStatus::WaitingConfirmation
                    ) || (restored.status == StepExecutionStatus::Failed
                        && restored
                            .failure
                            .as_ref()
                            .is_some_and(|f| f.effect != ExecutionFailureEffect::Unknown)),
                    "回退不能重新激活旧执行器或未知副作用",
                )?;
                require(
                    !commit.archived_execution_active(restored)?,
                    "回退不能恢复活动写者或未知副作用",
                )?;
                steps.push(restored.clone());
                continue;
            }
            if !preview.affected.contains(&step.key)
                && let Some(old) = root
                    .instance
                    .steps
                    .iter()
                    .find(|e| e.node_id == step.node_id)
            {
                steps.push(old.clone());
                continue;
            }
            let path = root
                .instance
                .steps
                .first()
                .and_then(|e| e.path.rsplit_once('/'))
                .map(|(prefix, _)| format!("{prefix}/{}", step.key))
                .unwrap_or_else(|| format!("/{}", step.key));
            let mut e = queued_execution(
                &root.record.id,
                &next.workflow_ref,
                step,
                stable_object_id(
                    self.store.as_ref(),
                    &change.id,
                    &format!("plan-replacement:{}", step.node_id),
                )?,
                path,
            );
            e.round = root
                .instance
                .steps
                .iter()
                .find(|e| e.node_id == step.node_id)
                .map_or(1, |e| e.round + 1);
            if preview.bypassed.contains(&step.key) {
                e.status = StepExecutionStatus::Skipped;
                e.reason = changes.reason.clone();
            }
            steps.push(e);
        }
        next.steps = steps;
        let mut frozen = crate::orchestration::freeze_plan_record(
            &mut catalog,
            &mut next,
            &root.record.scope,
            &changes.reason,
        )?;
        let original_frozen_id = frozen.id.clone();
        frozen.id = stable_object_id(self.store.as_ref(), &change.id, "plan-revision")?;
        next.active_plan_revision_id = frozen.id.clone();
        for e in &mut next.steps {
            if e.plan_revision_id == original_frozen_id {
                e.plan_revision_id = frozen.id.clone();
            }
        }
        frozen.data["plan"] = json!(next.plan);
        frozen.content_hash = content_digest(&frozen.refs, &frozen.data);
        let candidate =
            ConfigurationCatalogSnapshot::new(catalog, before.revision, before.repository_ready)?;
        let publication =
            prepare_configuration_publication(self.store.as_ref(), &before, &candidate)?;
        commit.witness(publication.read_set.versions().iter().cloned())?;
        for change in publication.changes {
            commit.change(change)?;
        }
        let key = ResourceKey::new("governance", &frozen.id);
        commit.witness([ResourceVersion {
            resource: key.clone(),
            revision: 0,
        }])?;
        commit.change(ResourceChange {
            resource: key,
            value: self
                .store
                .encode_configuration_record(&candidate, &frozen)?,
        })?;
        let mut used = BTreeSet::new();
        for original in &root.instance.steps {
            let resources = root.step(self, &original.id)?;
            if let Some(replacement) = next.steps.iter().find(|e| e.node_id == original.node_id) {
                used.insert(replacement.id.clone());
                if replacement.id == original.id {
                    commit.witness(resources.reads.versions().iter().cloned())?;
                } else {
                    commit.new_record(
                        record(
                            "workflow_execution_history",
                            original.id.clone(),
                            root.record.scope.clone(),
                            original.step.clone(),
                            1,
                            None,
                            json!(original),
                            true,
                        ),
                        &format!("workflow-history:{}", original.id),
                    )?;
                    let target = one(
                        self.store.as_ref(),
                        "workflow_step",
                        &json!([root.record.id, replacement.id]).to_string(),
                    )?;
                    require(target.value.is_none(), "恢复或替换的执行已经存在")?;
                    let key = ResourceKey::new(
                        if target.revision > 0 {
                            "workflow_step_restore"
                        } else {
                            "workflow_step_replace"
                        },
                        resources.step.resource.id.clone(),
                    );
                    commit.witness(resources.reads.versions().iter().cloned())?;
                    commit.witness([
                        ResourceVersion {
                            resource: key.clone(),
                            revision: resources.step.revision,
                        },
                        version(&target),
                    ])?;
                    commit.change(ResourceChange {
                        resource: key,
                        value: self
                            .store
                            .encode_configuration_value(&candidate, &json!(replacement))?,
                    })?;
                }
            } else {
                commit.new_record(
                    record(
                        "workflow_execution_history",
                        original.id.clone(),
                        root.record.scope.clone(),
                        original.step.clone(),
                        1,
                        None,
                        json!(original),
                        true,
                    ),
                    &format!("workflow-history:{}", original.id),
                )?;
                let key =
                    ResourceKey::new("workflow_step_delete", resources.step.resource.id.clone());
                commit.witness(resources.reads.versions().iter().cloned())?;
                commit.witness([ResourceVersion {
                    resource: key.clone(),
                    revision: resources.step.revision,
                }])?;
                commit.change(ResourceChange {
                    resource: key,
                    value: Value::Null,
                })?;
            }
        }
        for e in next.steps.iter().filter(|e| !used.contains(&e.id)) {
            let key = ResourceKey::new(
                "workflow_step_create",
                json!([root.record.id, e.id]).to_string(),
            );
            commit.witness([ResourceVersion {
                resource: key.clone(),
                revision: 0,
            }])?;
            commit.change(ResourceChange {
                resource: key,
                value: self.store.encode_runtime_value(&json!(e))?,
            })?;
        }
        for e in next
            .steps
            .iter()
            .filter(|e| e.plan_revision_id != next.active_plan_revision_id)
        {
            commit.new_record(record("workflow_carry_forward",e.id.clone(),root.record.scope.clone(),e.step.clone(),1,None,json!({"from_plan":root.instance.active_plan_revision_id,"to_plan":next.active_plan_revision_id,"source_plan":e.plan_revision_id,"execution_id":e.id,"contract_digest":e.contract_digest,"input_digest":e.input_digest,"outputs":e.outputs,"invocation_timeout_extended":preview.budget_updates.contains(&e.step),"reverted_change_id":preview.reverted_change_id}),true),&format!("carry-forward:{}",e.id))?;
        }
        if let Some(parent) = &root.instance.parent_execution_id {
            commit.new_record(record("workflow_child_override",parent.clone(),root.record.scope.clone(),changes.reason.clone(),1,None,json!({"child_instance_id":root.record.id,"from_plan":root.instance.active_plan_revision_id,"change_id":change.id,"actor":actor}),true),"child-override")?;
        }
        for e in next
            .steps
            .iter()
            .filter(|e| preview.bypassed.contains(&e.step))
        {
            commit.decision(&root, Some(e), "bypass", &changes.reason)?;
        }
        for (node, requirement) in &preview.waivers {
            commit.new_record(record("workflow_waiver",root.record.id.clone(),root.record.scope.clone(),changes.reason.clone(),1,None,json!({"plan_revision_id":next.active_plan_revision_id,"node_id":node,"requirement":requirement,"actor":actor,"change_id":change.id}),true),&format!("waiver:{node}:{requirement}"))?;
        }
        self.prepare_changed_native_tasks(&mut commit, &changes, &preview)?;
        let mut children = BTreeSet::new();
        for resources in &subtree {
            if resources.record.id != root.record.id && children.insert(resources.record.id.clone())
            {
                let child = WorkflowHeaderFacts {
                    record: resources.record.clone(),
                    instance: resources.instance.clone(),
                    header: resources.header.clone(),
                };
                let mut cancelled = child.instance.as_ref().clone();
                cancelled.cancel_requested = true;
                cancelled.paused = false;
                cancelled.status = StepExecutionStatus::Cancelled;
                commit.header(&child, &cancelled)?;
            }
        }
        // Encode the new header against the new canonical source publication.
        let header = ResourceChange {
            resource: root.header.resource.clone(),
            value: {
                let mut data = json!(next);
                data.as_object_mut().unwrap().remove("steps");
                self.store.encode_configuration_value(&candidate, &data)?
            },
        };
        commit.witness([version(&root.header)])?;
        commit.change(header)?;
        if root.record.id == root.instance.root_instance_id {
            let selected = one(self.store.as_ref(), "workflow_driver_root", &root.record.id)?;
            if selected.value.is_some() {
                let driver = restored(self, &selected)?;
                let actual = ResourceSnapshot {
                    resource: ResourceKey::new("governance", &driver.id),
                    revision: selected.revision,
                    value: selected.value,
                };
                let mut data = driver.data.clone();
                data["policy"] = json!(next.plan.document.scheduling);
                commit.replace_record(&actual, driver, data)?;
            }
        }
        if root.record.id == root.instance.root_instance_id {
            let selected = one(
                self.store.as_ref(),
                "controller_owner_source",
                &json!([root.instance.owner_id, false]).to_string(),
            )?;
            commit.witness([version(&selected)])?;
            if selected.value.is_some() {
                let row = restored(self, &selected)?;
                let mut controller: WorkflowController = row.decode()?;
                if controller.root_instance_id.as_deref() == Some(root.record.id.as_str()) {
                    controller.policy = profile_control_policy(
                        &candidate.state,
                        &controller.harness_permission_ref,
                        Some(&next.plan.document.scheduling.agent),
                    )?;
                    controller.policy_digest = configuration_hash(&controller.policy);
                    let actual = ResourceSnapshot {
                        resource: ResourceKey::new("governance", &row.id),
                        revision: selected.revision,
                        value: selected.value,
                    };
                    commit.replace_record(&actual, row, json!(controller))?;
                }
            }
        }
        let mut data = change.data.clone();
        data["status"] = json!("applied");
        data["new_plan_revision_id"] = json!(next.active_plan_revision_id);
        commit.replace_record(selected, change.clone(), data)?;
        commit.decision(&root, None, "change", &changes.reason)?;
        let fingerprint = configuration_hash(&(
            "native-plan-activation/v1",
            &change.id,
            &preview.preview_hash,
        ));
        commit.finish(&fingerprint,json!({"id":change.id,"status":"applied","plan_revision_id":next.active_plan_revision_id}))?;
        Ok(())
    }
    fn prepare_changed_native_tasks(
        &self,
        commit: &mut WorkflowCommit<'_>,
        changes: &WorkflowChangeSet,
        preview: &NativePreview,
    ) -> Result<()> {
        let mut updates = BTreeMap::<Id, (ResourceSnapshot, Task)>::new();
        for raw in preview.footprint["tasks"].as_array().into_iter().flatten() {
            let task: Task =
                serde_json::from_value(raw.clone()).map_err(|e| err("storage", e.to_string()))?;
            let selected = one(self.store.as_ref(), "task", &task.id)?;
            require(
                selected.value.as_ref() == Some(raw) && selected.revision == task.revision,
                "原 Task 在停止期间变化",
            )?;
            updates.insert(task.id.clone(), (selected, task));
        }
        for op in &changes.operations {
            match op {
                WorkflowChangeOperation::ReviseTaskScope {
                    task_id,
                    title,
                    scope,
                } => {
                    let (_, task) = updates
                        .get_mut(task_id)
                        .ok_or_else(|| err("storage", "原 Task 修订依据缺失"))?;
                    task.title = title.clone();
                    task.scope = scope.clone();
                }
                WorkflowChangeOperation::ReviseTaskDependencies {
                    task_id,
                    dependencies,
                } => {
                    let (_, task) = updates
                        .get_mut(task_id)
                        .ok_or_else(|| err("storage", "原 Task 依赖依据缺失"))?;
                    task.dependencies = dependencies.clone();
                }
                _ => {}
            }
        }
        let mut nodes = BTreeMap::new();
        let mut todo = updates.values().map(|(_, t)| t.clone()).collect::<Vec<_>>();
        while let Some(task) = todo.pop() {
            if nodes.contains_key(&task.id) {
                continue;
            }
            require(nodes.len() < WORKFLOW_STEP_LIMIT, "Task 依赖闭包超过预算")?;
            for id in &task.dependencies {
                let parent = if let Some((_, t)) = updates.get(id) {
                    t.clone()
                } else {
                    let selected = one(self.store.as_ref(), "task", id)?;
                    let actual: Task = serde_json::from_value(
                        selected
                            .value
                            .clone()
                            .ok_or_else(|| err("not_found", "原依赖 Task 缺失"))?,
                    )
                    .map_err(|e| err("storage", e.to_string()))?;
                    commit.witness([version(&selected)])?;
                    actual
                };
                require(parent.feature_id == task.feature_id, "Task 依赖跨 Feature")?;
                todo.push(parent);
            }
            nodes.insert(task.id, task.dependencies);
        }
        valid_dag(&nodes.into_iter().collect::<Vec<_>>())?;
        for (_, (selected, mut task)) in updates {
            if preview.footprint["reset_tasks"]
                .as_array()
                .is_some_and(|ids| ids.iter().any(|id| id == &json!(task.id)))
                && task.status == Status::Completed
            {
                task.status = Status::Draft;
            }
            task.revision += 1;
            commit.witness([version(&selected)])?;
            commit.change(ResourceChange {
                resource: selected.resource,
                value: json!(task),
            })?;
        }
        Ok(())
    }
}
