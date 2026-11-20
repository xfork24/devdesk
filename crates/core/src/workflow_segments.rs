//! Finite delegation consumes its own operation budget in the step commit.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    platform_facts::{one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_resources::version,
    *,
};
pub(crate) fn segment_driver_matches(driver: Option<&Value>, grant: &Value) -> bool {
    match driver {
        Some(d) => {
            d["id"] == grant["driver_id"]
                && d["data"]["epoch"] == grant["driver_epoch"]
                && d["data"]["mode"] == grant["driver_mode"]
        }
        None => grant["driver_id"].is_null(),
    }
}
impl Engine {
    pub(crate) fn native_segment_dispatch(
        &self,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Value> {
        if let Some(receipt) = self.store.replay_commit(&c.command_id, fingerprint)? {
            return Ok(receipt.result);
        }
        require(
            matches!(
                context.actor(),
                WorkflowActor::Human | WorkflowActor::Controller { .. }
            ),
            "委托需要实际人类或原经理决定",
        )?;
        let root = WorkflowHeaderFacts::read(
            self,
            c.target
                .as_deref()
                .ok_or_else(|| err("invalid_params", "缺少流程"))?,
        )?;
        root.active()?;
        root.guard_request(c, None)?;
        let mut commit = WorkflowCommit::new(self, "workflow.segment.dispatch", c, context)?;
        commit.authorize(&root)?;
        commit.require_no_change(&root.record.id)?;
        commit.witness([version(&root.header)])?;
        let keys: Vec<Id> = serde_json::from_value(c.payload["steps"].clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        require(
            !keys.is_empty() && keys.len() <= 32,
            "委托需要 1 至 32 个确切节点",
        )?;
        let mut nodes = std::collections::BTreeSet::new();
        let mut needed_operations = 0u64;
        for key in keys {
            let e = root.step(self, &key)?;
            let step = root
                .instance
                .plan
                .document
                .steps
                .iter()
                .find(|s| s.node_id == e.execution.node_id)
                .ok_or_else(|| err("not_found", "委托步骤合同缺失"))?;
            require(
                (step.confirmation == ConfirmationPolicy::Automatic
                    || c.payload["include_human"] == true)
                    && nodes.insert(step.node_id.clone()),
                "委托节点重复或未显式允许人审节点执行",
            )?;
            commit.witness(e.reads.versions().iter().cloned())?;
            if !matches!(
                e.execution.status,
                StepExecutionStatus::Completed | StepExecutionStatus::Skipped
            ) {
                needed_operations += if e.execution.status == StepExecutionStatus::Queued {
                    2
                } else {
                    1
                };
                if c.payload["prepare_human_requests"] == true
                    && step.confirmation == ConfirmationPolicy::Human
                {
                    needed_operations += 1;
                }
            }
        }
        let driver = one(
            self.store.as_ref(),
            "workflow_driver_root",
            &root.instance.root_instance_id,
        )?;
        let actual = driver.value.as_ref();
        let prepare_human = c.payload["prepare_human_requests"] == true;
        let complete_instance = c.payload["complete_instance"] == true;
        require(
            !(prepare_human || complete_instance)
                || needed_operations + u64::from(complete_instance) <= 64,
            "自动准备与收尾超出有界委托操作预算，请先缩小执行批次",
        )?;
        require(
            !prepare_human || c.payload["include_human"] == true,
            "准备审批请求需要显式包含 Human 节点",
        )?;
        require(
            !complete_instance
                || root
                    .instance
                    .steps
                    .iter()
                    .all(|e| nodes.contains(&e.node_id)),
            "自动完成必须委托当前实例的全部节点",
        )?;
        if let WorkflowActor::Controller { controller_id, .. } = context.actor() {
            let selected = one(self.store.as_ref(), "governance", controller_id)?;
            let owner = restored(self, &selected)?;
            let controller: WorkflowController = owner.decode()?;
            for (enabled, operation) in [
                (prepare_human, "request_decision"),
                (complete_instance, "complete"),
            ] {
                if enabled {
                    controller.policy.permits(operation)?;
                    require(
                        !controller.policy.human_operations.contains(operation),
                        "自动委托操作仍需要独立人类授权",
                    )?;
                }
            }
            commit.witness([version(&selected)])?;
        }
        require(
            matches!(context.actor(), WorkflowActor::Human)
                || actual.is_some_and(|d| d["data"]["mode"] == "agent"),
            "当前经理不持有根 Agent 驱动",
        )?;
        commit.witness([version(&driver)])?;
        let id=commit.new_record(record("workflow_segment",root.record.id.clone(),root.record.scope.clone(),text(&c.payload,"reason")?,1,None,json!({"status":"active","plan_revision_id":root.instance.active_plan_revision_id,"node_ids":nodes,"include_human":c.payload["include_human"]==true,"prepare_human_requests":prepare_human,"complete_instance":complete_instance,"actor":context.actor(),"driver_id":actual.map(|d|&d["id"]),"driver_epoch":actual.map(|d|&d["data"]["epoch"]),"driver_mode":actual.map(|d|&d["data"]["mode"]),"max_operations":64,"operations":0}),false),"workflow-segment")?;
        commit.finish(fingerprint, json!({"id":id,"status":"active"}))
    }
    pub fn drive_workflow_segments(&self) -> Result<()> {
        for selected in self.pending_runtime_records("workflow_segment", "stopped")? {
            let segment = restored(self, &selected)?;
            if segment.data["reason"] != "blocked: 原委托决定者已撤权"
                || segment.data["actor"]["kind"] != "controller"
                || segment.data["operations"].as_u64().unwrap_or(u64::MAX)
                    >= segment.data["max_operations"].as_u64().unwrap_or(0)
            {
                continue;
            }
            let root = WorkflowHeaderFacts::read(self, &segment.definition_id)?;
            let driver = one(
                self.store.as_ref(),
                "workflow_driver_root",
                &root.instance.root_instance_id,
            )?;
            let authority = one(
                self.store.as_ref(),
                "controller_authority",
                &text(&segment.data["actor"], "controller_id")?,
            )?;
            if root.instance.paused
                || root.instance.cancel_requested
                || matches!(
                    root.instance.status,
                    StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
                )
                || root.instance.active_plan_revision_id != segment.data["plan_revision_id"]
                || !segment_driver_matches(driver.value.as_ref(), &segment.data)
                || !authority.value.as_ref().is_some_and(|r| {
                    r["data"]["status"] == "active"
                        && r["data"]["epoch"] == segment.data["actor"]["epoch"]
                })
            {
                continue;
            }
            let mut next = segment.clone();
            let mut data = next.data.clone();
            data["status"] = json!("active");
            data["reason"] = json!("");
            update_record(&mut next, data);
            match self.store.commit_changes(&PreparedChangeSet {
                command_id: format!(
                    "segment-authority-restored:{}:{}",
                    segment.id, selected.revision
                ),
                fingerprint: configuration_hash(&next.data),
                read_set: ReadSet::new(vec![
                    version(&selected),
                    version(&root.header),
                    version(&driver),
                    version(&authority),
                ])?,
                changes: vec![ResourceChange {
                    resource: selected.resource,
                    value: self.store.encode_runtime_record(&next)?,
                }],
                result: json!({"id":segment.id}),
                time: now() as i64,
            }) {
                Err(error) if error.retryable_conflict() => continue,
                result => {
                    result?;
                }
            }
        }
        let began = std::time::Instant::now();
        for (visited, selected) in self
            .pending_runtime_records("workflow_segment", "active")?
            .into_iter()
            .enumerate()
        {
            if visited > 0 && began.elapsed() >= std::time::Duration::from_millis(25) {
                break;
            }
            let segment = restored(self, &selected)?;
            let root = WorkflowHeaderFacts::read(self, &segment.definition_id)?;
            let driver = one(
                self.store.as_ref(),
                "workflow_driver_root",
                &root.instance.root_instance_id,
            )?;
            let revoked = root.instance.cancel_requested
                || root.instance.active_plan_revision_id != segment.data["plan_revision_id"]
                || !segment_driver_matches(driver.value.as_ref(), &segment.data);
            let ids = segment.data["node_ids"]
                .as_array()
                .ok_or_else(|| err("storage", "原委托节点缺失"))?;
            require(ids.len() <= 32, "原委托超过预算")?;
            let mut status = if revoked {
                Some(("stopped", "计划或驱动授权变化".to_owned()))
            } else if segment.data["operations"].as_u64().unwrap_or(u64::MAX) >= 64 {
                Some(("stopped", "委托预算已耗尽".into()))
            } else {
                None
            };
            let mut complete = true;
            if status.is_none() && !root.instance.paused {
                for id in ids {
                    let resources = root.step(
                        self,
                        id.as_str()
                            .ok_or_else(|| err("storage", "原委托 ID 无效"))?,
                    )?;
                    let e = &resources.execution;
                    if matches!(
                        e.status,
                        StepExecutionStatus::Completed | StepExecutionStatus::Skipped
                    ) {
                        continue;
                    }
                    complete = false;
                    if matches!(
                        e.status,
                        StepExecutionStatus::Failed
                            | StepExecutionStatus::Cancelled
                            | StepExecutionStatus::NeedsReconciliation
                            | StepExecutionStatus::WaitingInput
                    ) {
                        status = Some(("stopped", e.reason.clone()));
                        break;
                    }
                    let ready = self.native_step_readiness(&root, &resources)?;
                    if matches!(
                        e.status,
                        StepExecutionStatus::AwaitingAcceptance
                            | StepExecutionStatus::WaitingConfirmation
                    ) && ready["completion_satisfied"] == false
                        && ready["completion_overridden"] != true
                    {
                        status = Some((
                            "stopped",
                            ready["actions"]["accept"]["reason"]
                                .as_str()
                                .unwrap_or("步骤完成条件尚未满足")
                                .to_owned(),
                        ));
                        break;
                    }
                    if e.status == StepExecutionStatus::WaitingConfirmation
                        && ready["human_satisfied"] != true
                    {
                        if segment.data["prepare_human_requests"] == true {
                            match self
                                .prepare_segment_human_request(&segment, &root, &resources, &ready)
                            {
                                Ok(true) => break,
                                Ok(false) => {}
                                Err(error) if error.retryable_conflict() => break,
                                Err(error) => {
                                    status = Some(("stopped", error.to_string()));
                                    break;
                                }
                            }
                        }
                        // Dispatch delegation never supplies a Human signature.
                        // Keep the grant durable while unrelated nodes advance;
                        // acceptance still repeats the exact signature checks.
                        continue;
                    }
                    let method = if ready["dispatchable"] == true {
                        Some("workflow.step.dispatch")
                    } else if ready["actions"]["accept"]["allowed"] == true {
                        Some("workflow.step.accept")
                    } else {
                        None
                    };
                    if let Some(method) = method {
                        let c = Command {
                            command_id: format!(
                                "segment:{}:{method}:{}:{}",
                                segment.id, e.id, resources.step.revision
                            ),
                            target: Some(root.record.id.clone()),
                            expected_revision: Some(root.record.revision),
                            payload: json!({"plan_revision_id":root.instance.active_plan_revision_id,"control_version":root.instance.control_version,"expected_step_revision":resources.step.revision,"step":e.step,"execution_id":e.id,"input_digest":e.input_digest,"outputs":e.outputs,"reason":"原有界委托"}),
                        };
                        let context = InvocationContext::workflow(
                            WorkflowActor::Segment {
                                authorization_id: segment.id.clone(),
                            },
                            Some(root.instance.project_id.clone()),
                        );
                        match self.execute_in_context(method, c, &context) {
                            Ok(_) => {}
                            Err(error)
                                if matches!(
                                    error.code.as_str(),
                                    "storage_busy" | "revision_conflict" | "change_pending"
                                ) => {}
                            Err(error) => {
                                status = Some(("stopped", error.to_string()));
                            }
                        }
                        break;
                    }
                }
            } else {
                complete = false;
            }
            if complete {
                if segment.data["complete_instance"] == true
                    && !matches!(
                        root.instance.status,
                        StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
                    )
                {
                    if self.native_completion_ready(&root)? {
                        match self.complete_segment_instance(&segment, &root) {
                            Ok(_) => {}
                            Err(error) if error.retryable_conflict() => {}
                            Err(error) => status = Some(("stopped", error.to_string())),
                        }
                    } else {
                        status =
                            Some(("stopped", "整体完成条件或交付证据未满足，需经理核对".into()));
                    }
                    // Completion changes the header and consumes this grant in
                    // the same guarded command. Observe its terminal state next tick.
                    if status.is_none() {
                        continue;
                    }
                }
                if status.is_none() {
                    status = Some(("completed", String::new()));
                }
            }
            if let Some((status, reason)) = status {
                let mut next = segment.clone();
                let mut data = next.data.clone();
                data["status"] = json!(status);
                data["reason"] = json!(reason);
                update_record(&mut next, data);
                let result = self.store.commit_changes(&PreparedChangeSet {
                    command_id: format!("segment-status:{}:{}", segment.id, selected.revision),
                    fingerprint: configuration_hash(&next.data),
                    read_set: ReadSet::new(vec![
                        version(&selected),
                        version(&root.header),
                        version(&driver),
                    ])?,
                    changes: vec![ResourceChange {
                        resource: selected.resource,
                        value: self.store.encode_runtime_record(&next)?,
                    }],
                    result: json!({"id":segment.id}),
                    time: now() as i64,
                });
                if let Err(error) = result
                    && !matches!(error.code.as_str(), "storage_busy" | "revision_conflict")
                {
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}
