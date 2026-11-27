//! Resource-owned read intents and once-admitted native context replies.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    *,
};

pub struct ContextCallFacts {
    pub row: ResourceSnapshot,
    pub session: Session,
    pub caller_guards: Vec<ResourceVersion>,
    pub active: bool,
    pub suspended: bool,
}
struct ContextCall {
    row: ConfigurationRecord,
    version: ResourceVersion,
    session: Session,
    guards: Vec<ResourceVersion>,
    active: bool,
    suspended: bool,
}
impl ContextCall {
    fn read(engine: &Engine, id: &str) -> Result<Self> {
        let facts = engine.store.context_call_facts(id)?;
        require(
            facts.row.resource == ResourceKey::new("governance", id),
            "上下文调用读取身份不符",
        )?;
        let row: ConfigurationRecord = serde_json::from_value(
            engine.store.restore_runtime_record(
                facts
                    .row
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "上下文调用缺失"))?,
            )?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        let invocation = &row.data["invocation"];
        let configuration = facts
            .session
            .runtime_binding
            .as_ref()
            .and_then(|b| b.agent_configuration.as_ref())
            .ok_or_else(|| err("blocked", "上下文调用缺少原 Agent 锁"))?;
        require(
            row.r#type == "agent_context_call"
                && invocation["id"] == id
                && invocation["session_id"] == facts.session.id
                && invocation["agent_instance_id"] == configuration.agent_instance_id
                && invocation["agent_revision_id"] == configuration.agent_revision_id,
            "上下文调用改变了原 Agent 或会话",
        )?;
        let reference = text(&invocation["tool"], "revision_id")?;
        let definition: ToolDefinition = serde_json::from_value(row.data["definition"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let allowed = if reference == AGENT_CONTEXT_PROTOCOL {
            configuration.context_protocol.as_deref() == Some(AGENT_CONTEXT_PROTOCOL)
        } else {
            configuration.integration_lock.as_ref().is_some_and(|l| {
                l.platform_tools.iter().any(|t| t.id == reference)
                    || l.domain_tools.iter().any(|t| t.id == reference)
                    || (definition.implementation == "unbound"
                        && !row.data["error"].is_null()
                        && l.unbound_tools.iter().any(|t| t.id == reference))
            })
        };
        require(allowed, "上下文工具不属于原冻结 Capability")?;
        let version = ResourceVersion {
            resource: facts.row.resource,
            revision: facts.row.revision,
        };
        Ok(Self {
            row,
            version,
            session: facts.session,
            guards: facts.caller_guards,
            active: facts.active,
            suspended: facts.suspended,
        })
    }
    fn next(&self, data: Value) -> ConfigurationRecord {
        let mut row = self.row.clone();
        update_record(&mut row, data);
        row
    }
    fn commit(
        &self,
        engine: &Engine,
        phase: &str,
        next: &ConfigurationRecord,
        extra: Vec<ResourceChange>,
        mut guards: Vec<ResourceVersion>,
    ) -> Result<CommitOutcome> {
        guards.push(self.version.clone());
        let mut changes = vec![ResourceChange {
            resource: self.version.resource.clone(),
            value: engine.store.encode_runtime_record(next)?,
        }];
        let semantic = |row: &Value| {
            let mut value = row.clone();
            if let Some(object) = value.as_object_mut() {
                object.remove("created_at");
                object.remove("content_hash");
                if let Some(data) = object.get_mut("data").and_then(Value::as_object_mut) {
                    data.remove("finished_at");
                    data.remove("caller_guards");
                }
            }
            value
        };
        let fingerprint = configuration_hash(&(
            "context-call/v1",
            &self.version,
            phase,
            semantic(&next.to_storage()),
            extra
                .iter()
                .map(|c| (&c.resource, semantic(&c.value)))
                .collect::<Vec<_>>(),
        ));
        changes.extend(extra);
        engine.store.submit_changes(&PreparedChangeSet {
            command_id: format!("context-{phase}:{}:{}", self.row.id, self.version.revision),
            fingerprint,
            read_set: ReadSet::new(guards)?,
            changes,
            result: json!({"id":self.row.id}),
            time: now() as i64,
        })
    }
    fn observe(&self, engine: &Engine, phase: &str, result: &Value) -> Result<ResourceVersion> {
        let digest = configuration_hash(&("context-observe/v1", &self.version, phase, result));
        let key = ResourceKey::new("governance", format!("context-observation:{digest}"));
        let mut row = record(
            "context_call_observation",
            self.row.id.clone(),
            self.row.scope.clone(),
            phase.into(),
            1,
            None,
            json!({"call_version":self.version,"phase":phase,"value":{"result":result}}),
            true,
        );
        row.id = key.id.clone();
        let receipt = engine.store.commit_changes(&PreparedChangeSet {
            command_id: format!("context-journal:{digest}"),
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
            .ok_or_else(|| err("storage", "上下文观察缺少提交版本"))
    }
    fn receive(
        &self,
        engine: &Engine,
        observed: &ResourceVersion,
        result: &Result<Value>,
    ) -> Result<()> {
        let mut data = self.row.data.clone();
        data["status"] = json!(if result.is_ok() {
            "succeeded"
        } else {
            "failed"
        });
        data["result"] = result.as_ref().ok().cloned().unwrap_or(Value::Null);
        data["error"] = json!(result.as_ref().err());
        data["observation_id"] = json!(observed.resource.id);
        data["finished_at"] = json!(now());
        self.commit(
            engine,
            "read-admit",
            &self.next(data),
            vec![],
            vec![observed.clone()],
        )?;
        Ok(())
    }
    fn execute(&self, engine: &Engine) -> Result<()> {
        if self.row.data["status"] != "prepared" || self.suspended {
            return Ok(());
        }
        if !self.active {
            let mut data = self.row.data.clone();
            data["status"] = json!("failed");
            data["error"] = json!(err("cancelled", "原上下文调用资格已撤销"));
            data["issuer"] = json!("kernel_context");
            self.commit(
                engine,
                "read-denied",
                &self.next(data),
                vec![],
                self.guards.clone(),
            )?;
            return Ok(());
        }
        let key = ResourceKey::new("governance", format!("context-dispatch:{}", self.row.id));
        let mut intent = record(
            "context_call_dispatch",
            self.row.id.clone(),
            self.row.scope.clone(),
            "读取上下文意图".into(),
            1,
            None,
            json!({"call_version":self.version,"invocation_digest":configuration_hash(&self.row.data["invocation"]),"caller_guards":self.guards,"issuer":"kernel_context"}),
            true,
        );
        intent.id = key.id.clone();
        let mut guards = self.guards.clone();
        guards.push(ResourceVersion {
            resource: key.clone(),
            revision: 0,
        });
        let mut data = self.row.data.clone();
        data["status"] = json!("running");
        data["dispatch_id"] = json!(key.id);
        let next = self.next(data);
        let outcome = self.commit(
            engine,
            "read-prepare",
            &next,
            vec![ResourceChange {
                resource: key,
                value: intent.to_storage(),
            }],
            guards,
        )?;
        if !outcome.applied {
            return Ok(());
        }
        let running = Self {
            row: next,
            version: ResourceVersion {
                resource: self.version.resource.clone(),
                revision: self.version.revision + 1,
            },
            session: self.session.clone(),
            guards: self.guards.clone(),
            active: self.active,
            suspended: self.suspended,
        };
        let result = engine
            .store
            .configuration_snapshot()
            .map_err(|e| e.technical_not_applied())
            .and_then(|catalog| {
                engine.execute_context_call(&catalog, &running.row, &running.session)
            });
        let observed = running.observe(engine, "read", &json!(result))?;
        running.receive(engine, &observed, &result)
    }
    fn recover_observation(&self, engine: &Engine) -> Result<()> {
        if self.row.data["status"] != "running" {
            return Ok(());
        }
        let key = ResourceKey::new(
            "context_observation_head",
            json!([self.row.id, self.version.revision, "read"]).to_string(),
        );
        let rows = engine.store.read_resources(std::slice::from_ref(&key))?;
        require(rows.len() == 1, "上下文观察读取不完整")?;
        let Some(value) = rows[0].value.as_ref() else {
            return Ok(());
        };
        let observed = ResourceVersion {
            resource: ResourceKey::new("governance", text(value, "id")?),
            revision: value["revision"]
                .as_u64()
                .ok_or_else(|| err("storage", "原观察缺少版本"))?,
        };
        require(
            value["type"] == "context_call_observation"
                && value["data"]["call_version"] == json!(self.version)
                && value["data"]["phase"] == "read",
            "上下文观察改变了原请求版本",
        )?;
        let result: Result<Value> =
            serde_json::from_value(value["data"]["value"]["result"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        self.receive(engine, &observed, &result)
    }
    fn reply(&self, engine: &Engine) -> Result<()> {
        if self.suspended
            || self.row.data["reply_sent"] == true
            || matches!(
                self.row.data["status"].as_str(),
                Some("prepared" | "running")
            )
        {
            return Ok(());
        }
        let key = ResourceKey::new("governance", format!("context-reply:{}", self.row.id));
        let rows = engine.store.read_resources(std::slice::from_ref(&key))?;
        require(rows.len() == 1, "上下文回复意图读取不完整")?;
        if rows[0].value.is_some() {
            let observation_key = ResourceKey::new(
                "context_observation_head",
                json!([self.row.id, self.version.revision, "reply"]).to_string(),
            );
            let observations = engine.store.read_resources(&[observation_key])?;
            require(observations.len() == 1, "原生回复观察读取不完整")?;
            if let Some(value) = observations[0].value.as_ref() {
                require(
                    value["type"] == "context_call_observation"
                        && value["data"]["phase"] == "reply"
                        && value["data"]["call_version"] == json!(self.version),
                    "回复观察改变了原调用版本",
                )?;
                let result: Result<()> =
                    serde_json::from_value(value["data"]["value"]["result"].clone())
                        .map_err(|e| err("storage", e.to_string()))?;
                let observed = ResourceVersion {
                    resource: ResourceKey::new("governance", text(value, "id")?),
                    revision: value["revision"]
                        .as_u64()
                        .ok_or_else(|| err("storage", "回复观察版本缺失"))?,
                };
                let mut data = self.row.data.clone();
                data["reply_sent"] = json!(result.is_ok());
                data["reply_status"] = json!(if result.is_ok() {
                    "delivered"
                } else {
                    "needs_reconciliation"
                });
                data["reply_observation_id"] = json!(observed.resource.id);
                self.commit(
                    engine,
                    "reply-admit",
                    &self.next(data),
                    vec![],
                    vec![observed],
                )?;
            } else if !self.active {
                let mut data = self.row.data.clone();
                data["reply_sent"] = json!(true);
                data["reply_status"] = json!("worker_unavailable_outcome_unknown");
                self.commit(
                    engine,
                    "reply-closed",
                    &self.next(data),
                    vec![],
                    self.guards.clone(),
                )?;
            }
            return Ok(());
        }
        let success = self.row.data["status"] == "succeeded";
        let receipt = ToolReceipt {
            invocation_id: self.row.id.clone(),
            success,
            content: vec![
                json!({"type":"text","text":if success{self.row.data["result"].to_string()}else{self.row.data["error"].to_string()}}),
            ],
            structured_content: success.then(|| self.row.data["result"].clone()),
            remote_effect: "not_applied".into(),
            error: (!success)
                .then(|| json!({"issuer":"kernel_context","error":self.row.data["error"]})),
            origin: Origin::External,
        };
        let mut intent = record(
            "context_reply_intent",
            self.row.id.clone(),
            self.row.scope.clone(),
            "返回内核上下文结果".into(),
            1,
            None,
            json!({"issuer":"kernel_context","call_version":self.version,"value":receipt,"active":self.active}),
            true,
        );
        intent.id = key.id.clone();
        let mut data = self.row.data.clone();
        data["reply_status"] = json!(if self.active {
            "dispatching"
        } else {
            "worker_unavailable"
        });
        data["reply_sent"] = json!(!self.active);
        data["reply_issuer"] = json!("kernel_context");
        let next = self.next(data);
        let mut guards = self.guards.clone();
        guards.push(ResourceVersion {
            resource: key.clone(),
            revision: 0,
        });
        let outcome = self.commit(
            engine,
            "reply-prepare",
            &next,
            vec![ResourceChange {
                resource: key,
                value: intent.to_storage(),
            }],
            guards,
        )?;
        if !outcome.applied || !self.active {
            return Ok(());
        }
        let admitted = Self {
            row: next,
            version: ResourceVersion {
                resource: self.version.resource.clone(),
                revision: self.version.revision + 1,
            },
            session: self.session.clone(),
            guards: self.guards.clone(),
            active: self.active,
            suspended: self.suspended,
        };
        let result =
            engine
                .harness
                .answer_tool_call(&self.session, &self.row.data["request_id"], &receipt);
        let observed = admitted.observe(engine, "reply", &json!(result))?;
        let mut data = admitted.row.data.clone();
        data["reply_sent"] = json!(result.is_ok());
        data["reply_status"] = json!(if result.is_ok() {
            "delivered"
        } else {
            "needs_reconciliation"
        });
        data["reply_observation_id"] = json!(observed.resource.id);
        admitted.commit(
            engine,
            "reply-admit",
            &admitted.next(data),
            vec![],
            vec![observed],
        )?;
        Ok(())
    }
}

impl Engine {
    pub(crate) fn recover_agent_context_calls(&self) -> Result<()> {
        if !matches!(self.store.namespace(), StoreNamespace::Engineering { .. }) {
            return Ok(());
        }
        let mut after = None;
        loop {
            let page = self.store.context_call_page(after.as_deref())?;
            for id in &page.ids {
                let call = ContextCall::read(self, id)?;
                if call.row.data["status"] == "running" {
                    call.recover_observation(self)?;
                    let current = ContextCall::read(self, id)?;
                    if current.row.data["status"] == "running" {
                        let mut data = current.row.data.clone();
                        data["status"] = json!("needs_reconciliation");
                        data["error"] = json!(err(
                            "context_read_unknown",
                            "原读取已有执行资格但没有结果，不重新读取或发送"
                        ));
                        data["issuer"] = json!("kernel_context");
                        current.commit(
                            self,
                            "read-recovered",
                            &current.next(data),
                            vec![],
                            vec![],
                        )?;
                    }
                }
                // Startup has already fenced the previous native Turn. This
                // closes unavailable replies or admits an existing observation;
                // it never grants a second attempt to deliver the old result.
                ContextCall::read(self, id)?.reply(self)?;
            }
            let Some(next) = page.next_after else {
                break;
            };
            after = Some(next);
        }
        Ok(())
    }
    pub(crate) fn drive_agent_context_calls(&self) -> Result<()> {
        if !matches!(self.store.namespace(), StoreNamespace::Engineering { .. }) {
            return Ok(());
        }
        let after = self
            .context_call_cursor
            .lock()
            .map_err(|e| err("storage", e.to_string()))?
            .clone();
        let mut page = self.store.context_call_page(after.as_deref())?;
        if page.ids.is_empty() && after.is_some() {
            page = self.store.context_call_page(None)?;
        }
        let started = std::time::Instant::now();
        let mut visited = 0;
        let mut last = None;
        for id in &page.ids {
            if visited > 0 && started.elapsed() >= std::time::Duration::from_millis(25) {
                break;
            }
            visited += 1;
            last = Some(id.clone());
            *self
                .context_call_cursor
                .lock()
                .map_err(|e| err("storage", e.to_string()))? = last.clone();
            let work = (|| -> Result<()> {
                let call = ContextCall::read(self, id)?;
                call.execute(self)?;
                let call = ContextCall::read(self, id)?;
                call.recover_observation(self)?;
                ContextCall::read(self, id)?.reply(self)
            })();
            if let Err(error) = work
                && !matches!(error.code.as_str(), "revision_conflict" | "storage_busy")
            {
                return Err(error);
            }
        }
        *self
            .context_call_cursor
            .lock()
            .map_err(|e| err("storage", e.to_string()))? = if visited < page.ids.len() {
            last
        } else {
            page.next_after
        };
        Ok(())
    }
}
