//! Preserve quiescent, uncommitted work between Turns of the same execution.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    platform_facts::{one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_resources::version,
    *,
};
pub(crate) struct WorkspaceRecoveryAuthority<'a> {
    pub request_snapshot: &'a ResourceSnapshot,
    pub request: &'a ConfigurationRecord,
    pub attempt: &'a ConfigurationRecord,
    pub observation: &'a ResourceVersion,
    pub basis: &'a ReadSet,
}

impl Engine {
    pub(crate) fn workspace_turn_checkpoint(
        &self,
        session: &Session,
        tid: &str,
    ) -> Result<Option<(ConfigurationRecord, ReadSet)>> {
        let selected = one(self.store.as_ref(), "conversation_open_seed", tid)?;
        if selected.value.is_none() {
            return Ok(None);
        }
        let seed = ConfigurationRecord::from_storage(selected.value.clone().unwrap())?;
        let request = &seed.data["request"];
        if !request["configured_task"].is_object() || request["code_view"].is_null() {
            return Ok(None);
        }
        let view: FrozenCode = serde_json::from_value(request["code_view"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let binding: RuntimeBinding = serde_json::from_value(
            self.store
                .restore_runtime_value(&request["session"]["runtime_binding"])?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        if !seed.data["authority"]["workflow"].is_object()
            || request["session"]["id"] != session.id
            || request["session"]["agent"] != session.agent
            || Some(&binding) != session.runtime_binding.as_ref()
            || session
                .runtime_binding
                .as_ref()
                .is_none_or(|b| b.tool_policy == ToolPolicy::ReadOnly)
        {
            return Ok(None);
        }
        let state = match self.git.working_state(&view.context, &view.revision) {
            Ok(Some(state)) => state,
            Ok(None) => return Ok(None),
            Err(error) => json!({"error":error}),
        };
        let mut checkpoint = record(
            "workspace_continuation_checkpoint",
            session.id.clone(),
            Scope::project(
                session
                    .project_id
                    .as_deref()
                    .ok_or_else(|| err("invalid_scope", "工作区缺少项目"))?,
            ),
            "保留同一执行的未封存工作".into(),
            1,
            None,
            json!({"issuer":"kernel_workspace_checkpoint","session_id":session.id,"turn_id":tid,"authority":seed.data["authority"]["workflow"],"agent_revision_id":session.agent,"view":view,"authorization":binding.permissions.as_ref().map(|p|&p.scope),"state":state}),
            true,
        );
        checkpoint.id = format!("workspace-checkpoint:{tid}");
        let existing = one(self.store.as_ref(), "governance", &checkpoint.id)?;
        if let Some(raw) = &existing.value {
            let saved = ConfigurationRecord::from_storage(raw.clone())?;
            require(
                saved.r#type == checkpoint.r#type
                    && saved.immutable
                    && saved.scope == checkpoint.scope
                    && saved.definition_id == checkpoint.definition_id
                    && checkpoint
                        .data
                        .as_object()
                        .unwrap()
                        .iter()
                        .all(|(k, v)| saved.data[k] == *v),
                "原工作区检查点与当前现场不同",
            )?;
            checkpoint = saved;
        }
        Ok(Some((
            checkpoint,
            ReadSet::new(vec![version(&selected), version(&existing)])?,
        )))
    }

    /// Positive startup observation plus absence of a dispatch grant. A message
    /// containing "not_applied" is not proof; only the immutable native receipt is.
    pub(crate) fn unapplied_conversation_start(
        &self,
        tid: &str,
    ) -> Result<Option<(Error, Id, ReadSet)>> {
        let seed = one(self.store.as_ref(), "conversation_open_seed", tid)?;
        if seed.value.is_none() {
            return Ok(None);
        }
        let original = ConfigurationRecord::from_storage(seed.value.clone().unwrap())?;
        let fingerprint = text(&original.data, "fingerprint")?;
        let observed = one(
            self.store.as_ref(),
            "governance",
            &format!("conversation-open-observation:{fingerprint}"),
        )?;
        let dispatch = one(
            self.store.as_ref(),
            "governance",
            &format!("conversation-dispatch:{fingerprint}"),
        )?;
        let turn = one(self.store.as_ref(), "turn", tid)?;
        let Some(raw) = &observed.value else {
            return Ok(None);
        };
        let observation = restored(self, &observed)?;
        let failure: Result<ports::SessionReceipt> =
            serde_json::from_value(observation.data["value"]["result"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        let Err(error) = failure else {
            return Ok(None);
        };
        if observation.r#type != "conversation_start_observation"
            || !observation.immutable
            || raw["definition_id"] != original.id
            || observation.data["phase"] != "open"
            || observation.data["turn_id"] != tid
            || observation.data["session_id"] != original.data["session_id"]
            || observation.data["worker_operation_id"] != original.data["worker_operation_id"]
            || dispatch.value.is_some()
            || turn.value.as_ref().is_none_or(|t| {
                t["session_id"] != original.data["session_id"]
                    || t["consumed"] == true
                    || !t["external_turn"].is_null()
            })
            || error
                .execution_failure
                .as_ref()
                .is_none_or(|f| f.effect != ExecutionFailureEffect::NotApplied)
        {
            return Ok(None);
        }
        Ok(Some((
            error,
            observation.id,
            ReadSet::new(vec![
                version(&seed),
                version(&observed),
                version(&dispatch),
                version(&turn),
            ])?,
        )))
    }

    pub(crate) fn reconcile_workspace_continuation(
        &self,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Value> {
        require(
            matches!(context.actor(), WorkflowActor::Human),
            "历史未封存现场的采用需要明确维护指令",
        )?;
        self.recover_workspace_continuation(c, fingerprint, context, None)
    }
    pub(crate) fn workspace_recovery_source(
        &self,
        execution: &StepExecution,
    ) -> Result<Option<(Id, Id)>> {
        let Some(sid) = execution.session_id.as_deref() else {
            return Ok(None);
        };
        let latest = one(self.store.as_ref(), "session_latest_turn", sid)?;
        let Some(tid) = latest.value.as_ref().and_then(|t| t["id"].as_str()) else {
            return Ok(None);
        };
        if self.unapplied_conversation_start(tid)?.is_none() {
            return Ok(None);
        }
        let selected = one(self.store.as_ref(), "workspace_continuation_source", sid)?;
        let Some(raw) = selected.value else {
            return Ok(None);
        };
        let checkpoint = ConfigurationRecord::from_storage(raw)?;
        if checkpoint.data["issuer"] != "kernel_workspace_checkpoint"
            || !checkpoint.immutable
            || checkpoint.data["authority"]["execution_id"] != execution.id
            || checkpoint.data["authority"]["contract_digest"] != execution.contract_digest
            || checkpoint.data["authority"]["input_digest"] != execution.input_digest
        {
            return Ok(None);
        }
        Ok(Some((tid.into(), text(&checkpoint.data, "turn_id")?)))
    }
    pub(crate) fn recover_workspace_continuation(
        &self,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
        authority: Option<&WorkspaceRecoveryAuthority<'_>>,
    ) -> Result<Value> {
        require(
            matches!(context.actor(), WorkflowActor::Human) || authority.is_some(),
            "缺少原生核对授权",
        )?;
        text(&c.payload, "reason")?;
        if let Some(receipt) = self.store.replay_commit(&c.command_id, fingerprint)? {
            return Ok(receipt.result);
        }
        let root = WorkflowHeaderFacts::read(
            self,
            c.target
                .as_deref()
                .ok_or_else(|| err("invalid_params", "缺少流程"))?,
        )?;
        root.active()?;
        require(
            !root.instance.paused && !root.instance.cancel_requested,
            "流程已暂停或取消",
        )?;
        let resources = root.step(self, &text(&c.payload, "execution_id")?)?;
        root.guard_request(c, Some(&resources.step))?;
        require(
            matches!(
                resources.execution.status,
                StepExecutionStatus::NeedsReconciliation | StepExecutionStatus::Failed
            ),
            "原执行不在恢复状态",
        )?;
        let tid = resources
            .execution
            .turn_id
            .as_deref()
            .or_else(|| c.payload["failed_turn_id"].as_str())
            .ok_or_else(|| err("blocked", "原执行没有可核对的未发送轮次"))?;
        let (error, proof, reads) = self
            .unapplied_conversation_start(tid)?
            .ok_or_else(|| err("blocked", "没有原启动未执行的确切证明"))?;
        let sid = resources
            .execution
            .session_id
            .as_deref()
            .ok_or_else(|| err("blocked", "原执行没有会话"))?;
        let latest = one(self.store.as_ref(), "session_latest_turn", sid)?;
        require(
            latest.value.as_ref().is_some_and(|t| t["id"] == tid),
            "原失败后已有新的轮次，不能采用旧的未发送证明",
        )?;
        let unsubmitted = if resources.execution.turn_id.is_none() {
            let key = format!(
                "workflow:{}",
                configuration_hash(&(
                    resources.execution.id.as_str(),
                    resources.execution.sequence,
                    "conversation.send"
                ))
            );
            let intent = one(self.store.as_ref(), "workflow_command_intent", &key)?;
            require(
                intent.value.as_ref().is_some_and(|r| {
                    r["data"]["method"] == "conversation.send"
                        && matches!(r["data"]["status"].as_str(), Some("prepared" | "failed"))
                        && r["data"]["result"].is_null()
                }),
                "补充输入已有未核对的派发，不能采用旧证明",
            )?;
            let client = one(
                self.store.as_ref(),
                "conversation_client",
                &json!([
                    sid,
                    format!(
                        "{}:{}",
                        resources.execution.id, resources.execution.sequence
                    )
                ])
                .to_string(),
            )?;
            require(
                client.value.is_none(),
                "该补充输入已创建实际轮次，不能沿用旧未发送证明",
            )?;
            Some((intent, client))
        } else {
            None
        };
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
        let previous = text(&c.payload, "previous_turn_id")?;
        let old = one(self.store.as_ref(), "workflow_turn_observation", &previous)?;
        let value = old
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "原完成轮次缺失"))?;
        let result: Value = serde_json::from_str(value["assistant"].as_str().unwrap_or(""))
            .map_err(|e| err("storage", e.to_string()))?;
        require(
            value["turn"]["status"] == "completed"
                && value["turn"]["session_id"] == sid
                && value["task_source"]["data"]["source"]["execution_id"] == resources.execution.id
                && result["status"] == "needs_input"
                && value["runtime"].as_array().is_some_and(|r| !r.is_empty()),
            "现场没有同一执行的已停止等待输入来源",
        )?;
        let (mut checkpoint, checkpoint_reads) = self
            .workspace_turn_checkpoint(&session, &previous)?
            .ok_or_else(|| err("blocked", "原工作区无法建立检查点"))?;
        require(
            checkpoint.data["state"]["files"].is_array(),
            "原工作区状态无法核对",
        )?;
        require(
            checkpoint.data["authority"]["execution_id"] == resources.execution.id
                && checkpoint.data["authority"]["plan_revision_id"]
                    == resources.execution.plan_revision_id
                && checkpoint.data["authority"]["input_digest"] == resources.execution.input_digest
                && checkpoint.data["authority"]["contract_digest"]
                    == resources.execution.contract_digest,
            "历史现场不属于当前冻结任务",
        )?;
        let within = |name: &str, parent: &str| {
            parent == "." || name == parent || name.starts_with(&format!("{parent}/"))
        };
        for file in checkpoint.data["state"]["files"].as_array().unwrap() {
            let name = file["path"].as_str().unwrap_or("");
            let scope = &checkpoint.data["authorization"];
            require(
                scope["writable_paths"].as_array().is_some_and(|p| {
                    p.iter()
                        .any(|p| p.as_str().is_some_and(|p| within(name, p)))
                }) && !scope["read_only_paths"].as_array().is_some_and(|p| {
                    p.iter()
                        .any(|p| p.as_str().is_some_and(|p| within(name, p)))
                }),
                "现有修改超出原执行范围，保留现场待核对",
            )?;
        }
        let fresh_checkpoint =
            checkpoint_reads.require(&ResourceKey::new("governance", &checkpoint.id))? == 0;
        require(
            authority.is_none() || !fresh_checkpoint,
            "自主核对只能使用已有的真实工作区检查点",
        )?;
        if fresh_checkpoint {
            checkpoint.data["maintenance_origin"] = json!({"command_id":c.command_id,"reason":c.payload["reason"],"actor":context.actor(),"unapplied_start":proof});
            checkpoint.content_hash = content_digest(&checkpoint.refs, &checkpoint.data);
        }
        let workers = one(self.store.as_ref(), "session_live_workers", sid)?;
        require(
            workers
                .value
                .as_ref()
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty),
            "原 worker 尚未撤销",
        )?;
        let writers = one(
            self.store.as_ref(),
            "conversation_writers",
            &resources.instance.project_id,
        )?;
        require(
            writers
                .value
                .as_ref()
                .and_then(Value::as_array)
                .is_some_and(|writers| {
                    writers.iter().all(|w| {
                        w["worktree"] != checkpoint.data["view"]["context"]["source"]["worktree"]
                    })
                }),
            "工作区仍有其他写者",
        )?;
        let mut commit = WorkflowCommit::new(self, "workflow.workspace.reconcile", c, context)?;
        if let Some(auth) = authority {
            require(
                auth.request.r#type == "workflow_reconciliation"
                    && auth.request.definition_id == resources.execution.id
                    && auth.request.data["resolution"] == "inspect"
                    && auth.attempt.r#type == "workflow_reconciliation_attempt"
                    && auth.attempt.immutable
                    && auth.attempt.definition_id == auth.request.id
                    && resources.execution.runtime_context["reconciliation_request_id"]
                        == auth.request.id
                    && auth.request.data["original_step_revision"]
                        .as_u64()
                        .is_some_and(|r| r + 1 == resources.step.revision)
                    && matches!(context.actor(),WorkflowActor::Coordinator{source_id} if source_id==&auth.request.id),
                "原经理的核对授权已变化",
            )?;
            commit.coordinator_origin = Some(
                json!({"kind":"runtime_coordinator","authorization_id":auth.request.id,"observation_id":auth.observation.resource.id}),
            );
            commit.witness(auth.basis.versions().iter().cloned())?;
            commit.witness([
                version(auth.request_snapshot),
                ResourceVersion {
                    resource: ResourceKey::new("governance", &auth.attempt.id),
                    revision: auth.attempt.revision,
                },
                auth.observation.clone(),
            ])?;
        }
        commit.require_no_change(&root.record.id)?;
        commit.witness(resources.reads.versions().iter().cloned())?;
        commit.witness(reads.versions().iter().cloned())?;
        commit.witness(checkpoint_reads.versions().iter().cloned())?;
        commit.witness([
            version(&selected),
            version(&old),
            version(&workers),
            version(&writers),
            version(&latest),
        ])?;
        if let Some((intent, client)) = &unsubmitted {
            commit.witness([version(intent), version(client)])?;
            if intent
                .value
                .as_ref()
                .is_some_and(|r| r["data"]["status"] == "prepared")
            {
                let mut row = ConfigurationRecord::from_storage(intent.value.clone().unwrap())?;
                let original = one(self.store.as_ref(), "governance", &row.id)?;
                require(
                    original.revision == intent.revision && original.value == intent.value,
                    "原补充输入意图在核对期间已变化",
                )?;
                let mut data = row.data.clone();
                data["status"] = json!("failed");
                data["error"] = json!({"code":"unsubmitted_input_reconciled","message":"原补充输入未创建轮次，已核对并保留原请求，不重放","execution_failure":{"kind":"technical","effect":"not_applied"}});
                let observed=commit.new_record(record("workflow_intent_observation",row.id.clone(),root.record.scope.clone(),"核对原补充输入未派发".into(),1,None,json!({"intent_id":row.id,"intent_revision":row.revision,"original_command_id":row.data["command"]["command_id"],"issuer":"kernel_workspace_reconciliation","value":{"status":"failed","error":data["error"]},"client_guard":version(client)}),true),"workspace-unsubmitted-input-observation")?;
                data["observation_id"] = json!(observed);
                update_record(&mut row, data);
                commit.witness([version(&original)])?;
                // Keep the already encoded immutable command byte-for-byte,
                // exactly as normal workflow-intent observation does.
                commit.change(ResourceChange {
                    resource: original.resource,
                    value: row.to_storage(),
                })?;
            }
        }
        let turn = one(self.store.as_ref(), "turn", tid)?;
        let mut next_turn = turn.value.clone().unwrap();
        next_turn["status"] = json!("failed");
        next_turn["finished"] = json!(now());
        next_turn["revision"] = json!(turn.revision + 1);
        commit.change(ResourceChange {
            resource: turn.resource,
            value: next_turn,
        })?;
        let mut next_session = selected.value.clone().unwrap();
        next_session["lifecycle"] = json!("idle");
        next_session["revision"] = json!(selected.revision + 1);
        commit.change(ResourceChange {
            resource: selected.resource,
            value: next_session,
        })?;
        if fresh_checkpoint {
            commit.change(ResourceChange {
                resource: ResourceKey::new("governance", &checkpoint.id),
                value: self.store.encode_runtime_record(&checkpoint)?,
            })?;
        }
        let mut next = resources.execution.clone();
        next.status = StepExecutionStatus::WaitingInput;
        next.failure = None;
        next.finished = None;
        let original_failure = one(self.store.as_ref(), "governance", &proof)?;
        let waiting =
            ConfigurationRecord::from_storage(original_failure.value.clone().unwrap())?.created_at;
        commit.witness([version(&original_failure)])?;
        self.start_native_workflow_wait(
            &mut commit,
            &resources,
            waiting,
            "startup_not_applied_recovery",
        )?;
        next.budget_wait_started = Some(waiting);
        self.update_native_workflow_budget_with_wait(
            &mut commit,
            &root,
            &resources,
            &mut next,
            Some(waiting),
        )?;
        require(
            next.status == StepExecutionStatus::WaitingInput,
            "扣除已证明未执行的等待后，原执行额度仍不足",
        )?;
        let stop = one(self.store.as_ref(), "workflow_stop_source", &next.id)?;
        commit.witness([version(&stop)])?;
        if let Some(raw) = &stop.value {
            let old_stop = ConfigurationRecord::from_storage(raw.clone())?;
            require(
                resources
                    .execution
                    .failure
                    .as_ref()
                    .is_some_and(|f| f.code == "execution_timeout")
                    && old_stop.data["actor"].is_null()
                    && old_stop.data["origin"]["kind"] == "runtime_coordinator"
                    && old_stop.data["origin"]["execution_id"] == next.id
                    && resources.execution.turn_id.is_none(),
                "原停止包含其他决定或副作用，不能撤回",
            )?;
            commit.new_record(record("workflow_stop_reconciliation",old_stop.id.clone(),root.record.scope.clone(),"核对未派发等待造成的超时停止".into(),1,None,json!({"issuer":"kernel_workspace_reconciliation","execution_id":next.id,"unapplied_start":proof,"wait_started":waiting,"checkpoint_id":checkpoint.id,"reason":c.payload["reason"]}),true),"workspace-timeout-stop-reconciliation")?;
        }
        next.reason = "未发送输入的启动失败已核对；原修改已保留，可由经理继续补充输入".into();
        next.runtime_context
            .as_object_mut()
            .map(|o| o.remove("reconciliation_request_id"));
        commit.step(&resources, &mut next)?;
        if let Some(auth) = authority {
            let mut data = auth.request.data.clone();
            data["status"] = json!("completed");
            data["observation_id"] = json!(auth.observation.resource.id);
            commit.replace_record(auth.request_snapshot, auth.request.clone(), data)?;
        }
        commit.new_record(record("workspace_continuation_recovery",next.id.clone(),root.record.scope.clone(),"恢复同一执行的原工作区".into(),1,None,json!({"turn_id":tid,"previous_turn_id":previous,"checkpoint_id":checkpoint.id,"unapplied_start":proof,"original_error":error,"actor":context.actor(),"reason":c.payload["reason"]}),true),"workspace-recovery")?;
        commit.finish(fingerprint,json!({"execution_id":next.id,"status":"waiting_input","checkpoint_id":checkpoint.id,"changes_preserved":true}))
    }
}
