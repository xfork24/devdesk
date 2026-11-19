//! An explicit reconciliation is a new, versioned authorization. Its immutable
//! observations never rewrite the old callback's versions or resend its effect.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    platform_facts::{PlatformAction, PlatformPlan, one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_outputs::WorkflowOutputProof,
    workflow_resources::{WorkflowResources, version},
    *,
};

#[derive(Serialize, Deserialize)]
enum ReconciliationFact {
    Pending,
    Platform {
        receipt: Value,
    },
    PlatformNotApplied {
        source: Id,
        error: Error,
    },
    External {
        receipt: ToolReceipt,
    },
    Agent {
        observation: Value,
    },
    AgentStartNotApplied {
        source: Id,
        error: Error,
    },
    WorkspaceContinuation {
        failed_turn_id: Id,
        previous_turn_id: Id,
    },
    NotApplied {
        source: Id,
    },
    HumanEffect {
        resolution: String,
        evidence: String,
    },
}
impl Engine {
    pub(crate) fn drive_native_workflow_reconciliations(&self) -> Result<()> {
        let began = std::time::Instant::now();
        let mut count = 0;
        for status in ["prepared", "inspecting"] {
            for selected in self.pending_runtime_records("workflow_reconciliation", status)? {
                if count > 0 && began.elapsed() >= std::time::Duration::from_millis(25) {
                    return Ok(());
                }
                count += 1;
                match self.observe_native_workflow_reconciliation(&selected) {
                    Err(error)
                        if matches!(
                            error.code.as_str(),
                            "storage_busy" | "revision_conflict" | "workflow_tool_suspended"
                        ) =>
                    {
                        continue;
                    }
                    result => result?,
                }
            }
        }
        Ok(())
    }
    fn observe_native_workflow_reconciliation(&self, selected: &ResourceSnapshot) -> Result<()> {
        let mut request = restored(self, selected)?;
        let instance = text(&request.data, "instance_id")?;
        let execution = text(&request.data, "execution_id")?;
        let root = WorkflowHeaderFacts::read(self, &instance)?;
        let resources = root.step(self, &execution)?;
        require(
            request.r#type == "workflow_reconciliation"
                && request.definition_id == execution
                && resources.execution.runtime_context["reconciliation_request_id"] == request.id
                && resources.execution.plan_revision_id == request.data["plan_revision_id"]
                && resources.execution.input_digest == request.data["input_digest"]
                && resources.execution.contract_digest == request.data["contract_digest"],
            "核对不能替换原步骤、计划或任务",
        )?;
        let expected = request.data["original_step_revision"]
            .as_u64()
            .ok_or_else(|| err("storage", "原核对缺少步骤版本"))?
            + 1;
        if resources.step.revision != expected {
            return self.finish_unresolved_reconciliation(
                selected,
                &request,
                None,
                &err("revision_conflict", "核对请求对应的原步骤已变化"),
            );
        }
        let key = ResourceKey::new(
            "governance",
            format!("workflow-reconciliation-attempt:{}", request.id),
        );
        let existing = one(self.store.as_ref(), "governance", &key.id)?;
        let mut guard = version(selected);
        let attempt = if existing.value.is_some() {
            restored(self, &existing)?
        } else {
            let header: ResourceVersion =
                serde_json::from_value(request.data["original_header_version"].clone())
                    .map_err(|e| err("storage", e.to_string()))?;
            let mut basis = ReadSet::new(vec![header, version(&resources.step)])?;
            let session = if let Some(sid) = &resources.execution.session_id {
                let selected = one(self.store.as_ref(), "session", sid)?;
                basis = basis.merge([version(&selected)])?;
                Some(sid.clone())
            } else {
                None
            };
            let call = if let Some(id) = &resources.execution.tool_call_id {
                let selected = one(self.store.as_ref(), "governance", id)?;
                basis = basis.merge([version(&selected)])?;
                Some(id.clone())
            } else {
                None
            };
            if let Some(tid) = &resources.execution.turn_id {
                let turn = one(self.store.as_ref(), "tool_turn", tid)?;
                basis = basis.merge([version(&turn)])?;
            }
            let mut row = record(
                "workflow_reconciliation_attempt",
                request.id.clone(),
                request.scope.clone(),
                "原执行核对的固定依据".into(),
                1,
                None,
                json!({"basis":basis,"instance_id":instance,"execution_id":execution,"session_id":session,"call_id":call,"turn_id":resources.execution.turn_id,"resolution":request.data["resolution"],"evidence":request.data["evidence"],"actor":request.data["actor"],"issuer":"kernel_reconciliation"}),
                true,
            );
            row.id = key.id.clone();
            let mut data = request.data.clone();
            data["status"] = json!("inspecting");
            data["attempt_id"] = json!(row.id);
            update_record(&mut request, data);
            let outcome = self.store.submit_changes(&PreparedChangeSet {
                command_id: key.id.clone(),
                fingerprint: configuration_hash(&row.data),
                read_set: basis.merge([guard.clone(), version(&existing)])?,
                changes: vec![
                    ResourceChange {
                        resource: key.clone(),
                        value: self.store.encode_runtime_record(&row)?,
                    },
                    ResourceChange {
                        resource: selected.resource.clone(),
                        value: self.store.encode_runtime_record(&request)?,
                    },
                ],
                result: json!({"id":row.id}),
                time: now() as i64,
            })?;
            if !outcome.applied {
                return Ok(());
            }
            guard.revision += 1;
            row
        };
        require(
            attempt.r#type == "workflow_reconciliation_attempt"
                && attempt.definition_id == request.id
                && attempt.data["instance_id"] == instance
                && attempt.data["execution_id"] == execution,
            "核对尝试改变了原来源",
        )?;
        let basis: ReadSet = serde_json::from_value(attempt.data["basis"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let result = self.inspect_workflow_reconciliation(&root, &resources, &attempt, &basis);
        let digest = configuration_hash(&(
            "workflow-reconciliation-observation/v1",
            &attempt.id,
            &result,
        ));
        let observed_key = ResourceKey::new(
            "governance",
            format!("workflow-reconciliation-observation:{digest}"),
        );
        let mut observation = record(
            "workflow_reconciliation_observation",
            attempt.id.clone(),
            request.scope.clone(),
            "原执行的实际核对观察".into(),
            1,
            None,
            json!({"attempt_id":attempt.id,"value":{"result":result},"issuer":"kernel_reconciliation"}),
            true,
        );
        observation.id = observed_key.id.clone();
        self.store.commit_changes(&PreparedChangeSet {
            command_id: observed_key.id.clone(),
            fingerprint: configuration_hash(&observation.data),
            read_set: ReadSet::new(vec![ResourceVersion {
                resource: observed_key.clone(),
                revision: 0,
            }])?,
            changes: vec![ResourceChange {
                resource: observed_key.clone(),
                value: observation.to_storage(),
            }],
            result: json!({"id":observation.id}),
            time: now() as i64,
        })?;
        let observed = ResourceVersion {
            resource: observed_key,
            revision: 1,
        };
        match result {
            Ok(ReconciliationFact::Pending) => Ok(()),
            Ok(fact) => {
                let current = ResourceSnapshot {
                    resource: selected.resource.clone(),
                    revision: guard.revision,
                    value: Some(request.to_storage()),
                };
                match self.admit_workflow_reconciliation(
                    &root,
                    &resources,
                    (&current, &request),
                    (&attempt, &observed),
                    basis,
                    fact,
                ) {
                    Err(error) => self.finish_unresolved_reconciliation(
                        &current,
                        &request,
                        Some(&observed),
                        &error,
                    ),
                    result => result,
                }
            }
            Err(error) => self.finish_unresolved_reconciliation(
                &ResourceSnapshot {
                    resource: selected.resource.clone(),
                    revision: guard.revision,
                    value: Some(request.to_storage()),
                },
                &request,
                Some(&observed),
                &error,
            ),
        }
    }
    fn inspect_workflow_reconciliation(
        &self,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
        attempt: &ConfigurationRecord,
        basis: &ReadSet,
    ) -> Result<ReconciliationFact> {
        let authorization = ResourceSnapshot {
            resource: ResourceKey::new("governance", &attempt.id),
            revision: attempt.revision,
            value: Some(attempt.to_storage()),
        };
        if let Some(sid) = attempt.data["session_id"].as_str() {
            let selected = one(self.store.as_ref(), "session", sid)?;
            let session: Session = serde_json::from_value(
                self.store.restore_runtime_value(
                    selected
                        .value
                        .as_ref()
                        .ok_or_else(|| err("not_found", "原会话缺失"))?,
                )?,
            )
            .map_err(|e| err("storage", e.to_string()))?;
            let recovery = self.workspace_recovery_source(&resources.execution)?;
            let tid = attempt.data["turn_id"]
                .as_str()
                .or_else(|| recovery.as_ref().map(|(id, _)| id.as_str()))
                .ok_or_else(|| err("needs_reconciliation", "原会话核对缺少真实轮次来源"))?;
            self.reconcile_authorized_workers(&session, tid, &authorization, basis)?;
            if let Some((failed_turn_id, previous_turn_id)) = recovery {
                return Ok(ReconciliationFact::WorkspaceContinuation {
                    failed_turn_id,
                    previous_turn_id,
                });
            }
            let observed = one(self.store.as_ref(), "workflow_turn_observation", tid)?;
            let value = observed
                .value
                .ok_or_else(|| err("not_found", "原轮次观察缺失"))?;
            if value["turn"]["status"] == "completed" {
                return Ok(ReconciliationFact::Agent { observation: value });
            }
            if let Some((error, source, _)) = self.unapplied_conversation_start(tid)? {
                return Ok(ReconciliationFact::AgentStartNotApplied { source, error });
            }
            let resolution = attempt.data["resolution"].as_str().unwrap_or("inspect");
            if resolution != "inspect" {
                require(
                    attempt.data["actor"]["kind"] == "human",
                    "人工副作用裁定需要真实 Human 签署",
                )?;
                let calls = one(
                    self.store.as_ref(),
                    "step_platform_calls",
                    &resources.execution.id,
                )?;
                require(
                    calls
                        .value
                        .as_ref()
                        .and_then(Value::as_array)
                        .is_some_and(Vec::is_empty),
                    "还有平台工具副作用需要先逐项核对",
                )?;
                return Ok(ReconciliationFact::HumanEffect {
                    resolution: resolution.into(),
                    evidence: text(&attempt.data, "evidence")?,
                });
            }
            return Err(err(
                "needs_reconciliation",
                "已核对并停止原 worker，供应商终态仍未知；保留原执行，等待有证据的人工处置",
            ));
        }
        let id = attempt.data["call_id"]
            .as_str()
            .ok_or_else(|| err("needs_reconciliation", "原执行没有可核对的工具或原生轮次"))?;
        let selected = one(self.store.as_ref(), "governance", id)?;
        let call = restored(self, &selected)?;
        require(
            call.scope.project_id() == Some(root.instance.project_id.as_str()),
            "原工具核对跨项目",
        )?;
        if call.data["remote_effect"] == "not_applied"
            && matches!(call.data["status"].as_str(), Some("failed" | "cancelled"))
        {
            return Ok(ReconciliationFact::NotApplied { source: call.id });
        }
        if call.r#type == "platform_call" {
            if call.data["status"] == "succeeded" {
                return Ok(ReconciliationFact::Platform {
                    receipt: call.data["receipt"].clone(),
                });
            }
            let effect = restored(
                self,
                &one(
                    self.store.as_ref(),
                    "governance",
                    &text(&call.data, "effect_id")?,
                )?,
            )?;
            let plan: PlatformPlan = serde_json::from_value(effect.data["plan"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            if (root.instance.cancel_requested
                || one(
                    self.store.as_ref(),
                    "workflow_stop_source",
                    &resources.execution.id,
                )?
                .value
                .is_some())
                && matches!(&plan.action, PlatformAction::Value(_))
            {
                return Ok(ReconciliationFact::NotApplied { source: effect.id });
            }
            if let Some((_, Ok(Some(receipt)))) = self.platform_observed(&effect, "result")? {
                return Ok(ReconciliationFact::Platform { receipt });
            }
            let inspected = (|| {
                if let PlatformAction::Delivery(action) = &plan.action
                    && let Some(receipt) = self
                        .recover_reconciled_delivery_sync(root, &effect, action, attempt, basis)?
                {
                    return Ok(Some(receipt));
                }
                self.inspect_platform_reconciliation(&effect, &plan.action)
            })();
            return match inspected {
                Err(error)
                    if matches!(&plan.action, PlatformAction::Delivery(action) if
                        (matches!(action.as_ref(),crate::platform_delivery::DeliveryAction::Merge {..}) && error.code=="stale_target")
                        || (matches!(action.as_ref(),crate::platform_delivery::DeliveryAction::Open {..}) && error.code=="obsolete_protection_preflight"))
                        && error.execution_failure.as_ref().is_some_and(|f| {
                            f.kind == ExecutionFailureKind::Technical
                                && f.effect == ExecutionFailureEffect::NotApplied
                        }) =>
                {
                    Ok(ReconciliationFact::PlatformNotApplied {
                        source: effect.id,
                        error,
                    })
                }
                result => result.map(|r| {
                    r.map_or(ReconciliationFact::Pending, |receipt| {
                        ReconciliationFact::Platform { receipt }
                    })
                }),
            };
        }
        require(call.r#type == "tool_call", "未知工具核对协议")?;
        let invocation: ToolInvocation = serde_json::from_value(call.data["invocation"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        require(
            invocation.operation_id.as_deref() == Some(resources.execution.id.as_str()),
            "外部工具不属于原执行",
        )?;
        if let Ok(receipt) = serde_json::from_value::<ToolReceipt>(call.data["receipt"].clone()) {
            return Ok(ReconciliationFact::External { receipt });
        }
        self.tool_executor
            .as_ref()
            .ok_or_else(|| err("unsupported", "缺少外部工具执行器"))?
            .inspect(&invocation)
            .map(|r| {
                r.map_or(ReconciliationFact::Pending, |receipt| {
                    ReconciliationFact::External { receipt }
                })
            })
    }
    fn admit_workflow_reconciliation(
        &self,
        root: &WorkflowHeaderFacts,
        resources: &WorkflowResources,
        (request_snapshot, request): (&ResourceSnapshot, &ConfigurationRecord),
        (attempt, observed): (&ConfigurationRecord, &ResourceVersion),
        basis: ReadSet,
        fact: ReconciliationFact,
    ) -> Result<()> {
        if let ReconciliationFact::WorkspaceContinuation {
            failed_turn_id,
            previous_turn_id,
        } = fact
        {
            let command = Command {
                command_id: format!("workspace-reconciliation-admit:{}", attempt.id),
                target: Some(root.record.id.clone()),
                expected_revision: Some(root.record.revision),
                payload: json!({"execution_id":resources.execution.id,"expected_step_revision":resources.step.revision,"plan_revision_id":root.instance.active_plan_revision_id,"control_version":root.instance.control_version,"failed_turn_id":failed_turn_id,"previous_turn_id":previous_turn_id,"reason":"按已授权的执行核对，验证原检查点、未发送证明和原始拒绝时间，恢复同一执行"}),
            };
            let context = InvocationContext::workflow(
                WorkflowActor::Coordinator {
                    source_id: request.id.clone(),
                },
                Some(root.instance.project_id.clone()),
            );
            let auth = crate::workspace_continuation::WorkspaceRecoveryAuthority {
                request_snapshot,
                request,
                attempt,
                observation: observed,
                basis: &basis,
            };
            let fingerprint =
                configuration_hash(&("workspace-reconciliation-admit/v1", &attempt.id, observed));
            return self
                .recover_workspace_continuation(&command, &fingerprint, &context, Some(&auth))
                .map(|_| ());
        }
        let command = Command {
            command_id: format!("workflow-reconciliation-admit:{}", attempt.id),
            target: Some(root.record.id.clone()),
            expected_revision: Some(root.record.revision),
            payload: json!({"attempt_id":attempt.id,"observation_id":observed.resource.id}),
        };
        let context = InvocationContext::workflow(
            WorkflowActor::Coordinator {
                source_id: request.id.clone(),
            },
            Some(root.instance.project_id.clone()),
        );
        let mut commit =
            WorkflowCommit::new(self, "runtime.workflow.reconciliation", &command, &context)?;
        commit.coordinator_origin = Some(
            json!({"kind":"runtime_coordinator","authorization_id":request.id,"observation_id":observed.resource.id}),
        );
        commit.witness(basis.versions().iter().cloned())?;
        commit.witness([
            version(request_snapshot),
            ResourceVersion {
                resource: ResourceKey::new("governance", &attempt.id),
                revision: attempt.revision,
            },
            observed.clone(),
        ])?;
        let stopped = root.instance.cancel_requested
            || one(
                self.store.as_ref(),
                "workflow_stop_source",
                &resources.execution.id,
            )?
            .value
            .is_some();
        let mut next = resources.execution.clone();
        let mut files = vec![];
        match fact {
            ReconciliationFact::PlatformNotApplied { source, error } => {
                require(
                    ["stale_target", "obsolete_protection_preflight"]
                        .contains(&error.code.as_str())
                        && error.execution_failure.as_ref().is_some_and(|f| {
                            f.kind == ExecutionFailureKind::Technical
                                && f.effect == ExecutionFailureEffect::NotApplied
                        }),
                    "缺少平台操作未应用的确切观察",
                )?;
                let selected = one(
                    self.store.as_ref(),
                    "governance",
                    attempt.data["call_id"].as_str().unwrap_or(""),
                )?;
                let call = restored(self, &selected)?;
                let original = one(self.store.as_ref(), "governance", &source)?;
                let effect = restored(self, &original)?;
                let plan: PlatformPlan = serde_json::from_value(effect.data["plan"].clone())
                    .map_err(|e| err("storage", e.to_string()))?;
                require(
                    effect.r#type == "platform_effect_intent"
                        && effect.immutable
                        && effect.definition_id == call.id
                        && call.r#type == "platform_call"
                        && call.data["request"]["instance_id"] == resources.record.id
                        && call.data["request"]["execution_id"] == resources.execution.id
                        && matches!(&plan.action, PlatformAction::Delivery(action) if
                            (matches!(action.as_ref(),crate::platform_delivery::DeliveryAction::Merge {..}) && error.code=="stale_target")
                            || (matches!(action.as_ref(),crate::platform_delivery::DeliveryAction::Open {..}) && error.code=="obsolete_protection_preflight")),
                    "未应用观察不属于原平台请求",
                )?;
                if error.code == "obsolete_protection_preflight"
                    && let PlatformAction::Delivery(action) = &plan.action
                    && let crate::platform_delivery::DeliveryAction::Open { context, .. } =
                        action.as_ref()
                {
                    let (proof, _) = self
                        .legacy_unapplied_pr_preflight(&effect, context)?
                        .ok_or_else(|| {
                            err("revision_conflict", "旧预检的未写入证明已变化，请重新核对")
                        })?;
                    commit.witness(proof.versions().iter().cloned())?;
                }
                commit.witness([version(&original)])?;
                commit.witness(
                    crate::platform_reconciliation::reconciliation_business_basis(
                        &plan,
                        &selected.resource,
                    )?
                    .versions()
                    .iter()
                    .cloned(),
                )?;
                if let Some(fence) = &plan.fence {
                    let mut closed = fence.clone();
                    let mut data = closed.data.clone();
                    data["status"] = json!("closed");
                    data["reconciliation_id"] = json!(request.id);
                    data["observation_id"] = json!(observed.resource.id);
                    update_record(&mut closed, data);
                    commit.change(ResourceChange {
                        resource: ResourceKey::new("governance", &closed.id),
                        value: closed.to_storage(),
                    })?;
                }
                let mut data = call.data.clone();
                data["status"] = json!("failed");
                data["remote_effect"] = json!("not_applied");
                data["error"] = json!(error);
                data["reconciliation_id"] = json!(request.id);
                data["reconciliation_observation_id"] = json!(observed.resource.id);
                commit.replace_record(&selected, call, data)?;
                next.status = if stopped {
                    StepExecutionStatus::Cancelled
                } else {
                    StepExecutionStatus::Failed
                };
                next.failure = Some(WorkflowFailure {
                    code: error.code,
                    message: error.message,
                    source_ref: observed.resource.id.clone(),
                    kind: ExecutionFailureKind::Technical,
                    effect: ExecutionFailureEffect::NotApplied,
                });
            }
            ReconciliationFact::Platform { receipt } => {
                let selected = one(
                    self.store.as_ref(),
                    "governance",
                    attempt.data["call_id"].as_str().unwrap_or(""),
                )?;
                let call = restored(self, &selected)?;
                self.prepare_platform_reconciliation_admission(
                    &mut commit,
                    &selected,
                    &call,
                    request,
                    observed,
                    &receipt,
                )?;
                if stopped {
                    next.status = StepExecutionStatus::Cancelled;
                    next.failure = Some(WorkflowFailure {
                        code: "observed_after_stop".into(),
                        message: "原平台结果已真实核对，原停止决定保持".into(),
                        source_ref: observed.resource.id.clone(),
                        kind: ExecutionFailureKind::Technical,
                        effect: ExecutionFailureEffect::Applied,
                    });
                } else {
                    let step = resources
                        .instance
                        .plan
                        .document
                        .steps
                        .iter()
                        .find(|s| s.key == next.step)
                        .ok_or_else(|| err("storage", "原步骤定义缺失"))?;
                    let outputs = crate::configured_runtime::mapped_tool_outputs(step, &receipt)?;
                    files=self.prepare_native_outputs(&mut commit,resources,&mut next,&json!({"status":"completed","summary":"原平台执行结果已通过显式核对","outputs":outputs}),&WorkflowOutputProof::Tool{call:Box::new(self.reconciled_platform_record(&call,request,observed,&receipt)),receipt},now())?;
                }
            }
            ReconciliationFact::External { receipt } => {
                let selected = one(
                    self.store.as_ref(),
                    "governance",
                    attempt.data["call_id"].as_str().unwrap_or(""),
                )?;
                let mut call = restored(self, &selected)?;
                require(
                    receipt.invocation_id == call.id && receipt.origin == Origin::External,
                    "原执行器回执身份不符",
                )?;
                if call.data["receipt"] != json!(receipt) {
                    let mut data = call.data.clone();
                    data["receipt"] = json!(receipt);
                    data["status"] = json!(if receipt.success {
                        "succeeded"
                    } else if receipt.remote_effect == "unknown" {
                        "needs_reconciliation"
                    } else {
                        "failed"
                    });
                    data["reconciliation_id"] = json!(request.id);
                    commit.replace_record(&selected, call.clone(), data.clone())?;
                    update_record(&mut call, data);
                }
                if receipt.remote_effect == "unknown" {
                    return Err(err("needs_reconciliation", "执行器回执仍声明未知副作用"));
                }
                if stopped {
                    next.status = StepExecutionStatus::Cancelled;
                } else if receipt.success {
                    let step = resources
                        .instance
                        .plan
                        .document
                        .steps
                        .iter()
                        .find(|s| s.key == next.step)
                        .ok_or_else(|| err("storage", "原工具步骤缺失"))?;
                    let value = receipt
                        .structured_content
                        .clone()
                        .unwrap_or(json!(receipt.content));
                    let outputs = crate::configured_runtime::mapped_tool_outputs(step, &value)?;
                    files=self.prepare_native_outputs(&mut commit,resources,&mut next,&json!({"status":"completed","summary":"原工具回执已真实核对","outputs":outputs}),&WorkflowOutputProof::Tool{call:Box::new(call),receipt:value},now())?;
                } else {
                    next.status = StepExecutionStatus::Failed;
                    next.failure = Some(WorkflowFailure {
                        code: "tool_failed".into(),
                        message: "原外部工具返回实际失败回执".into(),
                        source_ref: observed.resource.id.clone(),
                        kind: ExecutionFailureKind::Technical,
                        effect: if receipt.remote_effect == "not_applied" {
                            ExecutionFailureEffect::NotApplied
                        } else {
                            ExecutionFailureEffect::Applied
                        },
                    });
                }
            }
            ReconciliationFact::Agent { observation } => {
                require(
                    observation["turn"]["id"] == attempt.data["turn_id"]
                        && observation["turn"]["status"] == "completed",
                    "实际原生终态不属于原核对",
                )?;
                if stopped {
                    next.status = StepExecutionStatus::Cancelled;
                } else {
                    let value: Value =
                        serde_json::from_str(observation["assistant"].as_str().unwrap_or(""))
                            .map_err(|e| err("output_invalid", e.to_string()))?;
                    let runtime = serde_json::from_value(observation["runtime"].clone())
                        .map_err(|e| err("storage", e.to_string()))?;
                    files = self.prepare_native_outputs(
                        &mut commit,
                        resources,
                        &mut next,
                        &value,
                        &WorkflowOutputProof::Agent { runtime },
                        observation["turn"]["finished"]
                            .as_u64()
                            .ok_or_else(|| err("storage", "原实际终态缺少时间"))?,
                    )?;
                }
            }
            ReconciliationFact::AgentStartNotApplied { source, error } => {
                let tid = text(&attempt.data, "turn_id")?;
                let (actual, proof, reads) = self
                    .unapplied_conversation_start(&tid)?
                    .ok_or_else(|| err("revision_conflict", "原启动未执行证明已变化"))?;
                require(
                    proof == source && json!(actual) == json!(error),
                    "原启动观察不匹配",
                )?;
                commit.witness(reads.versions().iter().cloned())?;
                let turn = one(self.store.as_ref(), "turn", &tid)?;
                let mut raw = turn
                    .value
                    .clone()
                    .ok_or_else(|| err("not_found", "原轮次缺失"))?;
                raw["status"] = json!("failed");
                raw["finished"] = json!(now());
                raw["revision"] = json!(turn.revision + 1);
                commit.change(ResourceChange {
                    resource: turn.resource,
                    value: raw,
                })?;
                next.status = if stopped {
                    StepExecutionStatus::Cancelled
                } else {
                    StepExecutionStatus::Failed
                };
                let failure = error.execution_failure.as_ref().unwrap();
                next.failure = Some(WorkflowFailure {
                    code: error.code,
                    message: error.message,
                    source_ref: source,
                    kind: failure.kind,
                    effect: failure.effect,
                });
            }
            ReconciliationFact::NotApplied { source } => {
                next.status = if stopped {
                    StepExecutionStatus::Cancelled
                } else {
                    StepExecutionStatus::Failed
                };
                next.failure = Some(WorkflowFailure {
                    code: "observed_not_applied".into(),
                    message: "原意图具有未应用的实际依据，原执行不重新派发".into(),
                    source_ref: source,
                    kind: ExecutionFailureKind::Technical,
                    effect: ExecutionFailureEffect::NotApplied,
                });
            }
            ReconciliationFact::HumanEffect {
                resolution,
                evidence,
            } => {
                require(
                    request.data["actor"]["kind"] == "human",
                    "副作用裁定不是实际人类签署",
                )?;
                let decision=commit.new_record(record("workflow_manual_effect_decision",request.id.clone(),request.scope.clone(),"人类核对原副作用".into(),1,None,json!({"actor":request.data["actor"],"execution_id":next.id,"classification":resolution,"evidence":evidence,"observation_id":observed.resource.id,"issuer":"human_decision"}),true),"human-effect-decision")?;
                if let Some(tid) = &next.turn_id {
                    let turn = one(self.store.as_ref(), "turn", tid)?;
                    let mut raw = turn
                        .value
                        .clone()
                        .ok_or_else(|| err("not_found", "原轮次缺失"))?;
                    require(
                        raw["session_id"] == next.session_id.as_deref().unwrap_or(""),
                        "原裁定轮次不属于执行",
                    )?;
                    raw["status"] = json!("cancelled");
                    raw["cancel_requested"] = json!(true);
                    raw["finished"] = json!(now());
                    raw["revision"] = json!(turn.revision + 1);
                    commit.witness([version(&turn)])?;
                    commit.change(ResourceChange {
                        resource: turn.resource,
                        value: raw,
                    })?;
                }
                next.status = if stopped {
                    StepExecutionStatus::Cancelled
                } else {
                    StepExecutionStatus::Failed
                };
                next.failure = Some(WorkflowFailure {
                    code: "human_effect_decision".into(),
                    message: "实际人类已签署副作用分类；这不是供应商回执".into(),
                    source_ref: decision,
                    kind: if resolution == "not_applied" {
                        ExecutionFailureKind::Technical
                    } else {
                        ExecutionFailureKind::Business
                    },
                    effect: if resolution == "not_applied" {
                        ExecutionFailureEffect::NotApplied
                    } else {
                        ExecutionFailureEffect::Applied
                    },
                });
            }
            ReconciliationFact::Pending => return Ok(()),
            ReconciliationFact::WorkspaceContinuation { .. } => unreachable!(),
        }
        if next.status != StepExecutionStatus::NeedsReconciliation {
            next.runtime_context
                .as_object_mut()
                .map(|o| o.remove("reconciliation_request_id"));
            next.reason = next
                .failure
                .as_ref()
                .map(|f| f.message.clone())
                .unwrap_or_else(|| "原执行结果已通过显式核对".into());
        }
        commit.step(resources, &mut next)?;
        let mut data = request.data.clone();
        data["status"] = json!("completed");
        data["observation_id"] = json!(observed.resource.id);
        commit.replace_record(request_snapshot, request.clone(), data)?;
        let fingerprint =
            configuration_hash(&("workflow-reconciliation-admit/v1", &attempt.id, observed));
        let prepared = commit.prepare(&fingerprint, json!({"id":request.id}))?;
        if files.is_empty() {
            self.store.commit_changes(&prepared)?;
        } else {
            let borrowed = files
                .iter()
                .map(|(seal, body)| (seal.clone(), body.as_str()))
                .collect::<Vec<_>>();
            self.seal_command_batch_changes(prepared, &borrowed, request.scope.clone())?;
        }
        Ok(())
    }
    fn finish_unresolved_reconciliation(
        &self,
        snapshot: &ResourceSnapshot,
        request: &ConfigurationRecord,
        observation: Option<&ResourceVersion>,
        error: &Error,
    ) -> Result<()> {
        let mut row = request.clone();
        let mut data = row.data.clone();
        data["status"] = json!(if error.code == "revision_conflict" {
            "conflicted"
        } else {
            "unresolved"
        });
        data["error"] = json!(error);
        data["observation_id"] = json!(observation.map(|o| &o.resource.id));
        update_record(&mut row, data);
        let mut reads = ReadSet::new(vec![version(snapshot)])?;
        if let Some(observed) = observation {
            reads = reads.merge([observed.clone()])?;
        }
        self.store.commit_changes(&PreparedChangeSet {
            command_id: format!(
                "workflow-reconciliation-unresolved:{}:{}",
                row.id, snapshot.revision
            ),
            fingerprint: configuration_hash(&row.data),
            read_set: reads,
            changes: vec![ResourceChange {
                resource: snapshot.resource.clone(),
                value: self.store.encode_runtime_record(&row)?,
            }],
            result: json!({"id":row.id}),
            time: now() as i64,
        })?;
        Ok(())
    }
}
