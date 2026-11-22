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
        // Older hosts permanently suspended managers after a finite number of
        // wakeups. Restore that technical suspension without changing the
        // manager's authority, epoch, session or any Human decision.
        for selected in self.pending_runtime_records("workflow_controller", "budget_exhausted")? {
            let row = restored(self, &selected)?;
            let mut controller: WorkflowController = row.decode()?;
            if controller.reason != "编排唤醒预算已耗尽，请恢复或补充指令" {
                continue;
            }
            controller.status = "active".into();
            controller.reason.clear();
            controller.wakeup_turns = 0;
            let c = Command {
                command_id: format!("controller-unlimit:{}:{}", row.id, selected.revision),
                target: Some(row.id.clone()),
                expected_revision: Some(selected.revision),
                payload: json!({"epoch": controller.epoch}),
            };
            let context = InvocationContext::workflow(
                WorkflowActor::Coordinator {
                    source_id: row.id.clone(),
                },
                Some(controller.project_id.clone()),
            );
            let mut commit = WorkflowCommit::new(self, "runtime.controller.unlimit", &c, &context)?;
            commit.replace_record(&selected, row, json!(controller))?;
            match commit.finish(&configuration_hash(&c), json!({"id":c.target})) {
                Err(error) if error.retryable_conflict() => continue,
                result => {
                    result?;
                }
            }
        }
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
                    controller.status = "failed".into();
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
                                Some((&row.id, controller.epoch)),
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
                    let no_wakeup =
                        !bootstrap && events.is_empty() && users.is_empty() && !deadline;
                    let mut recovery_reminder = None;
                    if no_wakeup
                        && controller.run_state != "needs_attention"
                        && let Some(root_id) = controller.root_instance_id.as_deref()
                        && let Some(turn) = &latest.value
                        && turn["status"] == "completed"
                        && controller.wait_turn_id.as_deref() != turn["id"].as_str()
                    {
                        let root = WorkflowHeaderFacts::read(self, root_id)?;
                        let (mut pending, exhausted) =
                            self.workflow_recovery_attention(&mut commit, &root, false)?;
                        self.notify_exhausted_recovery(&mut commit, &row.id, &exhausted)?;
                        pending.sort_unstable();
                        if !pending.is_empty() {
                            let id = format!(
                                "controller-recovery-wakeup:{}",
                                configuration_hash(&(&row.id, &pending))
                            );
                            let prior = one(self.store.as_ref(), "governance", &id)?;
                            commit.witness([version(&prior)])?;
                            if prior.value.is_none() {
                                let mut reminder = record(
                                    "controller_recovery_wakeup",
                                    row.id.clone(),
                                    row.scope.clone(),
                                    "重新派发经理恢复任务".into(),
                                    1,
                                    None,
                                    json!({"source_turn_id":turn["id"],"pending":pending}),
                                    true,
                                );
                                reminder.id = id;
                                commit.insert_record(&reminder)?;
                                recovery_reminder = Some(pending);
                            }
                        }
                    }
                    if no_wakeup && recovery_reminder.is_none() {
                        let run = if let Some(root) = controller.root_instance_id.as_deref() {
                            let run = one(self.store.as_ref(), "workflow_run_authority", root)?;
                            commit.witness([version(&run)])?;
                            run.value
                        } else {
                            None
                        };
                        if run.as_ref().is_some_and(|r| {
                            matches!(r["status"].as_str(), Some("completed" | "cancelled"))
                        }) {
                            controller.status = "closed".into();
                        } else if let Some(turn) = &latest.value
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
                            // A manager remains available for the entire project. Wakeup
                            // counts are historical metadata, never admission authority.
                            controller.event_cursor += 1;
                            controller.delivery_id = format!(
                                "controller:{}:{}:{}",
                                row.id, controller.epoch, controller.event_cursor
                            );
                            let mut snapshot = json!({"renderer_version":ORCHESTRATION_RENDERER,"delivery_id":controller.delivery_id,"controller_id":row.id,"epoch":controller.epoch,"owner_id":controller.owner_id,"root_instance_id":controller.root_instance_id,"project_id":controller.project_id,"workspace_only":controller.workspace_only,"bootstrap":bootstrap,"events":events,"deadline_reached":deadline,"user_messages":users.iter().map(|r|&r["data"]["input"]).collect::<Vec<_>>()});
                            if let Some(pending) = recovery_reminder {
                                snapshot["recovery_reminder"] = json!({"executions":pending,"instruction":"The previous manager turn ended without disposing of these recoverable steps. Read their actual failure records and history, then act within the frozen authority."});
                            }
                            if let Some(root) = controller.root_instance_id.as_deref() {
                                snapshot["human_gates"] = self.controller_human_gates(root)?;
                                snapshot["technical_recovery"] = json!(
                                    "For the deepest affected step, use workflow.inspect and the exact failure receipt. With recovery.status=needs_manager, diagnose it and act in this turn: call recovery.operation using this failed step's execution_id, step, expected_step_revision, plan_revision_id, control_version and expected_revision. For workflow.step.rework omit rework/preview_hash: the kernel chooses the producer based on the exact test report and the frozen graph. Dispatch the returned producer and affected steps normally, then wait for the independent receipt. For recovery.kind=red_coverage, the baseline tests all passed. Delegate a frozen-scope coverage audit to the independent test author. Require evidence of a real missing behavior before adding an assertion. If no genuine Red can be established, preserve the passing evidence and bring the unresolved acceptance question to the human only after bounded recovery. If recovery.fixture_repair is present and the implementation diagnosis is grounded in a reproducible fixture fault, use workflow.step.rework on that failed implementation instead of repeating the same implementation. The kernel selects the independent test author and retains the verified Red baseline. When recovery.continue_operation is available and you can resolve the diagnosed blocker, use workflow.step.input on the exact same execution and provide the verified technical guidance in input. This preserves unfinished working files. When recovery.kind=platform_material, read it first and pass only the exact returned input_ref. Treat history as part of every proposal; a retry reason must name a concrete different action. Technical notices are admitted only after the applicable retry budget is exhausted. An ordinary human-input step still requires actual Human input or an exact platform material reference. Never request workflow.change.apply, a waiver, or approval for this repair. For recovery.status=exhausted, report the failure receipt, attempted repairs and actual retry counts once for that execution and wait for the human. Ask the human for decisions changing frozen business requirements, test expectations, scope, permissions or explicit human gates. Do not retry an unknown side effect without reconciliation."
                                );
                                snapshot["failure_reporting"] = json!(
                                    "执行或核对失败时，读取实际阻塞的最深子步骤，报告最新错误码、诊断、核对结果、实际尝试次数和下一步。确定未写入的技术故障按步骤额度重试。结果未知时先检查原操作。源发布例外：workflow.inspect 返回 source_publication_recovery.retry_allowed=true 时，用原 execution_id 和同一次读取的版本调用 workflow.step.retry，等待结果事件后再读取状态。该重试只允许创建绑定原提交且不存在的开发分支。pending=true 时等待；human_required=true 时停止重试，说明额度、最后一次错误及原分支和提交，让用户从项目经理待办或流程步骤确认返工。其他未知结果仅核对原操作。涉及凭据助手版本、签名、钥匙串权限或 GitHub Token 时按 Host 诊断报告，禁止索取凭据。"
                                );
                                snapshot["review_and_input_recovery"] = json!(
                                    "For review_rework, read the exact rejection and its budget and history. Choose the smallest authorized producer repair, preview the precise scope with workflow.rework.preview, submit the same rework with its preview_hash, then dispatch the returned work and an independent review. If it is an ordinary repair, use the remaining rework budget before proposing an exception. For input_review, a step's claim that it needs a human answer is unverified. Check the execution and its existing sources. Transfer a platform input_ref when available. For a technical correction continue the original execution with workflow.step.input and input beginning with devdesk-repair: followed by the distinct technical action. Genuine human facts and business decisions require workflow.decision.request for the exact waiting execution, with the reason only a human can decide and a reference to the inspected execution. Do not use a generic notification in place of recovery or a formal human decision."
                                );
                            }
                            snapshot["policy"] = json!(controller.policy);
                            // Refresh this metadata on every delivery, including old
                            // sessions whose bootstrap advertised the removed limits.
                            if let Some(policy) = snapshot["policy"].as_object_mut() {
                                policy.remove("max_turns_per_wakeup");
                                policy.remove("max_tool_calls_per_turn");
                            }
                            snapshot["manager_availability"] = json!("continuous");
                            if !bootstrap {
                                let recovery = instances
                                    .iter()
                                    .flat_map(|instance| instance["steps"].as_array().into_iter().flatten()
                                        .filter(|step| !step["recovery"].is_null() || !step["source_publication_recovery"].is_null())
                                        .map(|step| {
                                            let recovery = &step["recovery"];
                                            let history = recovery["history"].as_array().into_iter().flatten()
                                                .filter(|attempt| attempt["started"].is_number()).take(8).map(|attempt| json!({
                                                    "id":attempt["id"],"status":attempt["status"],"phase":attempt["record"]["phase"],
                                                    "action":attempt["record"]["action"].to_string().chars().take(240).collect::<String>()
                                                })).collect::<Vec<_>>();
                                            json!({"instance_id":instance["id"],"execution_id":step["id"],"step":step["step"],
                                                "recovery":recovery["status"],"kind":recovery["kind"],"publication":step["source_publication_recovery"],
                                                "attempts":recovery["budget"]["used"].as_u64().or_else(|| step["retry_count"].as_u64()),
                                                "limit":recovery["budget"]["limit"].as_u64().or_else(|| step["retry_limit"].as_u64()),"history":history})
                                        }))
                                    .take(16)
                                    .collect::<Vec<_>>();
                                if !recovery.is_empty() {
                                    snapshot["recovery_attention"] = json!(recovery);
                                }
                            }
                            if bootstrap {
                                snapshot["root"]=root.as_ref().map(|r|json!({"id":r.record.id,"revision":r.record.revision,"plan_revision_id":r.instance.active_plan_revision_id,"control_version":r.instance.control_version,"status":r.instance.status,"human_governance":r.instance.human_governance,"user_request":r.instance.user_request})).unwrap_or(Value::Null);
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
                            controller.wakeup_turns = controller.wakeup_turns.saturating_add(1);
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
                            self.renew_native_controller_lease(&mut commit, &row.id, &controller)?;
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
                let reason_limit = if e.failure.is_some() { 512 } else { 160 };
                let mut attention = json!({"step":e.step,"id":e.id,"status":e.status,"outputs":e.outputs,"input_digest":e.input_digest,"reason":e.reason.chars().take(reason_limit).collect::<String>(),"name":step.name,"session_id":e.session_id,"turn_id":e.turn_id,"confirmation_policy":step.confirmation,"confirmation_status":readiness["confirmation_status"],"confirmation_ref":readiness["confirmation_ref"],"confirmation_waiver_ref":readiness["confirmation_waiver_ref"],"human_satisfied":readiness["human_satisfied"],"actions":readiness["actions"],"step_revision":readiness["step_revision"]});
                if let Some(failure) = &e.failure {
                    attention["failure_code"] =
                        json!(failure.code.chars().take(128).collect::<String>());
                    attention["failure_effect"] = json!(failure.effect);
                }
                if let Some(recovery) = readiness.get("source_publication_recovery") {
                    attention["source_publication_recovery"] = recovery.clone();
                }
                if let Some(recovery) = readiness.get("recovery").filter(|v| !v.is_null()) {
                    let mut recovery = recovery.clone();
                    if let Some(rows) = recovery["history"].as_array_mut() {
                        rows.truncate(8);
                    }
                    attention["recovery"] = recovery;
                    attention["retry_count"] = readiness["retry_count"].clone();
                    attention["retry_limit"] = readiness["retry_limit"].clone();
                }
                shown.push(attention);
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
            let delegable_steps = instance
                .instance
                .steps
                .iter()
                .filter(|e| {
                    matches!(
                        e.status,
                        StepExecutionStatus::Queued
                            | StepExecutionStatus::Running
                            | StepExecutionStatus::AwaitingAcceptance
                            | StepExecutionStatus::WaitingConfirmation
                    )
                })
                .map(|e| {
                    let policy = instance
                        .instance
                        .plan
                        .document
                        .steps
                        .iter()
                        .find(|s| s.node_id == e.node_id)
                        .map(|s| s.confirmation.clone());
                    json!({"id":e.id,"step":e.step,"confirmation_policy":policy})
                })
                .collect::<Vec<_>>();
            instances.push(json!({"id":instance.record.id,"revision":instance.record.revision,"owner_id":instance.instance.owner_id,"standard_ref":instance.instance.plan.standard_ref,"workflow_ref":instance.instance.workflow_ref,"plan":instance.instance.active_plan_revision_id,"status":instance.instance.status,"control_version":instance.instance.control_version,"paused":instance.instance.paused,"steps":shown,"automatic_steps":automatic_steps,"delegable_steps":delegable_steps,"step_total":instance.instance.steps.len(),"step_selection":"attention_then_pending"}));
        }
        Ok((
            Some(root),
            instances,
            decisions.value.unwrap_or(json!([])),
            hint.value.unwrap_or(Value::Null),
        ))
    }
}
