//! Budget time is the union of real pause/wait intervals. Overlapping parent,
//! child and human waits are counted once; an open interval causes no tick writes.
use crate::{
    concurrency::*,
    platform_facts::{one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_resources::{WorkflowResources, version},
    *,
};
impl Engine {
    pub(crate) fn require_execution_time(&self, resources: &WorkflowResources) -> Result<()> {
        let definition = resources
            .instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.node_id == resources.execution.node_id)
            .ok_or_else(|| err("storage", "原计时步骤合同缺失"))?;
        if let (Some(limits), Some(budget)) = (
            &definition.budget.time_limits,
            &resources.execution.time_budget,
        ) {
            budget.check(limits, &crate::environment::execution_clock())?;
        }
        Ok(())
    }
    pub(crate) fn refresh_execution_time(
        &self,
        resources: &WorkflowResources,
        next: &mut StepExecution,
    ) -> Result<()> {
        let definition = resources
            .instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.node_id == next.node_id)
            .ok_or_else(|| err("storage", "原计时步骤合同缺失"))?;
        let Some(limits) = &definition.budget.time_limits else {
            return Ok(());
        };
        let clock = crate::environment::execution_clock();
        if next.time_budget.is_none() {
            let Some(started) = next.started else {
                return Ok(());
            };
            require(clock.wall_ms >= started, "执行起点晚于实际时钟")?;
            let mut budget = ExecutionTimeBudget::new(&next.id, clock.clone());
            budget.started_at = started;
            next.time_budget = Some(budget);
        }
        let budget = next.time_budget.as_mut().unwrap();
        let closed = matches!(
            next.status,
            StepExecutionStatus::Completed
                | StepExecutionStatus::Cancelled
                | StepExecutionStatus::Skipped
                | StepExecutionStatus::Failed
                | StepExecutionStatus::NeedsReconciliation
        );
        let effective = match budget.clock.effective_now(&clock) {
            Ok(now) => now,
            Err(_) if closed => {
                budget.next_deadline_at = None;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let human = matches!(
            next.status,
            StepExecutionStatus::WaitingInput | StepExecutionStatus::WaitingConfirmation
        );
        let changed = budget.clock.process_epoch != clock.process_epoch
            || resources.execution.status != next.status
            || resources.execution.sequence != next.sequence
            || resources.execution.session_id != next.session_id
            || resources.execution.turn_id != next.turn_id
            || resources.execution.tool_call_id != next.tool_call_id
            || resources.execution.outputs != next.outputs
            || resources.execution.time_budget.as_ref().is_some_and(|old| {
                old.human_wait_closed != budget.human_wait_closed
                    || old.human_wait_started_at != budget.human_wait_started_at
            });
        if human && !budget.human_wait_closed && budget.human_wait_started_at.is_none() {
            budget.human_wait_started_at = Some(effective);
        }
        if !human && !closed {
            budget.human_wait_started_at = None;
            budget.human_wait_closed = false;
        }
        if changed {
            budget.clock = ExecutionClockAnchor {
                wall_ms: effective,
                ..clock
            };
        }
        budget.next_deadline_at = (!closed).then(|| budget.deadline(limits));
        Ok(())
    }
    pub(crate) fn update_native_workflow_budget(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
        next: &mut StepExecution,
    ) -> Result<()> {
        self.update_native_workflow_budget_with_wait(commit, root, resources, next, None)
    }
    pub(crate) fn update_native_workflow_budget_with_wait(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
        next: &mut StepExecution,
        recovery_wait: Option<u64>,
    ) -> Result<()> {
        let Some(started) = next.started else {
            return Ok(());
        };
        let clock = now();
        let mut intervals = Vec::<(u64, u64, bool)>::new();
        if let Some(wait) = recovery_wait {
            intervals.push((wait.max(started), clock, true));
        }
        let mut current = root.record.id.clone();
        let mut seen = std::collections::BTreeSet::new();
        loop {
            require(
                seen.insert(current.clone()) && seen.len() <= WORKFLOW_DEPTH_LIMIT,
                "实际预算祖先存在环或超过边界",
            )?;
            let run = one(self.store.as_ref(), "workflow_run_authority", &current)?;
            let waits = one(
                self.store.as_ref(),
                "workflow_pause_intervals",
                &json!([current, started]).to_string(),
            )?;
            commit.witness([version(&run), version(&waits)])?;
            for row in waits
                .value
                .as_ref()
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let begin = row["data"]["started"]
                    .as_u64()
                    .ok_or_else(|| err("storage", "原暂停区间缺少起点"))?;
                let end = row["data"]["finished"].as_u64();
                intervals.push((
                    begin.max(started),
                    end.unwrap_or(clock).min(clock),
                    end.is_none(),
                ));
            }
            let value = run
                .value
                .as_ref()
                .ok_or_else(|| err("not_found", "原预算祖先缺失"))?;
            let Some(parent) = value["parent_execution_id"].as_str() else {
                break;
            };
            let owner = one(self.store.as_ref(), "workflow_execution_owner", parent)?;
            commit.witness([version(&owner)])?;
            current = text(
                owner
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "原预算父执行缺失"))?,
                "instance_id",
            )?;
        }
        let keys = [
            ResourceKey::new(
                "workflow_tree_waits",
                json!([root.instance.root_instance_id, next.id, started]).to_string(),
            ),
            ResourceKey::new(
                "workflow_capacity",
                json!([root.record.id, root.instance.root_instance_id]).to_string(),
            ),
        ];
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == 2 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "实际等待预算事实不完整",
        )?;
        commit.witness(rows.iter().map(version))?;
        for row in rows[0]
            .value
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let begin = row["started"]
                .as_u64()
                .ok_or_else(|| err("storage", "实际等待区间缺少起点"))?;
            let end = row["finished"].as_u64();
            intervals.push((
                begin.max(started),
                end.unwrap_or(clock).min(clock),
                end.is_none(),
            ));
        }
        let definition = resources
            .instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == next.step)
            .ok_or_else(|| err("storage", "原预算步骤定义缺失"))?;
        if matches!(definition.executor, StepExecutor::Workflow { .. }) {
            let active = commit.execution_active(resources)?;
            let wait = one(self.store.as_ref(), "workflow_active_wait", &next.id)?;
            commit.witness([version(&wait)])?;
            if !active && wait.value.is_none() {
                commit.identity = format!("{}:{}", commit.command.command_id, clock);
                self.start_native_workflow_wait(commit, resources, clock, "child_idle")?;
                intervals.push((clock, clock, true));
            } else if active
                && wait
                    .value
                    .as_ref()
                    .is_some_and(|r| r["data"]["reason"] == "child_idle")
            {
                let mut row = restored(self, &wait)?;
                let mut data = row.data.clone();
                data["finished"] = json!(clock);
                crate::governance::update_record(&mut row, data);
                let key = ResourceKey::new("governance", &row.id);
                commit.witness([ResourceVersion {
                    resource: key.clone(),
                    revision: wait.revision,
                }])?;
                commit.change(ResourceChange {
                    resource: key,
                    value: row.to_storage(),
                })?;
                // The old active interval was included above. Its actual close
                // belongs to this same atomic observation.
                if let Some(begin) = wait
                    .value
                    .as_ref()
                    .and_then(|r| r["data"]["started"].as_u64())
                {
                    for interval in &mut intervals {
                        if interval.0 == begin.max(started) && interval.2 {
                            interval.1 = clock;
                            interval.2 = false;
                        }
                    }
                }
            }
        }
        intervals.retain(|(a, b, _)| a <= b && *a <= clock);
        intervals.sort_by_key(|(a, b, _)| (*a, *b));
        let mut union = Vec::<(u64, u64, bool)>::new();
        for (begin, end, open) in intervals {
            if let Some(last) = union.last_mut()
                && begin <= last.1
            {
                last.1 = last.1.max(end);
                last.2 |= open;
            } else {
                union.push((begin, end, open));
            }
        }
        next.budget_waited_ms = union
            .iter()
            .filter(|(_, _, open)| !*open)
            .map(|(a, b, _)| b.saturating_sub(*a))
            .sum();
        next.budget_wait_started = union.iter().find(|(_, _, open)| *open).map(|(a, _, _)| *a);
        let elapsed = clock
            .saturating_sub(started)
            .saturating_sub(next.budget_waited_ms)
            .saturating_sub(
                next.budget_wait_started
                    .map_or(0, |wait| clock.saturating_sub(wait)),
            );
        if elapsed > definition.budget.timeout_seconds.saturating_mul(1000) {
            commit.request_execution_stop(resources, "超过原步骤执行时限，请求停止原执行者")?;
            next.status = StepExecutionStatus::NeedsReconciliation;
            next.reason = "超过配置执行时限，停止请求与原执行结果等待核对".into();
            next.failure = Some(WorkflowFailure {
                code: "execution_timeout".into(),
                message: next.reason.clone(),
                source_ref: next.id.clone(),
                kind: ExecutionFailureKind::Technical,
                effect: ExecutionFailureEffect::Unknown,
            });
        }
        Ok(())
    }
    pub(crate) fn native_workflow_header_observation(&self, id: &str) -> Result<()> {
        let root = WorkflowHeaderFacts::read(self, id)?;
        if matches!(
            root.instance.status,
            StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
        ) {
            return Ok(());
        }
        let command = Command {
            command_id: format!("workflow-header-observe:{}:{}", id, root.header.revision),
            target: Some(id.into()),
            expected_revision: Some(root.record.revision),
            payload: json!({"header_revision":root.header.revision}),
        };
        let context = InvocationContext::workflow(
            WorkflowActor::Coordinator {
                source_id: id.into(),
            },
            Some(root.instance.project_id.clone()),
        );
        let mut commit = WorkflowCommit::new(self, "runtime.workflow.status", &command, &context)?;
        commit.coordinator_origin =
            Some(json!({"kind":"runtime_coordinator","workflow_instance_id":id}));
        let keys = root
            .instance
            .steps
            .iter()
            .map(|e| ResourceKey::new("workflow_step", json!([id, e.id]).to_string()))
            .collect::<Vec<_>>();
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == keys.len()
                && rows
                    .iter()
                    .zip(&keys)
                    .all(|(r, k)| &r.resource == k && r.value.is_some()),
            "流程当前执行在准备期间已变化",
        )?;
        commit.witness(rows.iter().map(version))?;
        let first = root
            .instance
            .steps
            .first()
            .ok_or_else(|| err("storage", "流程没有当前执行"))?;
        let selected = root.step(self, &first.id)?;
        let cancelled = self.native_workflow_cancelled(&mut commit, &root, &selected)?;
        let states = rows
            .iter()
            .map(|r| r.value.as_ref().unwrap())
            .collect::<Vec<_>>();
        let status = if cancelled
            && states.iter().all(|e| {
                matches!(
                    e["status"].as_str(),
                    Some("completed" | "skipped" | "cancelled")
                )
            }) {
            StepExecutionStatus::Cancelled
        } else if states.iter().any(|e| e["status"] == "needs_reconciliation") {
            StepExecutionStatus::NeedsReconciliation
        } else if states.iter().any(|e| e["status"] == "failed") {
            StepExecutionStatus::Failed
        } else if states.iter().any(|e| !e["started"].is_null()) {
            StepExecutionStatus::Running
        } else {
            StepExecutionStatus::Queued
        };
        if status == root.instance.status && (!cancelled || root.instance.cancel_requested) {
            return Ok(());
        }
        let mut next = root.instance.as_ref().clone();
        next.status = status;
        if cancelled {
            next.cancel_requested = true;
            next.paused = false;
        }
        commit.header(&root, &next)?;
        let fingerprint = configuration_hash(&(
            "workflow-header-observe/v1",
            &command,
            &commit.reads,
            &next.status,
            next.cancel_requested,
        ));
        let prepared = commit.prepare(&fingerprint, json!({"id":id}))?;
        self.store.commit_changes(&prepared)?;
        Ok(())
    }
}
