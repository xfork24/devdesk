//! Rust and delegated drivers issue the same public resource-level commands.
//! Native leases and bounded cursors replace aggregate writer transactions.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    platform_facts::{one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_resources::version,
    *,
};
use std::collections::BTreeMap;
impl Engine {
    pub(crate) fn native_driver_select(
        &self,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Value> {
        require(
            matches!(context.actor(), WorkflowActor::Human),
            "切换根驱动需要真实 Human",
        )?;
        if let Some(r) = self.store.replay_commit(&c.command_id, fingerprint)? {
            return Ok(r.result);
        }
        let root = WorkflowHeaderFacts::read(
            self,
            c.target
                .as_deref()
                .ok_or_else(|| err("invalid_params", "缺少原流程"))?,
        )?;
        root.active()?;
        root.guard_request(c, None)?;
        require(
            root.record.id == root.instance.root_instance_id,
            "只能为根实例切换驱动",
        )?;
        let selection: OrchestrationSelection =
            serde_json::from_value(json!({"mode":c.payload["mode"]}))
                .map_err(|e| err("invalid_params", e.to_string()))?;
        selection.validate()?;
        let selected = one(self.store.as_ref(), "workflow_driver_root", &root.record.id)?;
        let previous = selected
            .value
            .as_ref()
            .map(|_| restored(self, &selected))
            .transpose()?;
        let epoch = previous
            .as_ref()
            .and_then(|r| r.data["epoch"].as_u64())
            .unwrap_or(0);
        let expected_epoch = c.payload["expected_driver_epoch"]
            .as_u64()
            .ok_or_else(|| err("precondition_required", "切换驱动需要原驱动代次"))?;
        if expected_epoch != epoch {
            return Err(err("revision_conflict", "原驱动代次已变化"));
        }
        let mut commit = WorkflowCommit::new(self, "workflow.driver.select", c, context)?;
        commit.witness([version(&root.header), version(&selected)])?;
        let catalog = self.store.configuration_snapshot()?;
        commit.witness([ResourceVersion {
            resource: ResourceKey::new("catalog_clock", "configuration"),
            revision: catalog.revision,
        }])?;
        let controller = if selection.mode == "agent" {
            Some(self.prepare_native_root_controller(
                &mut commit,
                &root.record,
                &root.instance,
                &catalog,
            )?)
        } else {
            if let Some(id) = previous
                .as_ref()
                .and_then(|r| r.data["controller_id"].as_str())
            {
                let selected = one(self.store.as_ref(), "governance", id)?;
                let row = restored(self, &selected)?;
                let mut actual: WorkflowController = row.decode()?;
                require(
                    actual.root_instance_id.as_deref() == Some(root.record.id.as_str()),
                    "原经理归属不符",
                )?;
                actual.epoch += 1;
                actual.status = "paused".into();
                actual.reason = "根调度控制已切换".into();
                self.prepare_controller_turn_cancellation(&mut commit, &actual)?;
                commit.replace_record(&selected, row, json!(actual))?;
                self.prepare_controller_lease(&mut commit, id, &actual, false)?;
            }
            None
        };
        let policy = &root.instance.plan.document.scheduling;
        let data = json!({"mode":selection.mode,"policy":policy,"epoch":epoch.checked_add(1).filter(|e|*e<=i64::MAX as u64).ok_or_else(||err("resource_exhausted","驱动代次超出范围"))?,"controller_id":controller,"expires_at":now()+policy.lease_seconds*1000,"reason":"","origin":context.actor()});
        let id = if let Some(previous) = previous {
            let actual = ResourceSnapshot {
                resource: ResourceKey::new("governance", &previous.id),
                revision: selected.revision,
                value: selected.value,
            };
            commit.replace_record(&actual, previous.clone(), data.clone())?;
            previous.id
        } else {
            let mut row = record(
                "workflow_driver",
                root.record.id.clone(),
                root.record.scope.clone(),
                "工作流驱动".into(),
                1,
                None,
                data.clone(),
                false,
            );
            row.id = format!("workflow-driver:{}", root.record.id);
            commit.insert_record(&row)?;
            row.id
        };
        commit.finish(fingerprint, json!({"id":id,"driver":data}))
    }
    pub(crate) fn native_workflow_members(&self, root: &str, cursor: &str) -> Result<Vec<Id>> {
        let key = ResourceKey::new("workflow_members", json!([root, cursor, 16]).to_string());
        let selected = one(self.store.as_ref(), &key.kind, &key.id)?;
        serde_json::from_value(
            selected
                .value
                .ok_or_else(|| err("storage", "流程成员分页缺失"))?,
        )
        .map_err(|e| err("storage", e.to_string()))
    }
    pub fn workflow_driver_query(&self, p: &Value) -> Result<Value> {
        let root = WorkflowHeaderFacts::read(self, &text(p, "id")?)?;
        let driver = one(
            self.store.as_ref(),
            "workflow_driver_root",
            &root.instance.root_instance_id,
        )?;
        let after = p["after"].as_str().unwrap_or("");
        let ids = self.native_workflow_members(&root.instance.root_instance_id, after)?;
        let mut waiting = vec![];
        let mut runtime_approvals = vec![];
        let approvals = self.controller_step_approvals(&root.instance.root_instance_id)?;
        for id in &ids {
            let member = WorkflowHeaderFacts::read(self, id)?;
            let ancestors = self.controller_ancestors(id)?;
            for e in &member.instance.steps {
                if let Some(turn) = &e.turn_id {
                    let pending = one(self.store.as_ref(), "turn_pending_interactions", turn)?;
                    for raw in pending
                        .value
                        .as_ref()
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter(|r| r["kind"] == "tool_approval")
                    {
                        let mut item = raw.clone();
                        item["instance_id"] = json!(id);
                        item["ancestor_instance_ids"] = json!(ancestors);
                        item["owner_id"] = json!(member.instance.owner_id);
                        runtime_approvals.push(item);
                    }
                }
            }
            let offset = p["step_offset"].as_u64().unwrap_or(0) as usize;
            for e in member.instance.steps.iter().skip(offset).take(16) {
                let selected = self.native_step_readiness(&member, &member.step(self, &e.id)?)?;
                if e.status == StepExecutionStatus::AwaitingAcceptance
                    || (selected["actions"]["accept"]["allowed"] != true
                        && selected["dispatchable"] != true
                        && !matches!(
                            e.status,
                            StepExecutionStatus::Completed | StepExecutionStatus::Skipped
                        ))
                {
                    let mut waiting_for = Vec::<&str>::new();
                    if e.status == StepExecutionStatus::WaitingInput
                        || selected["input_gap"].is_string()
                    {
                        waiting_for.push("input");
                    }
                    if matches!(
                        e.status,
                        StepExecutionStatus::AwaitingAcceptance
                            | StepExecutionStatus::WaitingConfirmation
                    ) {
                        if selected["completion_satisfied"] == false
                            && selected["completion_overridden"] != true
                        {
                            waiting_for.push("completion");
                        }
                        if selected["human_satisfied"] == false {
                            waiting_for.push("confirmation");
                        }
                    }
                    if e.status == StepExecutionStatus::NeedsReconciliation {
                        waiting_for.push("reconciliation");
                    }
                    waiting.push(json!({"revision":member.record.revision,"plan_revision_id":member.instance.active_plan_revision_id,"control_version":member.instance.control_version,"step_name":member.instance.plan.document.steps.iter().find(|s|s.node_id==e.node_id).map(|s|&s.name),"ancestor_instance_ids":ancestors,"waiting_for":waiting_for,"instance_id":id,"owner_id":member.instance.owner_id,"execution_id":e.id,"step":e.step,"path":e.path,"status":e.status,"reason":e.reason,"actions":selected["actions"],"step_revision":selected["step_revision"],"input_digest":e.input_digest,"outputs":e.outputs}));
                }
            }
        }
        Ok(
            json!({"waiting":waiting,"approvals":approvals,"runtime_approvals":runtime_approvals,"driver":driver.value.map(|raw|self.store.restore_runtime_record(&raw)).transpose()?,"root_instance_id":root.instance.root_instance_id,"children_inherit":true,"next_after":if ids.len()==16{ids.last()}else{None},"step_limit":16}),
        )
    }
    pub fn drive_rust_workflows(&self) -> Result<()> {
        let began = std::time::Instant::now();
        for (visited, selected) in self
            .pending_runtime_records("workflow_driver", "rust")?
            .into_iter()
            .enumerate()
        {
            if visited > 0 && began.elapsed() >= std::time::Duration::from_millis(25) {
                break;
            }
            match self.drive_native_rust_driver(&selected, began) {
                Err(error)
                    if matches!(
                        error.code.as_str(),
                        "revision_conflict" | "storage_busy" | "blocked" | "change_pending"
                    ) =>
                {
                    continue;
                }
                result => result?,
            }
        }
        Ok(())
    }
    fn drive_native_rust_driver(
        &self,
        selected: &ResourceSnapshot,
        began: std::time::Instant,
    ) -> Result<()> {
        let mut selected = selected.clone();
        let mut row = restored(self, &selected)?;
        let root = WorkflowHeaderFacts::read(self, &row.definition_id)?;
        if matches!(
            root.instance.status,
            StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
        ) {
            return Ok(());
        }
        let policy: WorkflowScheduling = serde_json::from_value(row.data["policy"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        if row.data["expires_at"].as_u64().unwrap_or(0) < now() + policy.lease_seconds * 500 {
            let mut data = row.data.clone();
            if data["expires_at"].as_u64().unwrap_or(0) < now() {
                data["epoch"] = json!(data["epoch"].as_u64().unwrap_or(0) + 1);
            }
            data["expires_at"] = json!(now() + policy.lease_seconds * 1000);
            update_record(&mut row, data);
            self.store.commit_changes(&PreparedChangeSet {
                command_id: format!("driver-renew:{}:{}", row.id, selected.revision),
                fingerprint: configuration_hash(&row.data),
                read_set: ReadSet::new(vec![version(&selected), version(&root.header)])?,
                changes: vec![ResourceChange {
                    resource: selected.resource.clone(),
                    value: self.store.encode_runtime_record(&row)?,
                }],
                result: json!({"id":row.id}),
                time: now() as i64,
            })?;
            selected = one(self.store.as_ref(), "governance", &row.id)?;
        }
        let catalog = self.store.configuration_snapshot()?;
        let control = profile_control_policy(
            &catalog.state,
            root.instance
                .plan
                .harness_permission_ref
                .as_deref()
                .ok_or_else(|| err("permission_unconfigured", "根驱动缺少控制 Profile"))?,
            Some(&policy.agent),
        )?;
        let permits = |op: &str| {
            control.allowed_operations.contains(op) && !control.human_operations.contains(op)
        };
        let key = format!("rust-members:{}", row.id);
        let after = self
            .runtime_callback_cursors
            .lock()
            .map_err(|e| err("storage", e.to_string()))?
            .get(&key)
            .cloned()
            .unwrap_or_default();
        let mut ids = self.native_workflow_members(&root.record.id, &after)?;
        if ids.is_empty() && !after.is_empty() {
            ids = self.native_workflow_members(&root.record.id, "")?;
        }
        let mut actions = 0;
        for (visited, id) in ids.into_iter().enumerate() {
            if actions >= policy.max_actions_per_tick
                || (visited > 0 && began.elapsed() >= std::time::Duration::from_millis(25))
            {
                break;
            }
            self.runtime_callback_cursors
                .lock()
                .map_err(|e| err("storage", e.to_string()))?
                .insert(key.clone(), id.clone());
            let member = WorkflowHeaderFacts::read(self, &id)?;
            if member.instance.cancel_requested
                || member.instance.paused
                || matches!(
                    member.instance.status,
                    StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
                )
            {
                continue;
            }
            let step_key = format!("rust-steps:{}:{id}", row.id);
            let after = self
                .runtime_callback_cursors
                .lock()
                .map_err(|e| err("storage", e.to_string()))?
                .get(&step_key)
                .cloned()
                .unwrap_or_default();
            // Narrow candidates using this committed header before expensive
            // resource readiness checks. These facts only skip impossible
            // actions; the public command still validates the complete basis.
            let definitions = member
                .instance
                .plan
                .document
                .steps
                .iter()
                .map(|step| (&step.node_id, step))
                .collect::<BTreeMap<_, _>>();
            let statuses = member
                .instance
                .steps
                .iter()
                .map(|step| (&step.step, &step.status))
                .collect::<BTreeMap<_, _>>();
            let mut executions = member
                .instance
                .steps
                .iter()
                .filter(|execution| match execution.status {
                    StepExecutionStatus::Queued => {
                        definitions.get(&execution.node_id).is_some_and(|step| {
                            step.dependencies.iter().all(|dependency| {
                                statuses.get(dependency).is_some_and(|status| {
                                    matches!(
                                        status,
                                        StepExecutionStatus::Completed
                                            | StepExecutionStatus::Skipped
                                    )
                                })
                            })
                        })
                    }
                    StepExecutionStatus::AwaitingAcceptance
                    | StepExecutionStatus::WaitingConfirmation => true,
                    StepExecutionStatus::Failed => {
                        execution.failure.as_ref().is_some_and(|failure| {
                            failure.automatic_retry_safe()
                                && ((failure.code == "stale_target"
                                    && member.instance.plan.document.delivery_policy.is_some())
                                    || member
                                        .instance
                                        .plan
                                        .document
                                        .scheduling
                                        .retry_error_codes
                                        .contains(&failure.code))
                        })
                    }
                    _ => false,
                })
                .collect::<Vec<_>>();
            executions.sort_by_key(|e| &e.id);
            let selected_steps = executions
                .iter()
                .filter(|e| e.id > after)
                .chain(executions.iter().filter(|e| e.id <= after))
                .take(16)
                .copied()
                .collect::<Vec<_>>();
            let mut chosen = None;
            for e in selected_steps {
                self.runtime_callback_cursors
                    .lock()
                    .map_err(|e| err("storage", e.to_string()))?
                    .insert(step_key.clone(), e.id.clone());
                let resources = member.step(self, &e.id)?;
                let ready = self.native_step_readiness(&member, &resources)?;
                let method = if permits("retry")
                    && ready["actions"]["retry"]["allowed"] == true
                    && e.failure.as_ref().is_some_and(|f| {
                        f.automatic_retry_safe()
                            && ((f.code == "stale_target"
                                && member.instance.plan.document.delivery_policy.is_some())
                                || (ready["retry_count"].as_u64().unwrap_or(u64::MAX)
                                    < member.instance.plan.document.scheduling.max_retries
                                    && member
                                        .instance
                                        .plan
                                        .document
                                        .scheduling
                                        .retry_error_codes
                                        .contains(&f.code)))
                    }) {
                    Some("workflow.step.retry")
                } else if permits("accept") && ready["actions"]["accept"]["allowed"] == true {
                    Some("workflow.step.accept")
                } else if permits("dispatch") && ready["dispatchable"] == true {
                    Some("workflow.step.dispatch")
                } else {
                    None
                };
                if let Some(method) = method {
                    chosen = Some((method, Some(resources)));
                    break;
                }
                if began.elapsed() >= std::time::Duration::from_millis(25) {
                    break;
                }
            }
            if chosen.is_none() && permits("complete") && self.native_completion_ready(&member)? {
                chosen = Some(("workflow.complete", None));
            }
            let Some((method, execution)) = chosen else {
                continue;
            };
            let e = execution.as_ref().map(|r| &r.execution);
            let c = Command {
                command_id: format!(
                    "rust:{}:{}:{method}:{}:{}",
                    row.id,
                    row.data["epoch"],
                    e.map(|e| e.id.as_str()).unwrap_or(&id),
                    execution
                        .as_ref()
                        .map(|r| r.step.revision)
                        .unwrap_or(member.header.revision)
                ),
                target: Some(id),
                expected_revision: Some(member.record.revision),
                payload: json!({"plan_revision_id":member.instance.active_plan_revision_id,"control_version":member.instance.control_version,"expected_step_revision":execution.as_ref().map(|r|r.step.revision),"step":e.map(|e|&e.step),"execution_id":e.map(|e|&e.id),"input_digest":e.map(|e|&e.input_digest),"outputs":e.map(|e|&e.outputs),"reason":"Rust 驱动选择实际共同就绪集合"}),
            };
            let context = InvocationContext::workflow(
                WorkflowActor::Driver {
                    driver_id: row.id.clone(),
                    epoch: row.data["epoch"].as_u64().unwrap_or(0),
                },
                Some(member.instance.project_id.clone()),
            );
            if let Err(error) = self.execute_in_context(method, c, &context) {
                let current = one(self.store.as_ref(), "governance", &row.id)?;
                if current.revision == selected.revision {
                    let mut data = row.data.clone();
                    data["reason"] = json!({"code":error.code,"message":error.message});
                    let mut reason = row.clone();
                    update_record(&mut reason, data);
                    self.store.commit_changes(&PreparedChangeSet {
                        command_id: format!(
                            "driver-reason:{}:{}:{}",
                            row.id,
                            current.revision,
                            configuration_hash(&reason.data)
                        ),
                        fingerprint: configuration_hash(&reason.data),
                        read_set: ReadSet::new(vec![version(&current)])?,
                        changes: vec![ResourceChange {
                            resource: current.resource,
                            value: self.store.encode_runtime_record(&reason)?,
                        }],
                        result: json!({"id":row.id}),
                        time: now() as i64,
                    })?;
                }
                break;
            }
            actions += 1;
        }
        Ok(())
    }
}
