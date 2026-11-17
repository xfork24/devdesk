//! Kernel platform replies have a separate once-only delivery grant and raw
//! acknowledgement. A missing acknowledgement never grants another send.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    platform_facts::{one, restored},
    workflow_resources::version,
    *,
};

impl Engine {
    pub(crate) fn reply_native_platform_calls(&self) -> Result<()> {
        let started = std::time::Instant::now();
        let mut visited = 0;
        for work in self.pending_runtime_records("platform_reply_work", "pending")? {
            let work_record = restored(self, &work)?;
            let selected = one(
                self.store.as_ref(),
                "governance",
                &work_record.definition_id,
            )?;
            if selected.value.is_none() {
                self.project_failed_platform_dispatch((&work_record, &work), &selected)?;
                continue;
            }
            if !selected.value.as_ref().is_some_and(|r| {
                matches!(
                    r["data"]["status"].as_str(),
                    Some("succeeded" | "failed" | "cancelled" | "needs_reconciliation")
                )
            }) {
                continue;
            }
            if visited > 0 && started.elapsed() >= std::time::Duration::from_millis(25) {
                return Ok(());
            }
            visited += 1;
            let call = restored(self, &selected)?;
            let request: PlatformToolRequest = serde_json::from_value(call.data["request"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            let Some(caller) = &request.caller else {
                continue;
            };
            let done = one(
                self.store.as_ref(),
                "governance",
                &format!("platform-tool-reply:{}", call.id),
            )?;
            if done.value.is_some() {
                continue;
            }
            let session = one(self.store.as_ref(), "session", &caller.session_id)?;
            let actual: Session = serde_json::from_value(
                self.store.restore_runtime_value(
                    session
                        .value
                        .as_ref()
                        .ok_or_else(|| err("not_found", "平台回复原会话缺失"))?,
                )?,
            )
            .map_err(|e| err("storage", e.to_string()))?;
            let active = match self.platform_caller_guards(caller) {
                Ok(reads) => Some(reads),
                Err(error) if error.code == "revision_conflict" => None,
                Err(error) => return Err(error),
            };
            let success = call.data["status"] == "succeeded";
            let receipt = ToolReceipt {
                invocation_id: call.id.clone(),
                success,
                content: vec![
                    json!({"type":"text","text":if success{call.data["receipt"].to_string()}else{call.data["error"].to_string()}}),
                ],
                structured_content: success.then(|| call.data["receipt"].clone()),
                remote_effect: call.data["remote_effect"]
                    .as_str()
                    .unwrap_or("unknown")
                    .into(),
                error: (!success)
                    .then(|| json!({"issuer":"kernel_platform","error":call.data["error"]})),
                origin: Origin::External,
            };
            let key = ResourceKey::new("governance", format!("platform-reply-intent:{}", call.id));
            let previous = one(self.store.as_ref(), "governance", &key.id)?;
            let mut intent = record(
                "platform_reply_intent",
                call.id.clone(),
                call.scope.clone(),
                "交付内核平台回执".into(),
                1,
                None,
                json!({"status":"granted","call_version":version(&selected),"session_id":caller.session_id,"turn_id":caller.turn_id,"worker_operation_id":caller.operation_id,"request_id":caller.request_id,"receipt":receipt,"issuer":"kernel_platform"}),
                true,
            );
            intent.id = key.id.clone();
            let mut observations = vec![];
            if previous.value.is_some() {
                intent = restored(self, &previous)?;
                require(
                    intent.r#type == "platform_reply_intent"
                        && intent.definition_id == call.id
                        && intent.data["request_id"] == caller.request_id
                        && intent.data["receipt"] == json!(receipt),
                    "平台回复不能更换原请求或回执",
                )?;
                if let Some((observed, result)) = self.platform_observed(&intent, "reply")? {
                    observations.push(observed);
                    self.finish_platform_reply(
                        (&work_record, &work),
                        &call,
                        &done,
                        &intent,
                        observations,
                        if result.is_ok() {
                            "delivered"
                        } else {
                            "needs_reconciliation"
                        },
                    )?;
                } else if active.is_none() {
                    self.finish_platform_reply(
                        (&work_record, &work),
                        &call,
                        &done,
                        &intent,
                        vec![],
                        "worker_unavailable_outcome_unknown",
                    )?;
                }
                continue;
            }
            let mut reads = ReadSet::new(vec![
                version(&selected),
                version(&previous),
                version(&session),
            ])?;
            if let Some(active) = &active {
                reads = reads.merge(active.versions().iter().cloned())?;
            }
            let outcome = self.store.submit_changes(&PreparedChangeSet {
                command_id: key.id.clone(),
                fingerprint: configuration_hash(&intent.data),
                read_set: reads,
                changes: vec![ResourceChange {
                    resource: key,
                    value: intent.to_storage(),
                }],
                result: json!({"id":intent.id}),
                time: now() as i64,
            })?;
            if !outcome.applied {
                continue;
            }
            if active.is_none() {
                self.finish_platform_reply(
                    (&work_record, &work),
                    &call,
                    &done,
                    &intent,
                    vec![],
                    "worker_unavailable",
                )?;
                continue;
            }
            let result = self
                .harness
                .answer_tool_call(&actual, &caller.request_id, &receipt)
                .map(|_| Some(json!({"delivered":true})));
            let observed = self.platform_observe(&intent, "reply", &result)?;
            self.finish_platform_reply(
                (&work_record, &work),
                &call,
                &done,
                &intent,
                vec![observed],
                if result.is_ok() {
                    "delivered"
                } else {
                    "needs_reconciliation"
                },
            )?;
        }
        Ok(())
    }
    fn project_failed_platform_dispatch(
        &self,
        (work_record, work): (&ConfigurationRecord, &ResourceSnapshot),
        call: &ResourceSnapshot,
    ) -> Result<()> {
        let intent = one(
            self.store.as_ref(),
            "governance",
            &format!("agent-platform-intent:{}", work_record.definition_id),
        )?;
        let Some(raw) = &intent.value else {
            return Ok(());
        };
        if raw["data"]["status"] != "failed" {
            return Ok(());
        }
        require(
            raw["type"] == "workflow_intent"
                && raw["data"]["command"]["payload"]["agent_platform_call_id"]
                    == work_record.definition_id,
            "平台派发失败不能替换原请求来源",
        )?;
        let mut row = record(
            "platform_call",
            work_record.definition_id.clone(),
            work_record.scope.clone(),
            "平台原派发失败".into(),
            1,
            None,
            json!({"status":"failed","request":work_record.data["request"],"remote_effect":"not_applied","issuer":"kernel_platform","error":{"code":"dispatch_failed","message":raw["data"]["error"]}}),
            false,
        );
        row.id = call.resource.id.clone();
        self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("platform-dispatch-failed:{}", row.id),
            fingerprint: configuration_hash(&row.data),
            read_set: ReadSet::new(vec![version(work), version(call), version(&intent)])?,
            changes: vec![ResourceChange {
                resource: call.resource.clone(),
                value: self.store.encode_runtime_record(&row)?,
            }],
            result: json!({"id":row.id}),
            time: now() as i64,
        })?;
        Ok(())
    }
    fn finish_platform_reply(
        &self,
        (work_record, work): (&ConfigurationRecord, &ResourceSnapshot),
        call: &ConfigurationRecord,
        done: &ResourceSnapshot,
        intent: &ConfigurationRecord,
        observations: Vec<ResourceVersion>,
        status: &str,
    ) -> Result<()> {
        let mut reply = record(
            "platform_tool_reply",
            call.id.clone(),
            call.scope.clone(),
            "原生平台工具交付结果".into(),
            1,
            None,
            json!({"status":status,"receipt":intent.data["receipt"],"intent_id":intent.id,"issuer":"kernel_platform"}),
            true,
        );
        reply.id = done.resource.id.clone();
        let mut completed = work_record.clone();
        let mut data = completed.data.clone();
        data["status"] = json!("completed");
        data["reply_id"] = json!(reply.id);
        update_record(&mut completed, data);
        self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("platform-reply-finish:{}", call.id),
            fingerprint: configuration_hash(&("platform-reply-finish/v1", &intent.id, status)),
            read_set: ReadSet::new(vec![
                version(work),
                version(done),
                ResourceVersion {
                    resource: ResourceKey::new("governance", &intent.id),
                    revision: intent.revision,
                },
            ])?
            .merge(observations)?,
            changes: vec![
                ResourceChange {
                    resource: done.resource.clone(),
                    value: reply.to_storage(),
                },
                ResourceChange {
                    resource: work.resource.clone(),
                    value: self.store.encode_runtime_record(&completed)?,
                },
            ],
            result: json!({"id":reply.id}),
            time: now() as i64,
        })?;
        Ok(())
    }
}
