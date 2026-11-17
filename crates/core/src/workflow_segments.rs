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
                step.confirmation == ConfirmationPolicy::Automatic
                    && nodes.insert(step.node_id.clone()),
                "委托节点重复或要求人类确认",
            )?;
            commit.witness(e.reads.versions().iter().cloned())?;
        }
        let driver = one(
            self.store.as_ref(),
            "workflow_driver_root",
            &root.instance.root_instance_id,
        )?;
        let actual = driver.value.as_ref();
        require(
            matches!(context.actor(), WorkflowActor::Human)
                || actual.is_some_and(|d| d["data"]["mode"] == "agent"),
            "当前经理不持有根 Agent 驱动",
        )?;
        commit.witness([version(&driver)])?;
        let id=commit.new_record(record("workflow_segment",root.record.id.clone(),root.record.scope.clone(),text(&c.payload,"reason")?,1,None,json!({"status":"active","plan_revision_id":root.instance.active_plan_revision_id,"node_ids":nodes,"actor":context.actor(),"driver_id":actual.map(|d|&d["id"]),"driver_epoch":actual.map(|d|&d["data"]["epoch"]),"driver_mode":actual.map(|d|&d["data"]["mode"]),"max_operations":64,"operations":0}),false),"workflow-segment")?;
        commit.finish(fingerprint, json!({"id":id,"status":"active"}))
    }
    pub fn drive_workflow_segments(&self) -> Result<()> {
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
                            | StepExecutionStatus::WaitingConfirmation
                    ) {
                        status = Some(("stopped", e.reason.clone()));
                        break;
                    }
                    let ready = self.native_step_readiness(&root, &resources)?;
                    if e.status == StepExecutionStatus::AwaitingAcceptance
                        && ready["completion_satisfied"] == false
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
                status = Some(("completed", String::new()));
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
