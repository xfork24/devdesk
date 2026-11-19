//! Bounded projection of actual runtime receipts into their original execution.
//! Business dispatch/acceptance remains an explicit versioned command.
use crate::{
    concurrency::*,
    platform_facts::{one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_outputs::WorkflowOutputProof,
    workflow_resources::{WorkflowResources, version},
    *,
};

fn runtime_command(execution: &StepExecution, method: &str, sequence: u64) -> String {
    format!(
        "workflow:{}",
        configuration_hash(&(execution.id.as_str(), sequence, method))
    )
}
impl Engine {
    fn observed_workflow_intent(
        &self,
        resources: &WorkflowResources,
        method: &str,
        sequence: u64,
    ) -> Result<ResourceSnapshot> {
        let row = one(
            self.store.as_ref(),
            "workflow_command_intent",
            &runtime_command(&resources.execution, method, sequence),
        )?;
        if let Some(raw) = &row.value {
            require(
                raw["type"] == "workflow_intent"
                    && raw["definition_id"] == resources.execution.id
                    && raw["data"]["method"] == method
                    && raw["data"]["execution_authority"]["execution_id"] == resources.execution.id
                    && raw["data"]["execution_authority"]["plan_revision_id"]
                        == resources.execution.plan_revision_id
                    && raw["data"]["execution_authority"]["input_digest"]
                        == resources.execution.input_digest,
                "实际运行回执不属于原步骤意图",
            )?;
        }
        Ok(row)
    }
    fn native_workflow_step_observation(
        &self,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
    ) -> Result<()> {
        let execution = &resources.execution;
        let timed = execution.time_budget.is_some()
            && !matches!(
                execution.status,
                StepExecutionStatus::Completed
                    | StepExecutionStatus::Cancelled
                    | StepExecutionStatus::Skipped
                    | StepExecutionStatus::Failed
                    | StepExecutionStatus::NeedsReconciliation
            );
        if !matches!(
            execution.status,
            StepExecutionStatus::Running | StepExecutionStatus::NeedsReconciliation
        ) && !root.instance.cancel_requested
            && !timed
        {
            return Ok(());
        }
        let command = Command {
            command_id: format!(
                "workflow-observe:{}:{}:{}",
                execution.id, resources.step.revision, root.header.revision
            ),
            target: Some(root.record.id.clone()),
            expected_revision: Some(root.record.revision),
            payload: json!({"execution_id":execution.id,"original_step_revision":resources.step.revision,"original_header_revision":root.header.revision}),
        };
        let context = InvocationContext::workflow(
            WorkflowActor::Coordinator {
                source_id: root.record.id.clone(),
            },
            Some(root.instance.project_id.clone()),
        );
        let mut commit = WorkflowCommit::new(self, "runtime.workflow.observe", &command, &context)?;
        commit.witness(resources.reads.versions().iter().cloned())?;
        commit.coordinator_origin = Some(
            json!({"kind":"runtime_coordinator","workflow_instance_id":root.record.id,"execution_id":execution.id,"step_revision":resources.step.revision}),
        );
        let mut next = execution.clone();
        let mut files = vec![];
        let cancelled = self.native_workflow_cancelled(&mut commit, root, resources)?;
        let stop = one(self.store.as_ref(), "workflow_stop_source", &execution.id)?;
        commit.witness([version(&stop)])?;
        if cancelled || stop.value.is_some() {
            if !matches!(
                execution.status,
                StepExecutionStatus::Completed
                    | StepExecutionStatus::Skipped
                    | StepExecutionStatus::Cancelled
            ) {
                next = commit.stop_execution(resources, "原流程或祖先已请求停止")?;
            }
            if !commit.has_changes() {
                return Ok(());
            }
            let fingerprint =
                configuration_hash(&("workflow-observe-stop/v1", &command, &commit.reads));
            commit.finish(&fingerprint, json!({"id":next.id}))?;
            return Ok(());
        }
        if execution.status == StepExecutionStatus::NeedsReconciliation {
            return Ok(());
        }
        if let Err(error) = self.require_execution_time(resources) {
            commit.request_execution_stop(resources, "执行计时期限已到或时钟待核对")?;
            next.status = StepExecutionStatus::NeedsReconciliation;
            next.reason = error.message.clone();
            next.failure = Some(WorkflowFailure {
                code: error.code,
                message: error.message,
                source_ref: next.id.clone(),
                kind: ExecutionFailureKind::Technical,
                effect: ExecutionFailureEffect::Unknown,
            });
            commit.step(resources, &mut next)?;
            let fingerprint =
                configuration_hash(&("workflow-time-limit/v1", &command, &commit.reads, &next));
            commit.finish(&fingerprint, json!({"id":next.id}))?;
            return Ok(());
        }
        if execution.status != StepExecutionStatus::Running {
            return Ok(());
        }
        let step = resources
            .instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == execution.step)
            .ok_or_else(|| err("storage", "原执行步骤定义缺失"))?;
        let mut failure_source = execution.id.clone();
        let mut observed_effect = None;
        let operation = (|| -> Result<()> {
            match &step.executor {
                StepExecutor::Agent { .. } => {
                    if execution.session_id.is_none() {
                        let intent = self.observed_workflow_intent(resources, "session.open", 1)?;
                        commit.witness([version(&intent)])?;
                        if let Some(raw) = &intent.value {
                            if raw["data"]["status"] == "failed" {
                                self.native_step_failure(
                                    &mut next,
                                    &raw["data"]["error"],
                                    &text(raw, "id")?,
                                    "会话准备失败",
                                    ExecutionFailureKind::Technical,
                                );
                            } else if raw["data"]["status"] == "completed" {
                                let sid = text(&raw["data"]["result"], "id")?;
                                let session = one(self.store.as_ref(), "session", &sid)?;
                                let association =
                                    one(self.store.as_ref(), "configured_session_source", &sid)?;
                                let actual: Session = serde_json::from_value(
                                    self.store.restore_runtime_value(
                                        session
                                            .value
                                            .as_ref()
                                            .ok_or_else(|| err("not_found", "实际正式会话缺失"))?,
                                    )?,
                                )
                                .map_err(|e| err("storage", e.to_string()))?;
                                require(
                                    actual.owner_id == resources.instance.owner_id
                                        && actual.project_id.as_deref()
                                            == Some(resources.instance.project_id.as_str())
                                        && actual.agent
                                            == execution.agent_revision_id.as_deref().unwrap_or("")
                                        && association.value.as_ref().is_some_and(|r| {
                                            r["data"]["execution_id"] == execution.id
                                                && r["data"]["instance_id"] == root.record.id
                                                && r["data"]["plan_revision_id"]
                                                    == execution.plan_revision_id
                                        }),
                                    "正式会话不是原步骤的实际接纳结果",
                                )?;
                                commit.witness([version(&session), version(&association)])?;
                                failure_source = text(raw, "id")?;
                                observed_effect = Some(ExecutionFailureEffect::NotApplied);
                                next.session_id = Some(sid.clone());
                                next.runtime_context = self.native_workflow_runtime_context(
                                    &mut commit,
                                    resources,
                                    &actual,
                                )?;
                                let supplemental =
                                    self.native_retry_supplement(&mut commit, &next)?;
                                let input = render_source_task(
                                    &resources.instance,
                                    step,
                                    &next,
                                    supplemental.as_deref(),
                                )?;
                                commit.runtime_intent(resources,&next,"conversation.send",&sid,Some(session.revision),json!({"client_message_id":format!("{}:{}",next.id,next.sequence),"input":input}))?;
                            }
                        }
                    } else {
                        let input = self.observed_workflow_intent(
                            resources,
                            "conversation.send",
                            execution.sequence,
                        )?;
                        commit.witness([version(&input)])?;
                        if let Some(raw) = &input.value {
                            if raw["data"]["status"] == "failed" {
                                self.native_step_failure(
                                    &mut next,
                                    &raw["data"]["error"],
                                    &text(raw, "id")?,
                                    "轮次准备失败",
                                    ExecutionFailureKind::Technical,
                                );
                            } else if next.turn_id.is_none() && raw["data"]["status"] == "completed"
                            {
                                next.turn_id = Some(text(&raw["data"]["result"], "id")?);
                            }
                        }
                        if next.status == StepExecutionStatus::Running
                            && let Some(tid) = next.turn_id.clone()
                        {
                            let rows = self.store.read_resources(&[
                                ResourceKey::new("workflow_turn_observation", &tid),
                                ResourceKey::new("tool_turn", &tid),
                            ])?;
                            require(rows.len() == 2, "实际轮次观察不完整")?;
                            commit.witness(rows.iter().map(version))?;
                            let observed = rows[0]
                                .value
                                .as_ref()
                                .ok_or_else(|| err("not_found", "实际轮次缺失"))?;
                            let turn = &observed["turn"];
                            require(
                                turn["id"] == tid
                                    && turn["session_id"]
                                        == next.session_id.as_deref().unwrap_or("")
                                    && turn["project_id"] == resources.instance.project_id
                                    && turn["revision"] == rows[1].revision,
                                "实际轮次不属于原步骤会话或同一版本",
                            )?;
                            require(
                                observed["task_source"]["data"]["source"]["execution_id"]
                                    == execution.id
                                    && observed["task_source"]["data"]["source"]["instance_id"]
                                        == root.record.id,
                                "实际输入缺少该步骤的冻结任务来源",
                            )?;
                            match turn["status"].as_str() {
                                Some("completed") => {
                                    if commit.execution_active(resources)? {
                                        next.status = StepExecutionStatus::NeedsReconciliation;
                                        next.reason =
                                            "实际轮次结束，但仍有写者或未核对的工具副作用".into();
                                    } else {
                                        failure_source = tid.clone();
                                        observed_effect = Some(ExecutionFailureEffect::Applied);
                                        let result: Value = serde_json::from_str(
                                            observed["assistant"].as_str().unwrap_or(""),
                                        )
                                        .map_err(|e| {
                                            err(
                                                "output_invalid",
                                                format!("实际 Agent 输出不是声明的 JSON 封套: {e}"),
                                            )
                                        })?;
                                        let runtime =
                                            serde_json::from_value(observed["runtime"].clone())
                                                .map_err(|e| err("storage", e.to_string()))?;
                                        files = self.prepare_native_outputs(
                                            &mut commit,
                                            resources,
                                            &mut next,
                                            &result,
                                            &WorkflowOutputProof::Agent { runtime },
                                            turn["finished"].as_u64().ok_or_else(|| {
                                                err("storage", "实际完成轮次缺少终止时间")
                                            })?,
                                        )?;
                                    }
                                }
                                Some("needs_reconciliation") => {
                                    next.status = StepExecutionStatus::NeedsReconciliation;
                                    next.reason = "原生执行结果需要核对，保留原输入与回执".into();
                                }
                                Some("failed") => self.native_step_failure(
                                    &mut next,
                                    &if observed["failure"].is_object() {
                                        observed["failure"].clone()
                                    } else {
                                        json!(err("agent_failed", "实际 Agent 轮次失败"))
                                    },
                                    &tid,
                                    "实际 Agent 轮次失败",
                                    ExecutionFailureKind::Unknown,
                                ),
                                Some("cancelled") => {
                                    next.status = StepExecutionStatus::Cancelled;
                                    next.reason = "实际 Agent 轮次已经取消".into();
                                    next.finished = turn["finished"].as_u64();
                                }
                                _ => {}
                            }
                        }
                    }
                }
                StepExecutor::Tool { .. } => {
                    let intent =
                        self.observed_workflow_intent(resources, "workflow.tool.execute", 1)?;
                    commit.witness([version(&intent)])?;
                    if let Some(raw) = &intent.value {
                        if raw["data"]["status"] == "failed" {
                            self.native_step_failure(
                                &mut next,
                                &raw["data"]["error"],
                                &text(raw, "id")?,
                                "工具准备失败",
                                ExecutionFailureKind::Technical,
                            );
                        } else if raw["data"]["status"] == "completed" {
                            let id = text(&raw["data"]["result"], "id")?;
                            let selected = one(self.store.as_ref(), "governance", &id)?;
                            let call = restored(self, &selected)?;
                            commit.witness([version(&selected)])?;
                            next.tool_call_id = Some(id.clone());
                            require(
                                matches!(call.r#type.as_str(), "platform_call" | "tool_call")
                                    && call.scope.project_id()
                                        == Some(resources.instance.project_id.as_str()),
                                "工具实际回执身份或范围不符",
                            )?;
                            match call.data["status"].as_str() {
                                Some("succeeded") => {
                                    failure_source = call.id.clone();
                                    observed_effect = Some(ExecutionFailureEffect::Applied);
                                    let receipt = if call.r#type == "platform_call" {
                                        call.data["receipt"].clone()
                                    } else {
                                        let receipt: ToolReceipt =
                                            serde_json::from_value(call.data["receipt"].clone())
                                                .map_err(|e| {
                                                    err("adapter_contract", e.to_string())
                                                })?;
                                        require(
                                            receipt.invocation_id == call.id
                                                && receipt.success
                                                && call.data["invocation"]["operation_id"]
                                                    == execution.id,
                                            "外部工具实际回执不属于原步骤",
                                        )?;
                                        receipt.structured_content.unwrap_or(json!(receipt.content))
                                    };
                                    let outputs = crate::configured_runtime::mapped_tool_outputs(
                                        step, &receipt,
                                    )?;
                                    let envelope = json!({"status":"completed","summary":"实际工具执行已完成","outputs":outputs});
                                    let finished = call.data["finished"]
                                        .as_u64()
                                        .or_else(|| call.data["finished_at"].as_u64())
                                        .unwrap_or(call.created_at);
                                    files = self.prepare_native_outputs(
                                        &mut commit,
                                        resources,
                                        &mut next,
                                        &envelope,
                                        &WorkflowOutputProof::Tool {
                                            call: Box::new(call),
                                            receipt,
                                        },
                                        finished,
                                    )?;
                                }
                                Some("needs_reconciliation") => {
                                    next.status = StepExecutionStatus::NeedsReconciliation;
                                    next.reason = "原工具副作用需要核对".into();
                                }
                                Some("failed") => self.native_step_failure(
                                    &mut next,
                                    &call.data["error"],
                                    &call.id,
                                    "实际工具执行失败",
                                    ExecutionFailureKind::Unknown,
                                ),
                                Some("cancelled") => {
                                    next.status = StepExecutionStatus::Cancelled;
                                    next.reason = "原工具已经取消".into();
                                    next.finished = call.data["finished"].as_u64();
                                }
                                _ => {}
                            }
                        }
                    }
                }
                StepExecutor::Workflow { .. } => {
                    if let Some((outputs, proof, finished)) =
                        self.native_child_outputs(&mut commit, resources, step)?
                    {
                        files=self.prepare_native_outputs(&mut commit,resources,&mut next,&json!({"status":"completed","summary":"本次实际子流程已经完成","outputs":outputs}),&proof,finished)?;
                    }
                }
                StepExecutor::Control => {}
            }
            Ok(())
        })();
        if let Err(mut error) = operation {
            if matches!(
                error.code.as_str(),
                "revision_conflict" | "storage_busy" | "workflow_tool_suspended"
            ) {
                return Err(error);
            }
            commit.clear_changes();
            files.clear();
            next = execution.clone();
            if error.execution_failure.is_none() {
                error.execution_failure = Some(ExecutionFailureDisposition {
                    kind: ExecutionFailureKind::Business,
                    effect: observed_effect.unwrap_or(ExecutionFailureEffect::Unknown),
                });
            }
            self.native_step_failure(
                &mut next,
                &json!(error),
                &failure_source,
                "实际执行结果未通过原输出合同",
                ExecutionFailureKind::Business,
            );
        }
        if next.status == StepExecutionStatus::Running {
            self.update_native_workflow_budget(&mut commit, root, resources, &mut next)?;
        }
        if matches!(
            next.status,
            StepExecutionStatus::Failed | StepExecutionStatus::NeedsReconciliation
        ) && next.failure.is_none()
        {
            next.failure = Some(WorkflowFailure {
                code: if next.status == StepExecutionStatus::NeedsReconciliation {
                    "result_unknown"
                } else {
                    "execution_failed"
                }
                .into(),
                message: next.reason.clone(),
                source_ref: next
                    .tool_call_id
                    .clone()
                    .or_else(|| next.turn_id.clone())
                    .unwrap_or_else(|| next.id.clone()),
                kind: ExecutionFailureKind::Unknown,
                effect: ExecutionFailureEffect::Unknown,
            });
        }
        if json!(next) == json!(execution) && !commit.has_changes() {
            return Ok(());
        }
        commit.step(resources, &mut next)?;
        // Target metadata is part of this derived observation's identity. No
        // business command, supplier request or original callback is rebased.
        let fingerprint =
            configuration_hash(&("workflow-step-observe/v1", &command, &commit.reads, &next));
        let prepared = commit.prepare(&fingerprint, json!({"id":next.id}))?;
        if files.is_empty() {
            self.store.commit_changes(&prepared)?;
        } else {
            let borrowed = files
                .iter()
                .map(|(seal, body)| (seal.clone(), body.as_str()))
                .collect::<Vec<_>>();
            self.seal_command_batch_changes(prepared, &borrowed, resources.record.scope.clone())?;
        }
        Ok(())
    }
    fn native_step_failure(
        &self,
        next: &mut StepExecution,
        value: &Value,
        source: &str,
        fallback: &str,
        kind: ExecutionFailureKind,
    ) {
        let error = serde_json::from_value::<Error>(value.clone()).unwrap_or_else(|_| {
            err(
                "execution_failed",
                crate::workflow_failure::failure_message(value, fallback),
            )
        });
        let failure = WorkflowFailure::from_error(&error, source.into(), kind);
        next.status = if failure.effect == ExecutionFailureEffect::Unknown {
            StepExecutionStatus::NeedsReconciliation
        } else {
            StepExecutionStatus::Failed
        };
        next.reason = error.message;
        next.failure = Some(failure);
        next.finished = Some(now());
    }
    pub(crate) fn evaluate_native_workflows(&self) -> Result<()> {
        let _span = self.store.observe_query("runtime.evaluate_workflows");
        self.drive_native_workflow_reconciliations()?;
        let began = std::time::Instant::now();
        let mut visited = 0;
        let status_key = "workflow-observation-status";
        let start = {
            let mut cursors = self
                .runtime_callback_cursors
                .lock()
                .map_err(|e| err("storage", e.to_string()))?;
            let next = cursors
                .get(status_key)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0)
                % 4;
            cursors.insert(status_key.into(), ((next + 1) % 4).to_string());
            next
        };
        let statuses = ["queued", "running", "failed", "needs_reconciliation"];
        for status in statuses
            .into_iter()
            .cycle()
            .skip(start)
            .take(statuses.len())
        {
            for selected in self.pending_runtime_records("workflow_instance", status)? {
                if visited > 0 && began.elapsed() >= std::time::Duration::from_millis(25) {
                    return Ok(());
                }
                visited += 1;
                let result = (|| -> Result<()> {
                    let root = WorkflowHeaderFacts::read(self, &selected.resource.id)?;
                    let key = format!("workflow-step-observe:{}", root.record.id);
                    let after = self
                        .runtime_callback_cursors
                        .lock()
                        .map_err(|e| err("storage", e.to_string()))?
                        .get(&key)
                        .cloned()
                        .unwrap_or_default();
                    let mut candidates = root
                        .instance
                        .steps
                        .iter()
                        .filter(|e| {
                            matches!(
                                e.status,
                                StepExecutionStatus::Running
                                    | StepExecutionStatus::NeedsReconciliation
                            ) || (e
                                .time_budget
                                .as_ref()
                                .is_some_and(|b| b.next_deadline_at.is_some())
                                && !matches!(
                                    e.status,
                                    StepExecutionStatus::Completed
                                        | StepExecutionStatus::Cancelled
                                        | StepExecutionStatus::Skipped
                                        | StepExecutionStatus::Failed
                                        | StepExecutionStatus::NeedsReconciliation
                                ))
                                || (root.instance.cancel_requested
                                    && !matches!(
                                        e.status,
                                        StepExecutionStatus::Completed
                                            | StepExecutionStatus::Cancelled
                                            | StepExecutionStatus::Skipped
                                    ))
                        })
                        .map(|e| e.id.clone())
                        .collect::<Vec<_>>();
                    candidates.sort();
                    let ordered = candidates
                        .iter()
                        .filter(|id| *id > &after)
                        .chain(candidates.iter().filter(|id| *id <= &after))
                        .take(16)
                        .cloned()
                        .collect::<Vec<_>>();
                    for (index, id) in ordered.iter().enumerate() {
                        if index > 0 && began.elapsed() >= std::time::Duration::from_millis(25) {
                            break;
                        }
                        self.runtime_callback_cursors
                            .lock()
                            .map_err(|e| err("storage", e.to_string()))?
                            .insert(key.clone(), id.clone());
                        let resources = root.step(self, id)?;
                        self.native_workflow_step_observation(&root, &resources)?;
                    }
                    // Observations above may have changed these very steps.
                    // Do not feed our own pre-commit view into completion hints.
                    let current = WorkflowHeaderFacts::read(self, &root.record.id)?;
                    self.publish_workflow_rule_hints(&current)?;
                    self.native_workflow_header_observation(&current.record.id)
                })();
                if let Err(error) = result
                    && !matches!(
                        error.code.as_str(),
                        "revision_conflict"
                            | "storage_busy"
                            | "workflow_tool_suspended"
                            | "material_seal_unresolved"
                    )
                {
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}
