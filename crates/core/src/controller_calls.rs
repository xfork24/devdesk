//! Native controller actions retain literal arguments and their original write
//! versions. Results and reply acknowledgements have immutable journals.
use crate::{
    concurrency::*,
    controllers::{CONTROL_TOOLS, control_query, native_reply_value, operation},
    governance::{record, update_record},
    platform_facts::{one, restored},
    workflow_commits::WorkflowHeaderFacts,
    workflow_resources::version,
    *,
};
impl Engine {
    fn execute_controller_notice(
        &self,
        selected: &ResourceSnapshot,
        call: &ConfigurationRecord,
        method: &str,
        args: &Value,
    ) -> Result<Value> {
        // Wait registration is a technical compare-and-set against an event
        // head, not a business advancement. Re-read that head and the original
        // actor/delivery when a concurrent result wins; never reuse old guards.
        // Other operations retain their literal business request semantics.
        let attempts = if method == "workflow.controller.wait" {
            3
        } else {
            1
        };
        for attempt in 0..attempts {
            let _attempt = (method == "workflow.controller.wait")
                .then(|| self.store.observe_query("controller.wait.attempt"));
            match self.execute_controller_notice_once(selected, call, method, args) {
                Err(error) if error.code == "revision_conflict" && attempt + 1 < attempts => {}
                result => return result,
            }
        }
        unreachable!("the final bounded attempt always returns")
    }
    fn execute_controller_notice_once(
        &self,
        selected: &ResourceSnapshot,
        call: &ConfigurationRecord,
        method: &str,
        args: &Value,
    ) -> Result<Value> {
        let fingerprint = configuration_hash(&(
            "controller-notice/v1",
            &call.id,
            selected.revision,
            method,
            args,
        ));
        let action = format!("{}:action", call.id);
        if let Some(receipt) = self.store.replay_commit(&action, &fingerprint)? {
            return Ok(receipt.result);
        }
        let (mut controller, authority) = self.controller_call_guards(call)?;
        require(!controller.workspace_only, "讨论工作台没有根流程控制权")?;
        controller.policy.permits("inspect")?;
        let original = one(self.store.as_ref(), "governance", &call.definition_id)?;
        require(
            authority.require(&original.resource)? == original.revision,
            "经理准备期间已变化",
        )?;
        let row = restored(self, &original)?;
        let c = Command {
            command_id: action,
            target: Some(row.id.clone()),
            expected_revision: Some(original.revision),
            payload: args.clone(),
        };
        let actor = WorkflowActor::Controller {
            controller_id: row.id.clone(),
            session_id: text(&call.data, "session_id")?,
            turn_id: text(&call.data, "turn_id")?,
            epoch: controller.epoch,
        };
        let context = InvocationContext::workflow(actor, Some(controller.project_id.clone()))
            .with_authority(authority)?;
        let mut commit = crate::workflow_commits::WorkflowCommit::new(self, method, &c, &context)?;
        commit.witness([version(selected), version(&original)])?;
        let root = controller
            .root_instance_id
            .as_deref()
            .ok_or_else(|| err("not_found", "经理没有工作流根"))?;
        if method == "workflow.controller.notify" {
            let target = args["instance_id"]
                .as_str()
                .or_else(|| args["id"].as_str())
                .unwrap_or(root);
            let instance = WorkflowHeaderFacts::read(self, target)?;
            require(
                instance.instance.root_instance_id == root
                    && args["plan_revision_id"] == instance.instance.active_plan_revision_id,
                "通知必须绑定当前根与计划",
            )?;
            commit.witness([version(&instance.header)])?;
            let interaction = self.validate_controller_interaction(
                &mut commit,
                &instance,
                &args["interaction"],
                &json!({}),
            )?;
            let id=commit.new_record(record("controller_interaction",row.id.clone(),row.scope.clone(),text(&interaction,"title")?,1,None,json!({"instance_id":target,"plan_revision_id":args["plan_revision_id"],"turn_id":call.data["turn_id"],"interaction":interaction}),true),"controller-notice")?;
            return commit.finish(&fingerprint, json!({"id":id,"status":"published"}));
        }
        require(
            controller.status == "active" && json!(controller.delivery_id) == args["delivery_id"],
            "等待必须绑定当前经理投递",
        )?;
        let summary = text(args, "summary")?;
        require(summary.len() <= 8192, "经理工作摘要过大")?;
        let waiting: Vec<String> =
            serde_json::from_value(args.get("waiting_for").cloned().unwrap_or(json!([])))
                .map_err(|e| err("invalid_params", e.to_string()))?;
        require(waiting.len() <= 32, "等待对象过多")?;
        for id in &waiting {
            // A platform step intentionally shares its ID with its platform_call
            // row. Resolve actual execution ownership first; that unrelated row
            // must neither reject the wait nor become its authorization source.
            let owner = one(self.store.as_ref(), "workflow_execution_owner", id)?;
            let instance_id = if let Some(instance) = &owner.value {
                commit.witness([version(&owner)])?;
                text(instance, "instance_id")?
            } else {
                let selected = one(self.store.as_ref(), "governance", id)?;
                if selected.value.is_none() {
                    return Err(err(
                        "invalid_params",
                        "等待对象必须是本根的流程、步骤执行或待决定记录",
                    ));
                }
                let object = restored(self, &selected)?;
                require(
                    matches!(
                        object.r#type.as_str(),
                        "workflow_instance" | "workflow_pending_decision" | "workflow_segment"
                    ),
                    "等待对象必须是本根的流程、步骤执行或待决定记录",
                )?;
                if object.r#type == "workflow_segment" {
                    require(
                        object.data["actor"]["kind"] == "controller"
                            && object.data["actor"]["controller_id"] == row.id
                            && object.data["actor"]["epoch"] == controller.epoch,
                        "等待委托必须属于当前经理授权",
                    )?;
                }
                commit.witness([version(&selected)])?;
                if object.r#type == "workflow_instance" {
                    object.id
                } else {
                    object.definition_id
                }
            };
            let instance = WorkflowHeaderFacts::read(self, &instance_id)?;
            require(
                instance.instance.root_instance_id == root,
                "等待对象不属于当前根实例",
            )?;
            commit.witness([version(&instance.header)])?;
        }
        let seconds = args["timeout_seconds"].as_u64();
        require(
            seconds.is_none_or(|s| s > 0 && s <= controller.policy.timeout_seconds),
            "等待期限超出预算",
        )?;
        let (events, page, consumed) = crate::controller_events::native_events(
            self,
            root,
            controller.source_cursor,
            Some((&row.id, controller.epoch)),
        )?;
        commit.witness([version(&page)])?;
        controller.source_cursor = consumed;
        let mut acknowledged = 0;
        for item in &events {
            if item["source"]["origin_turn_id"] != call.data["turn_id"] {
                break;
            }
            let Some(id) = item["source"]["cause_call_id"].as_str() else {
                break;
            };
            let reply = one(
                self.store.as_ref(),
                "governance",
                &format!("controller-tool-reply:{id}"),
            )?;
            commit.witness([version(&reply)])?;
            if !reply.value.as_ref().is_some_and(|r| {
                r["type"] == "controller_tool_reply"
                    && r["definition_id"] == id
                    && r["data"]["status"] == "delivered"
            }) {
                break;
            }
            controller.source_cursor = item["id"].as_u64().unwrap();
            acknowledged += 1;
        }
        controller.acknowledged_cursor = controller.source_cursor;
        let mut pending = events.len() - acknowledged;
        if pending == 0 && page.revision > controller.source_cursor {
            let (remaining, head, consumed) = crate::controller_events::native_events(
                self,
                root,
                controller.source_cursor,
                Some((&row.id, controller.epoch)),
            )?;
            commit.witness([version(&head)])?;
            controller.source_cursor = consumed;
            pending = remaining.len();
        }
        controller.run_state = if pending == 0 { "waiting" } else { "runnable" }.into();
        controller.checkpoint =
            json!({"summary":summary,"waiting_for":waiting,"turn_id":call.data["turn_id"]});
        controller.wait_deadline = seconds.map(|s| now() + s * 1000);
        controller.wait_turn_id = call.data["turn_id"].as_str().map(str::to_owned);
        let result = json!({"status":if pending==0{"waiting"}else{"pending_events"},"pending":pending,"cursor":controller.source_cursor});
        commit.replace_record(&original, row, json!(controller))?;
        commit.finish(&fingerprint, result)
    }
    fn controller_call_guards(
        &self,
        call: &ConfigurationRecord,
    ) -> Result<(WorkflowController, ReadSet)> {
        let selected = one(self.store.as_ref(), "governance", &call.definition_id)?;
        let row = restored(self, &selected)?;
        let controller: WorkflowController = row.decode()?;
        require(
            row.r#type == "workflow_controller"
                && controller.status == "active"
                && call.data["epoch"] == controller.epoch
                && controller.session_id.as_deref() == call.data["session_id"].as_str(),
            "原经理资格或代次已撤销",
        )?;
        let lease = one(
            self.store.as_ref(),
            "controller_lease_active",
            &json!([row.id, controller.epoch]).to_string(),
        )?;
        require(lease.revision == 1, "原经理租约已过期")?;
        let (current, instance) = self.controller_instance_current(&controller)?;
        require(current, "原经理 Agent 实例已停用或换绑")?;
        let native = self.native_worker_guards(
            &text(&call.data, "session_id")?,
            &text(&call.data, "turn_id")?,
            &text(&call.data, "worker_operation_id")?,
        )?;
        Ok((
            controller,
            native
                .merge([version(&selected), version(&lease)])?
                .merge(instance.versions().iter().cloned())?,
        ))
    }
    pub(crate) fn drive_native_controller_calls(&self) -> Result<()> {
        self.drive_controller_actions()?;
        self.reply_native_controller_calls()
    }
    /// Drive durable action results independently of reply delivery, including
    /// recovery after an action committed before the provider was notified.
    pub fn drive_controller_actions(&self) -> Result<()> {
        let began = std::time::Instant::now();
        for (visited, selected) in self
            .pending_runtime_records("controller_tool_call", "prepared")?
            .into_iter()
            .enumerate()
        {
            if visited > 0 && began.elapsed() >= std::time::Duration::from_millis(25) {
                break;
            }
            let call = restored(self, &selected)?;
            let result = self.execute_native_controller_action(&selected, &call);
            if result.as_ref().is_err_and(|e| {
                matches!(e.code.as_str(), "storage_busy" | "material_seal_unresolved")
            }) {
                continue;
            }
            let key = ResourceKey::new(
                "governance",
                format!("controller-action-observation:{}", call.id),
            );
            let _admission = self
                .store
                .observe_native_call(&call.id, "native.manager.result");
            let mut observed = record(
                "controller_action_observation",
                call.id.clone(),
                call.scope.clone(),
                "原经理工具结果".into(),
                1,
                None,
                json!({"call_version":version(&selected),"result":result,"issuer":"kernel_controller"}),
                true,
            );
            observed.id = key.id.clone();
            self.store.commit_changes(&PreparedChangeSet {
                command_id: key.id.clone(),
                fingerprint: configuration_hash(&observed.data),
                read_set: ReadSet::new(vec![ResourceVersion {
                    resource: key.clone(),
                    revision: 0,
                }])?,
                changes: vec![ResourceChange {
                    resource: key.clone(),
                    value: self.store.encode_runtime_record(&observed)?,
                }],
                result: json!({"id":observed.id}),
                time: now() as i64,
            })?;
            let mut data = call.data.clone();
            match result {
                Ok(value) => {
                    data["status"] = json!("succeeded");
                    data["receipt"] = value;
                }
                Err(error) => {
                    data["status"] = json!("failed");
                    data["error"] = json!(error);
                }
            }
            data["observation_id"] = json!(observed.id);
            let mut next = call.clone();
            update_record(&mut next, data);
            let outcome = self.store.commit_changes(&PreparedChangeSet {
                command_id: format!("controller-action-admit:{}", call.id),
                fingerprint: configuration_hash(&(
                    "controller-action-admit/v1",
                    &call.id,
                    &observed.id,
                )),
                read_set: ReadSet::new(vec![
                    version(&selected),
                    ResourceVersion {
                        resource: key,
                        revision: 1,
                    },
                ])?,
                changes: vec![ResourceChange {
                    resource: selected.resource,
                    value: self.store.encode_runtime_record(&next)?,
                }],
                result: json!({"id":call.id}),
                time: now() as i64,
            });
            if let Err(error) = outcome
                && !matches!(error.code.as_str(), "revision_conflict" | "storage_busy")
            {
                return Err(error);
            }
        }
        Ok(())
    }
    fn execute_native_controller_action(
        &self,
        selected: &ResourceSnapshot,
        call: &ConfigurationRecord,
    ) -> Result<Value> {
        let action = format!("{}:action", call.id);
        let _action = self
            .store
            .observe_native_call(&call.id, "native.manager.action");
        let method = text(&call.data, "method")?;
        let args = &call.data["arguments"];
        if matches!(
            method.as_str(),
            "workflow.controller.wait" | "workflow.controller.notify"
        ) {
            return self.execute_controller_notice(selected, call, &method, args);
        }
        let actor = WorkflowActor::Controller {
            controller_id: call.definition_id.clone(),
            session_id: text(&call.data, "session_id")?,
            turn_id: text(&call.data, "turn_id")?,
            epoch: call.data["epoch"]
                .as_u64()
                .ok_or_else(|| err("storage", "原代次缺失"))?,
        };
        let observation = one(
            self.store.as_ref(),
            "governance",
            &format!("controller-action-observation:{}", call.id),
        )?;
        if let Some(raw) = observation.value {
            require(
                raw["type"] == "controller_action_observation"
                    && raw["definition_id"] == call.id
                    && raw["data"]["call_version"] == json!(version(selected)),
                "原调用结果身份不符",
            )?;
            return serde_json::from_value::<Result<Value>>(
                self.store.restore_runtime_record(&raw)?["data"]["result"].clone(),
            )
            .map_err(|e| err("storage", e.to_string()))?;
        }
        let grant_key =
            ResourceKey::new("governance", format!("controller-action-grant:{}", call.id));
        let previous = one(self.store.as_ref(), "governance", &grant_key.id)?;
        // A successful business commit can be read after its caller ended.
        if let Some(raw) = &previous.value {
            let grant = restored(self, &previous)?;
            if grant.data["command"].is_object() {
                let c: Command = serde_json::from_value(grant.data["command"].clone())
                    .map_err(|e| err("storage", e.to_string()))?;
                if let Some(receipt) = self
                    .store
                    .replay_commit(&c.command_id, &command_fingerprint(&method, &c))?
                {
                    return Ok(receipt.result);
                }
            }
            require(
                raw["type"] == "controller_action_grant"
                    && grant.definition_id == call.id
                    && grant.data["method"] == method
                    && grant.data["arguments"] == *args,
                "原工具授权不能换参数",
            )?;
        }
        let (controller, authority) = self.controller_call_guards(call)?;
        if MANAGER_TOOLS.contains(&method.as_str()) {
            return self.manager_tool(&controller, call, &method, args);
        }
        require(
            !controller.workspace_only && CONTROL_TOOLS.contains(&method.as_str()),
            "经理未声明此编排工具",
        )?;
        controller.policy.permits(operation(&method))?;
        let root = controller.root_instance_id.as_deref();
        let target = args["instance_id"]
            .as_str()
            .or_else(|| args["id"].as_str())
            .or(root);
        if control_query(&method) {
            if root.is_none() && method == "workflow.inspect" {
                let catalog = self.store.configuration_snapshot()?;
                let owner = crate::work_owners::WorkOwnerFacts::read(
                    self.store.as_ref(),
                    &controller.owner_id,
                )?;
                return Ok(
                    json!({"owner_id":controller.owner_id,"owner_revision":owner.owner.revision,"configuration":catalog.state.governance.iter().filter(|r|r.r#type=="project_configuration" && r.definition_id==controller.project_id).max_by_key(|r|r.version),"allowed_actions":["start"]}),
                );
            }
            let target = target.ok_or_else(|| err("not_found", "尚未启动流程"))?;
            let source = one(self.store.as_ref(), "workflow_run_authority", target)?;
            require(
                source.value.as_ref().is_some_and(|v| {
                    v["project_id"] == controller.project_id
                        && serde_json::from_value::<Scope>(v["scope"].clone()).is_ok_and(|scope| {
                            scope.project_id() == Some(controller.project_id.as_str())
                        })
                        && v["root_instance_id"].as_str() == root
                }),
                "原查询跨流程根",
            )?;
            let mut params = args.clone();
            params["id"] = json!(target);
            params["project_id"] = json!(controller.project_id);
            let mut result = self
                .workflow_control_query(&method, &params)
                .ok_or_else(|| err("method_not_found", "未知编排查询"))??;
            if method == "workflow.inspect" && params["decision_ref"].is_null() {
                for execution in result["runtime"]["steps"]
                    .as_array_mut()
                    .into_iter()
                    .flatten()
                {
                    execution.as_object_mut().unwrap().remove("inputs");
                }
                for step in result["steps"].as_array_mut().into_iter().flatten() {
                    step["execution"].as_object_mut().unwrap().remove("inputs");
                }
                result["input_parts"] = json!("workflow.execution.inspect: part=inputs");
            }
            if method == "workflow.change.preview" {
                result.as_object_mut().unwrap().remove("document");
            }
            require(
                result.to_string().len() <= 256 * 1024,
                "查询结果过大，请使用分页或分块读取",
            )?;
            return Ok(result);
        }
        let target = if method == "workflow.start" {
            controller.owner_id.clone()
        } else {
            let id = target.ok_or_else(|| err("not_found", "尚未启动流程"))?;
            let actual = WorkflowHeaderFacts::read(self, id)?;
            require(
                Some(actual.instance.root_instance_id.as_str()) == root,
                "原控制命令跨流程根",
            )?;
            id.into()
        };
        let command = if previous.value.is_some() {
            serde_json::from_value(restored(self, &previous)?.data["command"].clone())
                .map_err(|e| err("storage", e.to_string()))?
        } else {
            let expected = args["expected_revision"].as_u64().ok_or_else(|| {
                err(
                    "precondition_required",
                    "工具写入必须携带读取时的原资源版本",
                )
            })?;
            let c = Command {
                command_id: action,
                target: Some(target),
                expected_revision: Some(expected),
                payload: args.clone(),
            };
            let mut grant = record(
                "controller_action_grant",
                call.id.clone(),
                call.scope.clone(),
                "原生经理命令的固定版本".into(),
                1,
                None,
                json!({"method":method,"arguments":args,"command":c,"call_version":version(selected),"authority":authority,"actor":actor}),
                true,
            );
            grant.id = grant_key.id.clone();
            self.store.commit_changes(&PreparedChangeSet {
                command_id: grant_key.id.clone(),
                fingerprint: configuration_hash(&grant.data),
                read_set: authority.merge([version(selected), version(&previous)])?,
                changes: vec![ResourceChange {
                    resource: grant_key,
                    value: self.store.encode_runtime_record(&grant)?,
                }],
                result: json!({"id":grant.id}),
                time: now() as i64,
            })?;
            c
        };
        let original = restored(
            self,
            &one(
                self.store.as_ref(),
                "governance",
                &format!("controller-action-grant:{}", call.id),
            )?,
        )?;
        let original_basis: ReadSet = serde_json::from_value(original.data["authority"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        // The command repeats live authorization and all domain witnesses. Its
        // original native caller lease cannot be replaced by a newer worker.
        let keys = original_basis
            .versions()
            .iter()
            .map(|v| v.resource.clone())
            .collect::<Vec<_>>();
        let actual = self.store.read_resources(&keys)?;
        require(
            actual
                .iter()
                .map(version)
                .eq(original_basis.versions().iter().cloned()),
            "原经理执行资格在准备期间变化",
        )?;
        let frozen = original_basis.merge([
            ResourceVersion {
                resource: ResourceKey::new("governance", &original.id),
                revision: original.revision,
            },
            version(selected),
        ])?;
        self.execute_in_context(
            &method,
            command,
            &InvocationContext::workflow(actor, Some(controller.project_id))
                .with_authority(frozen)?,
        )
    }
    fn reply_native_controller_calls(&self) -> Result<()> {
        let began = std::time::Instant::now();
        for (visited, work) in self
            .pending_runtime_records("controller_reply_work", "pending")?
            .into_iter()
            .enumerate()
        {
            if visited > 0 && began.elapsed() >= std::time::Duration::from_millis(25) {
                break;
            }
            let work_row = restored(self, &work)?;
            let selected = one(self.store.as_ref(), "governance", &work_row.definition_id)?;
            let call = restored(self, &selected)?;
            if !matches!(call.data["status"].as_str(), Some("succeeded" | "failed")) {
                continue;
            }
            let _delivery = self
                .store
                .observe_native_call(&call.id, "native.manager.delivery");
            let sid = text(&call.data, "session_id")?;
            let session = one(self.store.as_ref(), "session", &sid)?;
            let actual: Session = serde_json::from_value(
                self.store.restore_runtime_value(
                    session
                        .value
                        .as_ref()
                        .ok_or_else(|| err("not_found", "回复会话缺失"))?,
                )?,
            )
            .map_err(|e| err("storage", e.to_string()))?;
            let active = self
                .native_worker_guards(
                    &sid,
                    &text(&call.data, "turn_id")?,
                    &text(&call.data, "worker_operation_id")?,
                )
                .ok();
            let success = call.data["status"] == "succeeded";
            let value = if success {
                native_reply_value(
                    call.data["method"].as_str().unwrap_or(""),
                    &call.id,
                    call.data["receipt"].clone(),
                )
            } else {
                call.data["error"].clone()
            };
            let receipt = ToolReceipt {
                invocation_id: call.id.clone(),
                success,
                content: if success {
                    vec![]
                } else {
                    vec![json!({"type":"text","text":value.to_string()})]
                },
                structured_content: success.then_some(value.clone()),
                remote_effect: if success { "applied" } else { "not_applied" }.into(),
                error: (!success).then_some(value),
                origin: Origin::External,
            };
            let key = ResourceKey::new("governance", format!("controller-reply-grant:{}", call.id));
            let previous = one(self.store.as_ref(), "governance", &key.id)?;
            let mut grant = record(
                "controller_reply_grant",
                call.id.clone(),
                call.scope.clone(),
                "经理原生回复资格".into(),
                1,
                None,
                json!({"receipt":receipt,"request_id":call.data["request_id"],"call_version":version(&selected),"issuer":"kernel_controller"}),
                true,
            );
            grant.id = key.id.clone();
            let fresh = if previous.value.is_some() {
                grant = restored(self, &previous)?;
                require(
                    grant.data["receipt"] == json!(receipt)
                        && grant.data["call_version"] == json!(version(&selected)),
                    "原回复不能换结果或版本",
                )?;
                false
            } else {
                let mut reads = ReadSet::new(vec![
                    version(&selected),
                    version(&previous),
                    version(&session),
                ])?;
                if let Some(active) = &active {
                    reads = reads.merge(active.versions().iter().cloned())?;
                }
                self.store
                    .submit_changes(&PreparedChangeSet {
                        command_id: key.id.clone(),
                        fingerprint: configuration_hash(&grant.data),
                        read_set: reads,
                        changes: vec![ResourceChange {
                            resource: key,
                            value: grant.to_storage(),
                        }],
                        result: json!({"id":grant.id}),
                        time: now() as i64,
                    })?
                    .applied
            };
            let observed = if fresh && active.is_some() {
                let result = self
                    .harness
                    .answer_tool_call(&actual, &call.data["request_id"], &receipt)
                    .map(|_| Some(json!({"delivered":true})));
                Some((self.platform_observe(&grant, "reply", &result)?, result))
            } else {
                self.platform_observed(&grant, "reply")?
            };
            let status = if let Some((_, result)) = &observed {
                if result.is_ok() {
                    "delivered"
                } else {
                    "needs_reconciliation"
                }
            } else if active.is_none() {
                if fresh {
                    "worker_unavailable"
                } else {
                    "worker_unavailable_outcome_unknown"
                }
            } else {
                continue;
            };
            let mut reply = record(
                "controller_tool_reply",
                call.id.clone(),
                call.scope.clone(),
                "经理原生回复事实".into(),
                1,
                None,
                json!({"status":status,"grant_id":grant.id,"receipt":receipt,"issuer":"kernel_controller"}),
                true,
            );
            reply.id = format!("controller-tool-reply:{}", call.id);
            let reply_key = ResourceKey::new("governance", &reply.id);
            let mut done = work_row.clone();
            let mut data = done.data.clone();
            data["status"] = json!("completed");
            data["reply_id"] = json!(reply.id);
            update_record(&mut done, data);
            let mut reads = ReadSet::new(vec![
                version(&work),
                ResourceVersion {
                    resource: ResourceKey::new("governance", &grant.id),
                    revision: 1,
                },
                ResourceVersion {
                    resource: reply_key.clone(),
                    revision: 0,
                },
            ])?;
            if let Some((observed, _)) = observed {
                reads = reads.merge([observed])?;
            }
            self.store.commit_changes(&PreparedChangeSet {
                command_id: format!("controller-reply-finish:{}", call.id),
                fingerprint: configuration_hash(&reply.data),
                read_set: reads,
                changes: vec![
                    ResourceChange {
                        resource: reply_key,
                        value: reply.to_storage(),
                    },
                    ResourceChange {
                        resource: work.resource,
                        value: self.store.encode_runtime_record(&done)?,
                    },
                ],
                result: json!({"id":reply.id}),
                time: now() as i64,
            })?;
        }
        Ok(())
    }
}
