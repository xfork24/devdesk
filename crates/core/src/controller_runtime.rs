//! The controller coordinator observes one bounded actual work slice at a time.
//! It grants a durable original Session/input intent before Host materialization.
use crate::{
    concurrency::*,
    governance::record,
    platform_facts::{one, restored},
    work_owners::WorkOwnerFacts,
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_resources::version,
    *,
};
impl Engine {
    fn prepare_controller_runtime_intent(
        &self,
        commit: &mut WorkflowCommit<'_>,
        row: &ConfigurationRecord,
        controller: &WorkflowController,
        method: &str,
        (target, expected): (&str, u64),
        payload: Value,
    ) -> Result<()> {
        let command_id = format!(
            "controller:{}:{}:{}:{}",
            row.id, controller.epoch, method, controller.event_cursor
        );
        let prior = one(self.store.as_ref(), "workflow_command_intent", &command_id)?;
        require(prior.value.is_none(), "原经理命令意图已存在")?;
        commit.witness([version(&prior)])?;
        let mut intent = record(
            "workflow_intent",
            row.id.clone(),
            row.scope.clone(),
            method.into(),
            1,
            None,
            json!({"status":"prepared","controller":true,"owner_id":controller.owner_id,"method":method,"command":Command{command_id,target:Some(target.into()),expected_revision:Some(expected),payload},"origin":{"kind":"runtime_coordinator","controller_id":row.id,"epoch":controller.epoch}}),
            false,
        );
        intent.id = commit.object_id(&format!("controller-intent:{method}"))?;
        commit.insert_record(&intent)
    }
    pub fn drive_controllers(&self) -> Result<()> {
        self.drive_native_controller_calls()?;
        let began = std::time::Instant::now();
        for (visited, selected) in self
            .pending_runtime_records("workflow_controller", "active")?
            .into_iter()
            .enumerate()
        {
            if visited > 0 && began.elapsed() >= std::time::Duration::from_millis(25) {
                break;
            }
            match self.observe_native_controller(&selected) {
                Err(error)
                    if matches!(
                        error.code.as_str(),
                        "storage_busy" | "revision_conflict" | "resource_pending"
                    ) =>
                {
                    continue;
                }
                result => result?,
            }
        }
        Ok(())
    }
    fn observe_native_controller(&self, selected: &ResourceSnapshot) -> Result<()> {
        let row = restored(self, selected)?;
        let mut controller: WorkflowController = row.decode()?;
        let c = Command {
            command_id: format!("controller-observe:{}:{}", row.id, selected.revision),
            target: Some(row.id.clone()),
            expected_revision: Some(selected.revision),
            payload: json!({"epoch":controller.epoch}),
        };
        let context = InvocationContext::workflow(
            WorkflowActor::Coordinator {
                source_id: row.id.clone(),
            },
            Some(controller.project_id.clone()),
        );
        let mut commit = WorkflowCommit::new(self, "runtime.controller.observe", &c, &context)?;
        commit.coordinator_origin = Some(
            json!({"kind":"runtime_coordinator","controller_id":row.id,"epoch":controller.epoch}),
        );
        commit.witness([version(selected)])?;
        let (current, instance) = self.controller_instance_current(&controller)?;
        commit.witness(instance.versions().iter().cloned())?;
        if !current {
            controller.epoch += 1;
            controller.status = "paused".into();
            controller.reason = "经理实例已换绑或移除，需要实际人类建立后继会话".into();
            self.prepare_controller_turn_cancellation(&mut commit, &controller)?;
        } else if controller.session_id.is_none() {
            let original = one(
                self.store.as_ref(),
                "controller_latest_intent",
                &json!([row.id, controller.epoch, "session.open"]).to_string(),
            )?;
            commit.witness([version(&original)])?;
            if let Some(raw) = &original.value {
                if raw["data"]["status"] == "completed" {
                    let sid = text(&raw["data"]["result"], "id")?;
                    let association = one(self.store.as_ref(), "controller_session_source", &sid)?;
                    require(
                        association.value.as_ref().is_some_and(|r| {
                            r["data"]["controller_id"] == row.id
                                && r["data"]["epoch"] == controller.epoch
                        }),
                        "已接纳经理会话缺少原关联",
                    )?;
                    commit.witness([version(&association)])?;
                    controller.session_id = Some(sid);
                } else if matches!(raw["data"]["status"].as_str(), Some("failed" | "revoked")) {
                    controller.status = "failed".into();
                    controller.reason = raw["data"]["error"].to_string();
                }
            } else {
                let owner = WorkOwnerFacts::read(self.store.as_ref(), &controller.owner_id)?;
                commit.witness(owner.read_set.versions().iter().cloned())?;
                self.prepare_controller_runtime_intent(&mut commit,&row,&controller,"session.open",(&controller.owner_id,owner.owner.revision),json!({"agent":controller.agent_revision_id,"work_type":"orchestration","controller_id":row.id,"controller_epoch":controller.epoch,"force_new":true,"snapshot_id":controller.snapshot_id,"predecessor":controller.predecessor_session_id}))?;
            }
        } else {
            let sid = controller.session_id.as_ref().unwrap().clone();
            let active = one(self.store.as_ref(), "session_active_turns", &sid)?;
            commit.witness([version(&active)])?;
            let turns = active
                .value
                .as_ref()
                .and_then(Value::as_array)
                .ok_or_else(|| err("storage", "经理活动轮次缺失"))?;
            if let Some(turn) = turns.first() {
                require(turns.len() == 1, "经理存在多个活动轮次")?;
                self.prepare_controller_wakeup_observation(&mut commit, &row.id, turn)?;
                if turn["status"] == "needs_reconciliation" {
                    controller.status = "needs_reconciliation".into();
                    controller.reason = "原经理轮次结果未知，需先停止并核对".into();
                } else if turn["started"].as_u64().is_some_and(|t| {
                    now().saturating_sub(t) > controller.policy.timeout_seconds * 1000
                }) {
                    controller.status = "budget_exhausted".into();
                    controller.reason = "经理轮次超时，已请求停止".into();
                    self.prepare_controller_turn_cancellation(&mut commit, &controller)?;
                }
                self.renew_native_controller_lease(&mut commit, &row.id, &controller)?;
            } else {
                let latest = one(self.store.as_ref(), "session_latest_turn", &sid)?;
                commit.witness([version(&latest)])?;
                if let Some(turn) = &latest.value {
                    self.prepare_controller_wakeup_observation(&mut commit, &row.id, turn)?;
                }
                let intent = one(
                    self.store.as_ref(),
                    "controller_latest_intent",
                    &json!([row.id, controller.epoch, "conversation.send"]).to_string(),
                )?;
                commit.witness([version(&intent)])?;
                if latest.value.as_ref().is_some_and(|v| {
                    matches!(
                        v["status"].as_str(),
                        Some("failed" | "needs_reconciliation")
                    )
                }) {
                    controller.status = if latest
                        .value
                        .as_ref()
                        .is_some_and(|v| v["status"] == "failed")
                    {
                        "failed"
                    } else {
                        "needs_reconciliation"
                    }
                    .into();
                    let failure = one(
                        self.store.as_ref(),
                        "turn_status_reason",
                        latest
                            .value
                            .as_ref()
                            .and_then(|v| v["id"].as_str())
                            .unwrap_or(""),
                    )?;
                    commit.witness([version(&failure)])?;
                    controller.reason = failure
                        .value
                        .as_ref()
                        .and_then(|v| v["content"].as_str())
                        .unwrap_or("原经理轮次失败或结果未知，请核对后建立后继")
                        .to_owned();
                } else if intent
                    .value
                    .as_ref()
                    .is_some_and(|r| r["data"]["status"] == "prepared")
                {
                    return Ok(());
                } else if intent.value.as_ref().is_some_and(|r| {
                    matches!(r["data"]["status"].as_str(), Some("failed" | "revoked"))
                }) {
                    let raw = intent.value.as_ref().unwrap();
                    for id in raw["data"]["command"]["payload"]["controller_input_refs"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        let selected = one(
                            self.store.as_ref(),
                            "governance",
                            id.as_str()
                                .ok_or_else(|| err("storage", "原输入引用无效"))?,
                        )?;
                        let input = restored(self, &selected)?;
                        require(
                            input.r#type == "controller_input" && input.definition_id == row.id,
                            "原经理输入归属不符",
                        )?;
                        if input.data["consumed"] == true {
                            let mut data = input.data.clone();
                            data["consumed"] = json!(false);
                            commit.replace_record(&selected, input, data)?;
                        }
                    }
                    controller.status = "failed".into();
                    controller.reason = raw["data"]["error"].to_string();
                } else {
                    let inputs = one(self.store.as_ref(), "controller_pending_inputs", &row.id)?;
                    commit.witness([version(&inputs)])?;
                    let users = inputs
                        .value
                        .as_ref()
                        .and_then(Value::as_array)
                        .ok_or_else(|| err("storage", "原用户输入页缺失"))?;
                    if controller.workspace_only && users.is_empty() {
                        if commit.has_changes() {
                            let fp = configuration_hash(&(
                                "controller-wakeup-observe/v1",
                                &c,
                                &commit.reads,
                            ));
                            commit.finish(&fp, json!({"id":row.id}))?;
                        }
                        return Ok(());
                    }
                    let bootstrap = controller.bootstrap_epoch != controller.epoch;
                    let (events, event_page) =
                        if let Some(root) = controller.root_instance_id.as_deref() {
                            let (events, page, consumed) = crate::controller_events::native_events(
                                self,
                                root,
                                controller.source_cursor,
                            )?;
                            if !bootstrap {
                                controller.source_cursor = consumed;
                            }
                            (if bootstrap { vec![] } else { events }, Some(page))
                        } else {
                            (vec![], None)
                        };
                    if let Some(page) = &event_page {
                        commit.witness([version(page)])?;
                    }
                    let deadline = controller.wait_deadline.is_some_and(|d| d <= now());
                    if !bootstrap && events.is_empty() && users.is_empty() && !deadline {
                        if let Some(turn) = &latest.value
                            && turn["status"] == "completed"
                            && controller.wait_turn_id.as_deref() != turn["id"].as_str()
                        {
                            controller.run_state = "needs_attention".into();
                            controller.reason =
                                "项目经理结束本轮但未登记等待；请核对经理工作记录".into();
                        }
                    } else {
                        let (root, instances, decisions, hint) =
                            self.native_controller_snapshot(&controller)?;
                        if root.as_ref().is_some_and(|r| {
                            matches!(
                                r.instance.status,
                                StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
                            )
                        }) {
                            controller.status = "closed".into();
                        } else {
                            let progress = events.iter().any(|e| {
                                let source = &e["source"];
                                source["origin_turn_id"].is_null()
                                    && (source["change"]["kind"] == "human.confirmed"
                                        || (source["change"]["kind"] == "decision.changed"
                                            && source["change"]["status"] != "pending")
                                        || source["change"]["has_outputs"] == true
                                        || source["change"]["changes"]
                                            .as_array()
                                            .into_iter()
                                            .flatten()
                                            .any(|c| {
                                                c["execution"]["outputs"]
                                                    .as_object()
                                                    .is_some_and(|v| !v.is_empty())
                                            }))
                            });
                            if progress || !users.is_empty() {
                                controller.wakeup_turns = 0;
                            }
                            if controller.wakeup_turns >= controller.policy.max_turns_per_wakeup {
                                controller.status = "budget_exhausted".into();
                                controller.reason = "编排唤醒预算已耗尽，请恢复或补充指令".into();
                            } else {
                                controller.event_cursor += 1;
                                controller.delivery_id = format!(
                                    "controller:{}:{}:{}",
                                    row.id, controller.epoch, controller.event_cursor
                                );
                                let mut snapshot = json!({"renderer_version":ORCHESTRATION_RENDERER,"delivery_id":controller.delivery_id,"controller_id":row.id,"epoch":controller.epoch,"owner_id":controller.owner_id,"root_instance_id":controller.root_instance_id,"project_id":controller.project_id,"workspace_only":controller.workspace_only,"bootstrap":bootstrap,"events":events,"deadline_reached":deadline,"user_messages":users.iter().map(|r|&r["data"]["input"]).collect::<Vec<_>>()});
                                if let Some(root) = controller.root_instance_id.as_deref() {
                                    snapshot["human_gates"] = self.controller_human_gates(root)?;
                                }
                                if bootstrap {
                                    snapshot["root"]=root.as_ref().map(|r|json!({"id":r.record.id,"revision":r.record.revision,"plan_revision_id":r.instance.active_plan_revision_id,"control_version":r.instance.control_version,"status":r.instance.status,"human_governance":r.instance.human_governance,"user_request":r.instance.user_request})).unwrap_or(Value::Null);
                                    snapshot["policy"] = json!(controller.policy);
                                    snapshot["checkpoint"] = controller.checkpoint.clone();
                                    snapshot["instances"] = json!(instances);
                                    snapshot["project_tools"] = json!(MANAGER_TOOLS);

                                    snapshot["decisions"] = decisions;
                                    snapshot["event_hint"] = hint;
                                    controller.bootstrap_epoch = controller.epoch;
                                    controller.source_cursor =
                                        event_page.as_ref().map_or(0, |p| p.revision);
                                }
                                if let Some(cursor) = events.last().and_then(|e| e["id"].as_u64()) {
                                    controller.source_cursor = cursor;
                                }
                                controller.run_state = "running".into();
                                controller.wait_deadline = None;
                                controller.reason.clear();
                                require(
                                    snapshot.to_string().len() <= 512 * 1024,
                                    "经理唤醒快照超出预算，请先分块处理输入",
                                )?;
                                let session = one(self.store.as_ref(), "session", &sid)?;
                                commit.witness([version(&session)])?;
                                controller.wakeup_turns += 1;
                                let client = format!(
                                    "controller:{}:{}:{}",
                                    row.id, controller.epoch, controller.event_cursor
                                );
                                self.prepare_controller_runtime_intent(&mut commit,&row,&controller,"conversation.send",(&sid,session.revision),json!({"client_message_id":client,"input":snapshot.to_string(),"controller_epoch":controller.epoch,"controller_input_refs":users.iter().map(|r|&r["id"]).collect::<Vec<_>>()}))?;
                                let wakeup = OrchestrationWakeup {
                                    id: client.clone(),
                                    controller_id: row.id.clone(),
                                    event_cursor: controller.event_cursor,
                                    status: "prepared".into(),
                                    turn_id: None,
                                    created_at: now(),
                                };
                                let key = ResourceKey::new("orchestration_wakeup", client);
                                commit.witness([ResourceVersion {
                                    resource: key.clone(),
                                    revision: 0,
                                }])?;
                                commit.change(ResourceChange {
                                    resource: key,
                                    value: json!(wakeup),
                                })?;
                                for raw in users {
                                    let input = ConfigurationRecord::from_storage(raw.clone())?;
                                    require(
                                        input.r#type == "controller_input"
                                            && input.definition_id == row.id,
                                        "用户输入跨经理",
                                    )?;
                                    let selected = ResourceSnapshot {
                                        resource: ResourceKey::new("governance", &input.id),
                                        revision: input.revision,
                                        value: Some(raw.clone()),
                                    };
                                    let mut data = input.data.clone();
                                    data["consumed"] = json!(true);
                                    commit.replace_record(&selected, input, data)?;
                                }
                                self.renew_native_controller_lease(
                                    &mut commit,
                                    &row.id,
                                    &controller,
                                )?;
                            }
                        }
                    }
                }
            }
        }
        if json!(controller) != row.data {
            commit.replace_record(selected, row.clone(), json!(controller))?;
        }
        if commit.has_changes() {
            let fingerprint = configuration_hash(&(
                "controller-observe/v1",
                &row.id,
                selected.revision,
                &commit.reads,
            ));
            let prepared = commit.prepare(&fingerprint, json!({"id":row.id}))?;
            self.store.commit_changes(&prepared)?;
        }
        Ok(())
    }
    fn prepare_controller_wakeup_observation(
        &self,
        commit: &mut WorkflowCommit<'_>,
        controller: &str,
        turn: &Value,
    ) -> Result<()> {
        let Some(client) = turn["client_message_id"].as_str() else {
            return Ok(());
        };
        let wakeup = one(self.store.as_ref(), "orchestration_wakeup", client)?;
        commit.witness([version(&wakeup)])?;
        if let Some(mut value) = wakeup.value.clone() {
            require(
                value["controller_id"] == controller && value["turn_id"] == turn["id"],
                "原经理唤醒与实际轮次不符",
            )?;
            if value["status"] == "dispatched" {
                let status = if turn["consumed"] == true {
                    Some("consumed")
                } else if matches!(turn["status"].as_str(), Some("failed" | "cancelled")) {
                    turn["status"].as_str()
                } else {
                    None
                };
                if let Some(status) = status {
                    value["status"] = json!(status);
                    commit.change(ResourceChange {
                        resource: wakeup.resource,
                        value,
                    })?;
                }
            }
        }
        Ok(())
    }
    fn renew_native_controller_lease(
        &self,
        commit: &mut WorkflowCommit<'_>,
        id: &str,
        controller: &WorkflowController,
    ) -> Result<()> {
        let selected = one(self.store.as_ref(), "orchestration_lease", id)?;
        let actual = selected
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "经理原租约缺失"))?;
        require(actual["epoch"] == controller.epoch, "经理原租约代次变化")?;
        if actual["expires_at"].as_u64().unwrap_or(0)
            < now() + controller.policy.timeout_seconds * 500
        {
            self.prepare_controller_lease(commit, id, controller, controller.status == "active")?;
        } else {
            commit.witness([version(&selected)])?;
        }
        Ok(())
    }
    fn native_controller_snapshot(
        &self,
        controller: &WorkflowController,
    ) -> Result<(Option<WorkflowHeaderFacts>, Vec<Value>, Value, Value)> {
        let Some(id) = &controller.root_instance_id else {
            return Ok((None, vec![], json!([]), Value::Null));
        };
        let root = WorkflowHeaderFacts::read(self, id)?;
        let hint = one(self.store.as_ref(), "workflow_root_event", id)?;
        let decisions = one(self.store.as_ref(), "workflow_root_decisions", id)?;
        let mut ids = self.native_workflow_members(id, "")?;
        if !ids.contains(id) {
            ids.insert(0, id.clone());
            ids.truncate(16);
        }
        let mut instances = vec![];
        for id in ids {
            let instance = if id == root.record.id {
                WorkflowHeaderFacts {
                    record: root.record.clone(),
                    instance: root.instance.clone(),
                    header: root.header.clone(),
                }
            } else {
                WorkflowHeaderFacts::read(self, &id)?
            };
            let mut steps = instance.instance.steps.iter().collect::<Vec<_>>();
            steps.sort_by_key(|e| match e.status {
                StepExecutionStatus::Failed
                | StepExecutionStatus::NeedsReconciliation
                | StepExecutionStatus::WaitingInput
                | StepExecutionStatus::WaitingConfirmation
                | StepExecutionStatus::AwaitingAcceptance => 0,
                StepExecutionStatus::Running => 1,
                StepExecutionStatus::Queued => 2,
                _ => 3,
            });
            let mut shown = vec![];
            for e in steps.into_iter().take(16) {
                let readiness =
                    self.native_step_readiness(&instance, &instance.step(self, &e.id)?)?;
                let step = instance
                    .instance
                    .plan
                    .document
                    .steps
                    .iter()
                    .find(|s| s.node_id == e.node_id)
                    .ok_or_else(|| err("storage", "原步骤合同缺失"))?;
                shown.push(json!({"step":e.step,"id":e.id,"status":e.status,"outputs":e.outputs,"input_digest":e.input_digest,"reason":e.reason.chars().take(160).collect::<String>(),"name":step.name,"session_id":e.session_id,"turn_id":e.turn_id,"confirmation_policy":step.confirmation,"confirmation_status":readiness["confirmation_status"],"confirmation_ref":readiness["confirmation_ref"],"confirmation_waiver_ref":readiness["confirmation_waiver_ref"],"human_satisfied":readiness["human_satisfied"],"actions":readiness["actions"],"step_revision":readiness["step_revision"]}));
            }
            // The attention page is bounded independently from the delegation
            // candidates. Do not force a manager to load an entire frozen plan
            // just to authorize its still-pending automatic nodes.
            let automatic_steps = instance
                .instance
                .steps
                .iter()
                .filter(|e| {
                    matches!(
                        e.status,
                        StepExecutionStatus::Queued
                            | StepExecutionStatus::Running
                            | StepExecutionStatus::AwaitingAcceptance
                    ) && instance.instance.plan.document.steps.iter().any(|s| {
                        s.node_id == e.node_id && s.confirmation == ConfirmationPolicy::Automatic
                    })
                })
                .map(|e| json!({"id":e.id,"step":e.step}))
                .collect::<Vec<_>>();
            instances.push(json!({"id":instance.record.id,"revision":instance.record.revision,"owner_id":instance.instance.owner_id,"standard_ref":instance.instance.plan.standard_ref,"workflow_ref":instance.instance.workflow_ref,"plan":instance.instance.active_plan_revision_id,"status":instance.instance.status,"control_version":instance.instance.control_version,"paused":instance.instance.paused,"steps":shown,"automatic_steps":automatic_steps,"step_total":instance.instance.steps.len(),"step_selection":"attention_then_pending"}));
        }
        Ok((
            Some(root),
            instances,
            decisions.value.unwrap_or(json!([])),
            hint.value.unwrap_or(Value::Null),
        ))
    }
}
