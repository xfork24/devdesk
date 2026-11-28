//! Once-granted platform effects. Physical observations are immutable and
//! survive rejection by the original resource-level admission conditions.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    platform_facts::{PlatformAction, PlatformPlan, one, restored},
    workflow_resources::version,
    *,
};

struct PlatformCall {
    row: ConfigurationRecord,
    guard: ResourceVersion,
    request: PlatformToolRequest,
}
impl PlatformCall {
    fn read(engine: &Engine, id: &str) -> Result<Self> {
        let selected = one(engine.store.as_ref(), "governance", id)?;
        let row = restored(engine, &selected)?;
        require(
            row.r#type == "platform_call"
                && row.id == row.definition_id
                && row.scope.project_id() == engine.store.project_id().as_deref(),
            "平台调用身份或作用域不符",
        )?;
        let request: PlatformToolRequest = serde_json::from_value(row.data["request"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        require(
            request.execution_id == row.id
                && row.scope.project_id() == Some(request.project_id.as_str()),
            "平台调用改变了原请求",
        )?;
        Ok(Self {
            row,
            guard: version(&selected),
            request,
        })
    }
    fn changed(&self, engine: &Engine, data: Value) -> Result<ResourceChange> {
        let mut row = self.row.clone();
        update_record(&mut row, data);
        Ok(ResourceChange {
            resource: self.guard.resource.clone(),
            value: engine.store.encode_runtime_record(&row)?,
        })
    }
    fn commit(
        &self,
        engine: &Engine,
        phase: &str,
        reads: ReadSet,
        changes: Vec<ResourceChange>,
        data: Value,
    ) -> Result<CommitOutcome> {
        let mut all = changes;
        all.push(self.changed(engine, data)?);
        engine.store.submit_changes(&PreparedChangeSet {
            command_id: format!("platform-{phase}:{}:{}", self.row.id, self.guard.revision),
            fingerprint: configuration_hash(&("platform-phase/v1", &self.guard, phase)),
            read_set: reads.merge([self.guard.clone()])?,
            changes: all,
            result: json!({"id":self.row.id}),
            time: now() as i64,
        })
    }
    fn active(&self, engine: &Engine) -> Result<(bool, bool, ReadSet)> {
        let authority = serde_json::from_value(self.row.data["workflow_authority"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let live = engine
            .store
            .workflow_tool_facts(&authority, &self.request.project_id)?;
        let mut reads = ReadSet::new(live.versions)?;
        let caller = if let Some(caller) = &self.request.caller {
            match engine.platform_caller_guards(caller) {
                Ok(r) => {
                    reads = reads.merge(r.versions().iter().cloned())?;
                    true
                }
                Err(e)
                    if matches!(
                        e.code.as_str(),
                        "invalid_params" | "tool_not_authorized" | "revision_conflict"
                    ) =>
                {
                    false
                }
                Err(e) => return Err(e),
            }
        } else {
            true
        };
        Ok((
            live.allowed && caller && self.row.data["cancel_requested"] != true,
            live.suspended,
            reads,
        ))
    }
    fn prepare(&self, engine: &Engine) -> Result<()> {
        if self.row.data["status"] != "prepared" {
            return Ok(());
        }
        let Some(_attempt) = engine.effect_in_flight.enter(&self.row.id)? else {
            return Ok(());
        };
        let _prepare = engine
            .store
            .observe_native_call(&self.row.id, "native.platform.prepare");
        let (active, suspended, live) = self.active(engine)?;
        if suspended {
            return Ok(());
        }
        if !active {
            let mut data = self.row.data.clone();
            data["status"] = json!("cancelled");
            data["error"] = json!(err("cancelled", "平台原调用资格已撤销"));
            data["remote_effect"] = json!("not_applied");
            self.commit(engine, "denied", live, vec![], data)?;
            return Ok(());
        }
        let mut plan = match engine.prepare_platform_plan(&self.request) {
            Ok(plan) => plan,
            Err(error)
                if matches!(
                    error.code.as_str(),
                    "resource_pending" | "storage_busy" | "revision_conflict"
                ) =>
            {
                return Err(error);
            }
            Err(mut error) => {
                // No external effect has been granted or dispatched yet.
                // Preserve this trusted boundary in the workflow failure;
                // callers must not invent an unknown Docker/Git outcome for
                // a rejected input or scope check, nor automatically retry it.
                error.execution_failure = Some(ExecutionFailureDisposition {
                    kind: ExecutionFailureKind::Business,
                    effect: ExecutionFailureEffect::NotApplied,
                });
                let mut data = self.row.data.clone();
                data["status"] = json!("failed");
                data["error"] = json!(error);
                data["issuer"] = json!("kernel_platform");
                data["remote_effect"] = json!("not_applied");
                self.commit(engine, "preflight-rejected", live, vec![], data)?;
                return Ok(());
            }
        };
        let key = ResourceKey::new("governance", format!("platform-effect:{}", self.row.id));
        let mut initial =
            plan.basis
                .merge(live.versions().iter().cloned())?
                .merge([ResourceVersion {
                    resource: key.clone(),
                    revision: 0,
                }])?;
        let mut changes = vec![];
        // The exact step version authorizes the grant. The durable receipt
        // instead keeps the original semantic authority: attaching the queued
        // tool_call_id is bookkeeping, not revocation of the running contract.
        // The header, upstream steps and business/source witnesses stay exact.
        let own_step = ResourceKey::new(
            "workflow_step",
            json!([self.request.instance_id, self.request.step_execution_id()]).to_string(),
        );
        plan.basis = ReadSet::new(
            plan.basis
                .versions()
                .iter()
                .filter(|v| {
                    v.resource != self.guard.resource
                        && v.resource != own_step
                        && !(plan.fence.is_some() && v.resource.kind == "conversation_writers")
                })
                .cloned()
                .collect(),
        )?
        .merge(live.versions().iter().cloned())?;
        if let Some(fence) = &plan.fence {
            let fence_key = ResourceKey::new("governance", &fence.id);
            initial = initial.merge([ResourceVersion {
                resource: fence_key.clone(),
                revision: 0,
            }])?;
            changes.push(ResourceChange {
                resource: fence_key.clone(),
                value: fence.to_storage(),
            });
            plan.basis = plan.basis.merge([ResourceVersion {
                resource: fence_key,
                revision: 1,
            }])?;
        }
        if let Some(permit) = &plan.permit {
            let mut row = record(
                "workflow_action_permit",
                self.row.id.clone(),
                self.row.scope.clone(),
                "冻结候选接纳许可".into(),
                1,
                None,
                json!(permit),
                true,
            );
            row.id = format!("platform-permit:{}", self.row.id);
            let k = ResourceKey::new("governance", &row.id);
            initial = initial.merge([ResourceVersion {
                resource: k.clone(),
                revision: 0,
            }])?;
            changes.push(ResourceChange {
                resource: k,
                value: row.to_storage(),
            });
        }
        let mut effect = record(
            "platform_effect_intent",
            self.row.id.clone(),
            self.row.scope.clone(),
            "平台外部操作原始意图".into(),
            1,
            None,
            json!({"status":"granted","call_version":{"resource":self.guard.resource,"revision":self.guard.revision+1},"request_digest":configuration_hash(&self.request),"plan":plan,"issuer":"kernel_platform"}),
            true,
        );
        effect.id = key.id.clone();
        changes.push(ResourceChange {
            resource: key,
            value: engine.store.encode_runtime_record(&effect)?,
        });
        let mut data = self.row.data.clone();
        data["status"] = json!("running");
        data["effect_id"] = json!(effect.id);
        data["started"] = json!(now());
        if self
            .commit(engine, "grant", initial, changes, data)?
            .applied
        {
            let running = Self::read(engine, &self.row.id)?;
            match &plan.action {
                PlatformAction::Publish { .. } | PlatformAction::Delivery(_) => {}
                _ => {
                    let result = {
                        let _external = engine
                            .store
                            .observe_native_call(&self.row.id, "native.platform.external");
                        // Every contract that guards this operation has to allow it first.
                        plan.domain_checks
                            .iter()
                            .try_for_each(|check| {
                                engine.verify_domain_operation(
                                    &effect,
                                    &self.request.project_id,
                                    check,
                                )
                            })
                            .and_then(|()| engine.perform_platform_action(&plan.action, true))
                    };
                    let phase = if matches!(&plan.action, PlatformAction::Test(_))
                        && matches!(&result, Ok(None))
                    {
                        "start"
                    } else {
                        "result"
                    };
                    let observed = engine.platform_observe(&effect, phase, &result)?;
                    if phase == "result" {
                        running.receive(engine, &effect, &plan, &observed, &result)?;
                    }
                }
            }
        }
        Ok(())
    }
    fn receive(
        &self,
        engine: &Engine,
        effect: &ConfigurationRecord,
        plan: &PlatformPlan,
        observation: &ResourceVersion,
        result: &Result<Option<Value>>,
    ) -> Result<()> {
        if matches!(result, Ok(None)) {
            return Ok(());
        }
        let _admission = engine
            .store
            .observe_native_call(&self.row.id, "native.platform.admission");
        let expected: ResourceVersion = serde_json::from_value(effect.data["call_version"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        require(
            effect.definition_id == self.row.id
                && self.row.data["effect_id"] == effect.id
                && effect.data["request_digest"] == configuration_hash(&self.request),
            "平台结果不能替换原调用或原始请求",
        )?;
        if self.guard != expected {
            return self.rejected(
                engine,
                effect,
                observation,
                &err("revision_conflict", "平台原始接纳版本已变化"),
            );
        }
        let mut data = self.row.data.clone();
        let mut changes = vec![];
        let not_applied = result.as_ref().err().is_some_and(|error| {
            error
                .execution_failure
                .is_some_and(|d| d.effect == ExecutionFailureEffect::NotApplied)
        });
        match result {
            Ok(Some(receipt)) => {
                let validation = engine
                    .validate_platform_result(&plan.action, receipt)
                    .and_then(|_| {
                        engine
                            .tool_executor
                            .as_ref()
                            .ok_or_else(|| err("unsupported", "缺少输出校验器"))?
                            .validate_arguments(&self.request.definition.output_schema, receipt)
                    });
                if let Err(error) = validation {
                    return self.rejected(
                        engine,
                        effect,
                        observation,
                        &err("receipt_rejected", error.to_string()),
                    );
                }
                if let PlatformAction::Test(test) = &plan.action {
                    engine.prepare_platform_test_cleanup(effect, test, observation)?;
                }
                changes.extend(plan.changes.clone());
                if let PlatformAction::Delivery(action) = &plan.action
                    && let Some(change) = engine.delivery_admission_change(action, receipt)?
                {
                    changes.push(change);
                }
                if let Some(head) = &plan.head {
                    let mut next = head
                        .value
                        .clone()
                        .ok_or_else(|| err("not_found", "原代码头缺失"))?;
                    next["revision"] = json!(text(receipt, "revision")?);
                    changes.push(ResourceChange {
                        resource: head.resource.clone(),
                        value: next,
                    });
                }
                data["status"] = json!("succeeded");
                data["receipt"] = receipt.clone();
                data["receipt_digest"] = json!(configuration_hash(receipt));
                data["remote_effect"] = json!("applied");
            }
            Err(error) => {
                let read_only = matches!(
                    &plan.action,
                    PlatformAction::Value(_)
                        | PlatformAction::DomainRead { .. }
                        | PlatformAction::WorkspaceCurrent { .. }
                        | PlatformAction::ReusedPublication { .. }
                );
                let physical_unknown = !read_only && !not_applied;
                data["status"] = json!(
                    if physical_unknown || error.code == "needs_reconciliation" {
                        "needs_reconciliation"
                    } else if error.code == "cancelled" {
                        "cancelled"
                    } else {
                        "failed"
                    }
                );
                data["error"] = json!(error);
                data["remote_effect"] = json!(if not_applied || read_only {
                    "not_applied"
                } else {
                    "unknown"
                });
            }
            _ => unreachable!(),
        }
        // A known failed read never held a physical writer. Mutating effects
        // retain their fence until there is a positive physical completion.
        if (result.is_ok() || not_applied)
            && let Some(fence) = &plan.fence
        {
            let mut closed = fence.clone();
            let mut d = closed.data.clone();
            d["status"] = json!("closed");
            d["observation_id"] = json!(observation.resource.id);
            update_record(&mut closed, d);
            changes.push(ResourceChange {
                resource: ResourceKey::new("governance", &closed.id),
                value: closed.to_storage(),
            });
        }
        data["observation_id"] = json!(observation.resource.id);
        data["finished"] = json!(now());
        // A proven rejected external attempt changes only its own call/fence.
        // The target moving is the expected cause of rejection, not a reason
        // to hide that negative observation behind a stale-head conflict.
        let basis = if not_applied {
            ReadSet::new(
                plan.basis
                    .versions()
                    .iter()
                    .filter(|v| plan.head.as_ref().is_none_or(|h| v.resource != h.resource))
                    .cloned()
                    .collect(),
            )?
        } else {
            plan.basis.clone()
        };
        let reads = basis.merge([
            ResourceVersion {
                resource: ResourceKey::new("governance", &effect.id),
                revision: effect.revision,
            },
            observation.clone(),
        ])?;
        match self.commit(engine, "admit", reads, changes, data) {
            Ok(_) => Ok(()),
            Err(error) if error.code == "revision_conflict" => {
                self.rejected(engine, effect, observation, &error)
            }
            Err(error) => Err(error),
        }
    }
    fn rejected(
        &self,
        engine: &Engine,
        effect: &ConfigurationRecord,
        observed: &ResourceVersion,
        error: &devdesk_domain::Error,
    ) -> Result<()> {
        let key = ResourceKey::new(
            "governance",
            format!(
                "platform-rejected:{}",
                configuration_hash(&(&effect.id, &observed.resource.id, error))
            ),
        );
        let mut row = record(
            "platform_receipt_rejection",
            self.row.id.clone(),
            self.row.scope.clone(),
            "保留原始平台结果冲突".into(),
            1,
            None,
            json!({"effect_id":effect.id,"observation_id":observed.resource.id,"original_call_version":effect.data["call_version"],"error":error,"issuer":"kernel_platform"}),
            true,
        );
        row.id = key.id.clone();
        engine.store.commit_changes(&PreparedChangeSet {
            command_id: format!("platform-rejected:{}", row.id),
            fingerprint: configuration_hash(&row.data),
            read_set: ReadSet::new(vec![
                observed.clone(),
                ResourceVersion {
                    resource: key.clone(),
                    revision: 0,
                },
            ])?,
            changes: vec![ResourceChange {
                resource: key,
                value: row.to_storage(),
            }],
            result: json!({"id":row.id}),
            time: now() as i64,
        })?;
        let current = Self::read(engine, &self.row.id)?;
        if current.row.data["status"] == "running" && current.row.data["effect_id"] == effect.id {
            let mut data = current.row.data.clone();
            data["status"] = json!("needs_reconciliation");
            data["error"] = json!(if error.code == "receipt_rejected" {
                error.clone()
            } else {
                err(
                    "receipt_conflict",
                    "外部结果已保存，原接纳版本冲突；保留原请求等待显式核对",
                )
            });
            data["observation_id"] = json!(observed.resource.id);
            data["remote_effect"] = json!("unknown");
            current.commit(
                engine,
                "conflict-project",
                ReadSet::new(vec![observed.clone()])?,
                vec![],
                data,
            )?;
        }
        Ok(())
    }
    fn recover(&self, engine: &Engine) -> Result<()> {
        if self.row.data["status"] != "running" {
            return Ok(());
        }
        if engine.effect_in_flight.contains(&self.row.id)? {
            return Ok(());
        }
        let selected = one(
            engine.store.as_ref(),
            "governance",
            &text(&self.row.data, "effect_id")?,
        )?;
        let effect = restored(engine, &selected)?;
        let plan: PlatformPlan = serde_json::from_value(effect.data["plan"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        require(
            effect.r#type == "platform_effect_intent"
                && effect.immutable
                && effect.definition_id == self.row.id,
            "平台原始执行意图不符",
        )?;
        if let Some((observation, result)) = engine.platform_observed(&effect, "result")? {
            return self.receive(engine, &effect, &plan, &observation, &result);
        }
        // Every contract that guards this operation has to allow it before the operation runs.
        let verified = plan.domain_checks.iter().try_for_each(|check| {
            engine.verify_domain_operation(&effect, &self.request.project_id, check)
        });
        let result = match &plan.action {
            PlatformAction::Publish { .. } => engine.drive_platform_publish(&effect, &plan.action),
            PlatformAction::Delivery(action) => {
                verified.and_then(|()| engine.drive_delivery(&effect, action, false))
            }
            PlatformAction::Test(test) => {
                let (active, _, _) = self.active(engine)?;
                match engine.tests.inspect_test(test)? {
                    ports::TestExecutionObservation::Completed { result } => Ok(Some(result)),
                    ports::TestExecutionObservation::Running { timed_out }
                        if !active || timed_out =>
                    {
                        if !engine.stop_platform_test(&effect, test)? {
                            return Ok(());
                        }
                        Err(err(
                            if timed_out {
                                "test_timeout"
                            } else {
                                "cancelled"
                            },
                            "原测试 worker 已取得实际停止证明",
                        ))
                    }
                    ports::TestExecutionObservation::Running { .. } => return Ok(()),
                    ports::TestExecutionObservation::Stopped => {
                        Err(err("cancelled", "原测试执行已停止"))
                    }
                    ports::TestExecutionObservation::Unknown
                    | ports::TestExecutionObservation::NotStarted => Err(err(
                        "needs_reconciliation",
                        "原测试已有一次派发资格但结果不明，不重新启动",
                    )),
                }
            }
            _ => verified.and_then(|()| engine.perform_platform_action(&plan.action, false)),
        };
        if matches!(&result, Ok(None)) {
            return Ok(());
        }
        let observed = engine.platform_observe(&effect, "result", &result)?;
        self.receive(engine, &effect, &plan, &observed, &result)
    }
}
impl Engine {
    /// The durable call already exists. This fast path uses the same once-only
    /// grant and observation admission as the ordinary scheduler.
    pub fn drive_ready_native_platform_tool(&self, id: &str) -> Result<()> {
        let call = PlatformCall::read(self, id)?;
        require(
            call.request.caller.is_some(),
            "即时平台推进需要原生调用来源",
        )?;
        call.prepare(self)?;
        PlatformCall::read(self, id)?.recover(self)?;
        self.reply_native_platform_calls()
    }
    pub(crate) fn platform_observe(
        &self,
        effect: &ConfigurationRecord,
        phase: &str,
        result: &Result<Option<Value>>,
    ) -> Result<ResourceVersion> {
        let digest = configuration_hash(&("platform-observation/v1", &effect.id, phase, result));
        let key = ResourceKey::new("governance", format!("platform-observation:{digest}"));
        let mut row = record(
            "platform_effect_observation",
            effect.id.clone(),
            effect.scope.clone(),
            phase.into(),
            1,
            None,
            json!({"phase":phase,"effect_id":effect.id,"effect_digest":configuration_hash(&effect.data),"result":result,"issuer":"kernel_platform"}),
            true,
        );
        row.id = key.id.clone();
        let receipt = self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("platform-observe:{digest}"),
            fingerprint: digest,
            read_set: ReadSet::new(vec![ResourceVersion {
                resource: key.clone(),
                revision: 0,
            }])?,
            changes: vec![ResourceChange {
                resource: key.clone(),
                value: row.to_storage(),
            }],
            result: json!({"id":row.id}),
            time: now() as i64,
        })?;
        receipt
            .versions
            .into_iter()
            .find(|v| v.resource == key)
            .ok_or_else(|| err("storage", "平台观察缺少原始版本"))
    }
    pub(crate) fn platform_observed(
        &self,
        effect: &ConfigurationRecord,
        phase: &str,
    ) -> Result<Option<(ResourceVersion, Result<Option<Value>>)>> {
        let selected = one(
            self.store.as_ref(),
            "platform_observation_head",
            &json!([effect.id, phase]).to_string(),
        )?;
        let Some(raw) = &selected.value else {
            return Ok(None);
        };
        let row = ConfigurationRecord::from_storage(raw.clone())?;
        require(
            row.r#type == "platform_effect_observation"
                && row.immutable
                && row.definition_id == effect.id
                && row.data["effect_digest"] == configuration_hash(&effect.data)
                && row.data["phase"] == phase,
            "平台观察不能替换原执行意图",
        )?;
        let result = serde_json::from_value(row.data["result"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        Ok(Some((
            ResourceVersion {
                resource: ResourceKey::new("governance", row.id),
                revision: row.revision,
            },
            result,
        )))
    }
    pub(crate) fn perform_platform_action(
        &self,
        action: &PlatformAction,
        fresh: bool,
    ) -> Result<Option<Value>> {
        match action {
            PlatformAction::Delivery(_) => {
                Err(err("unsupported", "精简交付通过独立持久化阶段执行"))
            }
            PlatformAction::ReusedPublication { receipt, .. } => {
                // This grant contains a verified original immutable observation.
                // Replaying its fact never dispatches another Git/GitHub effect.
                Ok(Some(receipt.clone()))
            }
            PlatformAction::DomainRead {
                project,
                handler,
                input,
            } => {
                if !fresh {
                    return Err(err("needs_reconciliation", "领域读取缺少原观察"));
                }
                self.domain_reader
                    .as_ref()
                    .ok_or_else(|| err("domain_inactive", "领域运行端未启用"))?
                    .read(project, handler, input.clone())
                    .map(Some)
            }
            PlatformAction::Value(value) => {
                if fresh {
                    Ok(Some(value.clone()))
                } else {
                    Err(err(
                        "needs_reconciliation",
                        "原读取已获一次执行资格但没有观察，保留核对",
                    ))
                }
            }
            PlatformAction::WorkspaceCurrent { context, workspace } => {
                if fresh {
                    Ok(Some(
                        json!({"workspace_id":workspace.id,"revision":self.git.workspace_revision(context)?,"owner_id":workspace.owner_id}),
                    ))
                } else {
                    Err(err(
                        "needs_reconciliation",
                        "不重新读取可变工作空间来冒充原观察",
                    ))
                }
            }
            PlatformAction::Snapshot {
                operation,
                workspace,
                include_diff,
                scope_base,
            } => {
                let receipt = if fresh {
                    self.git.repository_operation(operation)?
                } else {
                    self.git
                        .inspect_repository_operation(operation)?
                        .ok_or_else(|| {
                            err("needs_reconciliation", "原快照缺少实际完成证明，不重新封存")
                        })?
                };
                let RepositoryReceipt::Snapshot { revision } = receipt else {
                    return Err(err("adapter_contract", "快照回执类型不符"));
                };
                let (context, expected) = match &operation.action {
                    RepositoryAction::Snapshot {
                        context, expected, ..
                    }
                    | RepositoryAction::Checkpoint {
                        context, expected, ..
                    } => (context, expected),
                    _ => return Err(err("storage", "原快照意图类型不符")),
                };
                let mut out = json!({"workspace_id":workspace.id,"revision":revision,"owner_id":workspace.owner_id});
                if *include_diff {
                    out["changes"] = json!({"base_revision":expected,"revision":revision,"diff":self.git.diff(context,expected,&revision)?});
                }
                if let Some(base) = scope_base {
                    out["scope_changes"] = json!({"base_revision":base,"revision":revision,"diff":self.git.diff(context,base,&revision)?});
                }
                Ok(Some(out))
            }
            PlatformAction::Test(test) => {
                if fresh {
                    self.tests.poll_test(test)
                } else {
                    Err(err(
                        "needs_reconciliation",
                        "测试重入必须通过只读原操作核对",
                    ))
                }
            }
            PlatformAction::Candidate {
                context,
                source,
                target,
                operation,
                workspace,
            } => {
                let revision = if fresh {
                    self.git.preflight_advance(context, target)?;
                    self.git.candidate(context, source, target, operation)?
                } else {
                    let op = RepositoryOperation {
                        id: operation.clone(),
                        project_id: context.project.clone(),
                        action: RepositoryAction::Candidate {
                            merge: operation.clone(),
                            context: context.clone(),
                            source: source.clone(),
                            target: target.clone(),
                            session: operation.clone(),
                        },
                        status: RepositoryOperationStatus::Dispatched,
                        receipt: None,
                        reason: String::new(),
                        revision: 1,
                    };
                    let Some(RepositoryReceipt::Candidate { revision }) =
                        self.git.inspect_repository_operation(&op)?
                    else {
                        return Err(err(
                            "needs_reconciliation",
                            "原候选缺少物理 marker，不重新构造",
                        ));
                    };
                    revision
                };
                let parents = self.git.commit_parents(context, &revision)?;
                require(
                    parents == vec![target.clone()],
                    "候选实际父提交与冻结目标不符",
                )?;
                Ok(Some(
                    json!({"revision":revision,"source_revision":source,"target_revision":target,"parents":parents,"diff":self.git.diff(context,target,&revision)?,"workspace_id":workspace,"owner_id":context.source.owner,"operation_id":operation}),
                ))
            }
            PlatformAction::Advance {
                context,
                expected,
                candidate,
                operation,
            } => {
                let revision = if fresh {
                    let actual = self.git.workspace_revision(&GitContext {
                        source: context.target.clone(),
                        ..context.clone()
                    })?;
                    self.git.preflight_advance(context, expected)?;
                    self.git
                        .advance(context, &actual, expected, candidate, operation)?
                } else {
                    match self.git.inspect_advance(context, operation)? {
                        AdvanceObservation::Applied { revision } => revision,
                        _ => {
                            return Err(err(
                                "needs_reconciliation",
                                "原推进缺少已同步的物理完成证明，不重新 CAS",
                            ));
                        }
                    }
                };
                require(&revision == candidate, "Git 推进未观察到确切候选")?;
                Ok(Some(
                    json!({"revision":revision,"previous":expected,"operation_id":operation}),
                ))
            }
            PlatformAction::Publish { .. } => {
                Err(err("unsupported", "发布通过各阶段独立持久化意图执行"))
            }
        }
    }
    pub(crate) fn validate_platform_result(
        &self,
        action: &PlatformAction,
        result: &Value,
    ) -> Result<()> {
        if let PlatformAction::Test(request) = action {
            let observed: TestResult = serde_json::from_value(result.clone())
                .map_err(|e| err("adapter_contract", e.to_string()))?;
            require(
                observed.operation_id == request.operation_id
                    && observed.candidate == request.candidate
                    && observed.environment == request.environment
                    && observed.origin == Origin::External,
                "测试回执改变了原操作、候选或冻结环境",
            )?;
        }
        Ok(())
    }
    fn stop_platform_test(
        &self,
        effect: &ConfigurationRecord,
        test: &ports::TestRequest,
    ) -> Result<bool> {
        let key = ResourceKey::new("governance", format!("platform-test-stop:{}", effect.id));
        let previous = one(self.store.as_ref(), "governance", &key.id)?;
        if previous.value.is_some() {
            if let Some((proof, Ok(Some(value)))) = self.platform_observed(effect, "stop")?
                && value["stopped"] == true
            {
                self.prepare_platform_test_cleanup(effect, test, &proof)?;
                return Ok(true);
            }
            if matches!(
                self.tests.inspect_test(test)?,
                ports::TestExecutionObservation::Stopped
            ) {
                let result = Ok(Some(json!({"stopped":true,"inspected":true})));
                let proof = self.platform_observe(effect, "stop-proof", &result)?;
                self.prepare_platform_test_cleanup(effect, test, &proof)?;
                return Ok(true);
            }
            return Ok(false);
        }
        let mut row = record(
            "platform_test_stop_intent",
            effect.id.clone(),
            effect.scope.clone(),
            "停止原测试 worker".into(),
            1,
            None,
            json!({"request":test,"issuer":"kernel_platform"}),
            true,
        );
        row.id = key.id.clone();
        let outcome = self.store.submit_changes(&PreparedChangeSet {
            command_id: key.id.clone(),
            fingerprint: configuration_hash(&row.data),
            read_set: ReadSet::new(vec![
                version(&previous),
                ResourceVersion {
                    resource: ResourceKey::new("governance", &effect.id),
                    revision: effect.revision,
                },
            ])?,
            changes: vec![ResourceChange {
                resource: key,
                value: row.to_storage(),
            }],
            result: json!({"id":row.id}),
            time: now() as i64,
        })?;
        if !outcome.applied {
            return Ok(false);
        }
        let result = self
            .tests
            .stop_test(test)
            .map(|stopped| Some(json!({"stopped":stopped})));
        let observed = self.platform_observe(effect, "stop", &result)?;
        if result
            .as_ref()
            .is_ok_and(|r| r.as_ref().is_some_and(|r| r["stopped"] == true))
        {
            self.prepare_platform_test_cleanup(effect, test, &observed)?;
        }
        result.map(|r| r.as_ref().is_some_and(|r| r["stopped"] == true))
    }
    pub(crate) fn stop_owned_platform_tests(&self) -> Result<()> {
        for status in ["prepared", "running", "needs_reconciliation"] {
            let mut after = None;
            loop {
                let page =
                    self.store
                        .runtime_record_page("platform_call", status, after.as_deref())?;
                for id in page.ids {
                    let call = PlatformCall::read(self, &id)?;
                    let effect = call.row.data["effect_id"]
                        .as_str()
                        .map(|id| one(self.store.as_ref(), "governance", id))
                        .transpose()?;
                    if let Some(effect) = effect
                        && effect.value.is_some()
                    {
                        let effect = restored(self, &effect)?;
                        let plan: PlatformPlan =
                            serde_json::from_value(effect.data["plan"].clone())
                                .map_err(|e| err("storage", e.to_string()))?;
                        if let PlatformAction::Test(test) = &plan.action {
                            self.stop_platform_test(&effect, test)?;
                        }
                    }
                }
                match page.next_after {
                    Some(next) => after = Some(next),
                    None => break,
                }
            }
        }
        self.drive_platform_test_cleanup()
    }
    pub fn drive_platform_tools(&self) -> Result<()> {
        self.drive_agent_context_calls()?;
        self.drive_platform_test_cleanup()?;
        let started = std::time::Instant::now();
        let mut visited = 0;
        let start = {
            let mut cursors = self
                .runtime_callback_cursors
                .lock()
                .map_err(|e| err("storage", e.to_string()))?;
            let start = cursors
                .get("platform-phase")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0)
                % 2;
            cursors.insert("platform-phase".into(), ((start + 1) % 2).to_string());
            start
        };
        'calls: for status in ["prepared", "running"]
            .into_iter()
            .cycle()
            .skip(start)
            .take(2)
        {
            for selected in self.pending_runtime_records("platform_call", status)? {
                if visited > 0 && started.elapsed() >= std::time::Duration::from_millis(25) {
                    break 'calls;
                }
                visited += 1;
                let work = (|| -> Result<()> {
                    let call = PlatformCall::read(self, &selected.resource.id)?;
                    call.prepare(self)?;
                    PlatformCall::read(self, &selected.resource.id)?.recover(self)
                })();
                if let Err(error) = work
                    && !matches!(
                        error.code.as_str(),
                        "revision_conflict"
                            | "storage_busy"
                            | "resource_pending"
                            | "workflow_tool_suspended"
                    )
                {
                    return Err(error);
                }
            }
        }
        self.reply_native_platform_calls()?;
        self.drive_platform_test_cleanup()
    }
}
