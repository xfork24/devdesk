//! Foreach expands the declared collection into actual child roots in one
//! native grant. Work bindings validate each selected owner and frozen catalog.
use crate::{
    concurrency::*,
    governance::record,
    material_commands::stable_object_id,
    platform_facts::one,
    work_owners::WorkOwnerFacts,
    workflow_commits::WorkflowCommit,
    workflow_resources::{WorkflowResources, version},
    workflow_start::queued_execution,
    *,
};
use std::collections::BTreeSet;
impl WorkflowCommit<'_> {
    pub fn dispatch_native_children(
        &mut self,
        resources: &WorkflowResources,
        step: &StepDefinition,
        execution: &StepExecution,
        facts: &WorkflowFacts,
        existing: usize,
    ) -> Result<()> {
        let StepExecutor::Workflow {
            workflow_ref,
            foreach,
            work_binding,
        } = &step.executor
        else {
            return Err(err("storage", "子流程派发缺少原调用定义"));
        };
        let instance = &resources.instance;
        let child = instance
            .plan
            .definitions
            .get(workflow_ref)
            .ok_or_else(|| err("configuration_required", "子流程不在冻结闭包"))?
            .clone();
        let before = self.engine.store.configuration_snapshot()?;
        let mut catalog = before.state.as_ref().clone();
        let mut source = catalog.resolve_workflow_source(&child)?;
        source.assign_node_ids(&child.id);
        let members = if let Some(collection) = foreach {
            let values = facts
                .value(&collection.source)
                .and_then(Value::as_array)
                .ok_or_else(|| err("invalid_workflow", "集合输入必须为数组"))?;
            require(
                values.len() as u64 >= collection.min_items && values.len() <= WORKFLOW_STEP_LIMIT,
                "集合成员数量不满足冻结配置",
            )?;
            values.iter().cloned().map(Some).collect::<Vec<_>>()
        } else {
            vec![None]
        };
        let frozen_members = members.iter().flatten().cloned().collect::<Vec<_>>();
        let (authority, guards) = crate::workflow_resources::live_workflow_chain(
            self.engine.store.as_ref(),
            &resources.record.id,
            &execution.id,
            resources.execution.status.clone(),
        )?;
        self.witness(guards.versions().iter().cloned())?;
        require(
            authority.instances.len() < WORKFLOW_DEPTH_LIMIT,
            "实际子流程调用深度超过上限",
        )?;
        let group = one(
            self.engine.store.as_ref(),
            "workflow_children_source",
            &execution.id,
        )?;
        require(group.value.is_none(), "原执行已展开子流程")?;
        self.witness([version(&group)])?;
        let mut records = vec![];
        let mut children = vec![];
        let mut count = existing;
        let mut owners = BTreeSet::new();
        for (item, member) in members.into_iter().enumerate() {
            let child_id = stable_object_id(
                self.engine.store.as_ref(),
                &self.command.command_id,
                &format!("child-instance:{item}"),
            )?;
            let mut inputs = execution.inputs.clone();
            if let (Some(collection), Some(member)) = (foreach, member) {
                inputs.insert(collection.item_input.clone(), member);
            }
            let mut owner = instance.owner_id.clone();
            let mut scope = resources.record.scope.clone();
            let mut bindings = instance.role_agent_bindings.clone();
            let mut plan = instance.plan.clone();
            let mut effective = child.clone();
            let mut document = source.clone();
            if let Some(binding) = work_binding {
                let work: WorkReference = serde_json::from_value(
                    inputs
                        .get(&binding.input)
                        .and_then(|v| v.pointer(&binding.pointer))
                        .ok_or_else(|| {
                            err("work_configuration_required", "调用缺少类型化工作引用")
                        })?
                        .clone(),
                )
                .map_err(|e| err("invalid_params", e.to_string()))?;
                let actual = WorkOwnerFacts::read(self.engine.store.as_ref(), work.id())?;
                actual.require_editable()?;
                require(actual.project == instance.project_id, "子工作跨项目")?;
                require(
                    matches!(
                        (&work, actual.owner.resource.kind.as_str()),
                        (WorkReference::Project { .. }, "project")
                            | (WorkReference::Feature { .. }, "feature")
                            | (WorkReference::Task { .. }, "task")
                    ),
                    "工作引用类型与实际对象不符",
                )?;
                if matches!(work, WorkReference::Task { .. }) {
                    require(
                        actual.owner.value.as_ref().is_some_and(|t| {
                            t["feature_id"] == instance.owner_id || t["id"] == instance.owner_id
                        }),
                        "Task 不属于当前父工作",
                    )?;
                }
                require(owners.insert(work.id().to_owned()), "集合重复调用同一工作")?;
                self.witness(actual.read_set.versions().iter().cloned())?;
                let active = one(
                    self.engine.store.as_ref(),
                    "workflow_any_active_owner",
                    work.id(),
                )?;
                require(active.revision == 0, "该工作已有活动流程")?;
                self.witness([version(&active)])?;
                owner = work.id().into();
                scope = actual.scope;
                let inherited = crate::standards::work_configuration(&catalog, &owner).cloned();
                let selection = if let Some(selection) = inherited {
                    selection
                } else {
                    crate::standards::select_work_configuration_for(
                        &mut catalog,
                        &work,
                        &scope,
                        &json!({"standard_ref":instance.plan.standard_ref,"workflow_ref":child.id,"role_agent_bindings":instance.role_agent_bindings,"harness_permission_ref":instance.plan.harness_permission_ref}),
                        None,
                    )?
                };
                effective = crate::governance::reference(
                    &catalog,
                    &text(&selection.data, "workflow_ref")?,
                    "workflow",
                    &scope,
                )?
                .clone();
                standard_entry(
                    &catalog,
                    &text(&selection.data, "standard_ref")?,
                    &effective.id,
                    &scope,
                )?;
                bindings = serde_json::from_value(selection.data["role_agent_bindings"].clone())
                    .map_err(|e| err("invalid_configuration", e.to_string()))?;
                plan = self.engine.compile_configured_workflow(
                    &crate::standards::selection_state(&catalog, &selection.data),
                    &effective,
                    &scope,
                    Some(&bindings),
                )?;
                plan.standard_ref = Some(text(&selection.data, "standard_ref")?);
                plan.work_configuration_ref = Some(selection.id);
                document = plan.document.clone();
                crate::standards::check_invocation_contract(&source, &document)?;
            }
            count = count
                .checked_add(document.steps.len())
                .ok_or_else(|| err("resource_exhausted", "子流程展开计数溢出"))?;
            require(count <= WORKFLOW_STEP_LIMIT, "实际流程树展开超过步骤上限")?;
            for child_step in &mut document.steps {
                if let Some(lock) = plan
                    .permission_locks
                    .get(&format!("{}:{}", effective.id, child_step.key))
                {
                    let (write, read) =
                        profile_pair_workspace_access(&lock.harness.profile, &lock.sandbox.profile);
                    child_step.access.writable_paths = write;
                    child_step.access.read_only_paths = read;
                    child_step.access.tool_refs = lock
                        .harness
                        .profile
                        .policy
                        .tool_refs
                        .clone()
                        .unwrap_or_default();
                }
            }
            for (name, port) in &document.inputs {
                if port.required {
                    require(inputs.contains_key(name), format!("子流程缺少输入 {name}"))?;
                }
                if let Some(value) = inputs.get(name) {
                    self.engine.enforce_delivery_value(
                        &catalog,
                        plan.definitions
                            .get(&port.delivery_ref)
                            .ok_or_else(|| err("configuration_required", "子流程输入类型未冻结"))?,
                        value,
                    )?;
                }
            }
            let steps = document
                .steps
                .iter()
                .map(|s| {
                    let mut e = queued_execution(
                        &child_id,
                        &effective.id,
                        s,
                        stable_object_id(
                            self.engine.store.as_ref(),
                            &self.command.command_id,
                            &format!("child-step:{item}:{}", s.node_id),
                        )?,
                        format!("{}/{item}/{}", execution.path, s.key),
                    );
                    e.round = execution.round;
                    Ok(e)
                })
                .collect::<Result<Vec<_>>>()?;
            plan.workflow_revision_id = effective.id.clone();
            plan.base_workflow_ref = Some(effective.id.clone());
            plan.document = document;
            let mut runtime = ConfiguredWorkflowInstance {
                active_plan_revision_id: String::new(),
                control_version: 1,
                schema_version: WORKFLOW_INSTANCE_SCHEMA,
                owner_id: owner.clone(),
                workflow_ref: effective.id.clone(),
                project_id: instance.project_id.clone(),
                plan,
                role_agent_bindings: bindings,
                inputs,
                user_request: instance.user_request.clone(),
                human_governance: instance.human_governance,
                steps,
                status: StepExecutionStatus::Queued,
                paused: false,
                parent_execution_id: Some(execution.id.clone()),
                root_instance_id: instance.root_instance_id.clone(),
                cancel_requested: false,
            };
            let frozen = crate::orchestration::freeze_plan_record(
                &mut catalog,
                &mut runtime,
                &scope,
                "child",
            )?;
            records.push(frozen);
            let mut invocation = record(
                "workflow_invocation",
                execution.id.clone(),
                resources.record.scope.clone(),
                step.name.clone(),
                1,
                None,
                json!({"parent_execution_id":execution.id,"parent_plan_revision_id":execution.plan_revision_id,"child_instance_id":child_id,"initial_plan_revision_id":runtime.active_plan_revision_id,"owner_id":owner,"member_identity":if work_binding.is_some(){owner}else{item.to_string()},"standard_ref":runtime.plan.standard_ref,"workflow_ref":runtime.workflow_ref,"work_configuration_ref":runtime.plan.work_configuration_ref,"input_digest":configuration_hash(&runtime.inputs)}),
                true,
            );
            invocation.id = stable_object_id(
                self.engine.store.as_ref(),
                &self.command.command_id,
                &format!("workflow-invocation:{item}"),
            )?;
            records.push(invocation);
            self.initialize_rework_budgets(&runtime, &scope)?;
            let mut row = record(
                "workflow_instance",
                runtime.owner_id.clone(),
                scope,
                format!("{} · {}", step.name, item + 1),
                1,
                None,
                json!(runtime),
                false,
            );
            row.id = child_id.clone();
            records.push(row);
            children.push(child_id);
        }
        let candidate =
            ConfigurationCatalogSnapshot::new(catalog, before.revision, before.repository_ready)?;
        let publication =
            prepare_configuration_publication(self.engine.store.as_ref(), &before, &candidate)?;
        self.witness(publication.read_set.versions().iter().cloned())?;
        for change in publication.changes {
            self.change(change)?;
        }
        for row in records {
            let key = ResourceKey::new("governance", &row.id);
            self.witness([ResourceVersion {
                resource: key.clone(),
                revision: 0,
            }])?;
            self.change(ResourceChange {
                resource: key,
                value: self
                    .engine
                    .store
                    .encode_configuration_record(&candidate, &row)?,
            })?;
        }
        self.new_record(record("workflow_children",execution.id.clone(),resources.record.scope.clone(),step.name.clone(),1,None,json!({"children":children,"foreach":foreach.is_some(),"collection":{"value":frozen_members},"max_concurrency":foreach.as_ref().map_or(1,|c|c.max_concurrency)}),true),"workflow-children")?;
        Ok(())
    }
}
