//! Once-admitted worker opening and input dispatch with original-version callbacks.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    tool_calls::WorkflowToolAuthority,
    *,
};
use ports::{SessionStart, TurnDispatch};

#[derive(Clone, Serialize, Deserialize)]
pub struct ConversationAuthority {
    pub workflow: Option<WorkflowToolAuthority>,
    pub controller: Option<(Id, Id, u64)>,
}
pub struct ConversationStartSource {
    pub session: Session,
    pub turn: ConversationTurn,
    pub agent: Definition,
    pub skills: Vec<Definition>,
    pub specifications: Vec<ConfigurationRecord>,
    pub configured_task: Option<ConfiguredAgentTask>,
    pub snapshot: Option<Snapshot>,
    pub documents: Vec<Document>,
    pub context_references: Vec<Value>,
    pub authority: ConversationAuthority,
    pub task_record_id: Option<Id>,
    pub context_record_id: Option<Id>,
    pub read_set: ReadSet,
}
struct ConversationInput {
    request: TurnDispatch,
    execution: RuntimeExecution,
    execution_revision: u64,
    fingerprint: String,
    open_intent: ResourceVersion,
}
struct ConversationStart {
    request: SessionStart,
    turn: ConversationTurn,
    authority: ConversationAuthority,
    open_intent: ResourceVersion,
    execution_revision: u64,
    fingerprint: String,
    context_references: Vec<Value>,
}
fn scope(session: &Session) -> Result<Scope> {
    if session.scope == Some(SessionScope::Global) {
        Ok(Scope::global())
    } else {
        Ok(Scope::project(
            session
                .project_id
                .as_ref()
                .ok_or_else(|| err("invalid_scope", "启动缺少项目"))?,
        ))
    }
}
fn version(row: &ResourceSnapshot) -> ResourceVersion {
    ResourceVersion {
        resource: row.resource.clone(),
        revision: row.revision,
    }
}
fn stable_id(value: impl Serialize) -> Result<Id> {
    uuid::Uuid::parse_str(&configuration_hash(&value)[..32])
        .map(|id| id.to_string())
        .map_err(|e| err("storage", e.to_string()))
}

fn original_read_set(versions: Vec<ResourceVersion>) -> Result<ReadSet> {
    let mut merged = std::collections::BTreeMap::new();
    for version in versions {
        if let Some(original) = merged.insert(version.resource, version.revision)
            && original != version.revision
        {
            return Err(err("revision_conflict", "派发源事实在准备期间变化"));
        }
    }
    ReadSet::new(
        merged
            .into_iter()
            .map(|(resource, revision)| ResourceVersion { resource, revision })
            .collect(),
    )
}

impl ConversationAuthority {
    pub(crate) fn live(
        &self,
        store: &dyn ports::EngineeringStore,
        session: &Session,
    ) -> Result<Option<Vec<ResourceVersion>>> {
        let mut guards = Vec::new();
        if let Some(authority) = &self.workflow {
            let facts = store.workflow_tool_facts(
                authority,
                session
                    .project_id
                    .as_deref()
                    .ok_or_else(|| err("invalid_scope", "工作流启动缺少项目"))?,
            )?;
            if facts.suspended {
                return Ok(None);
            }
            if !facts.allowed {
                return Err(err("revision_conflict", "原任务步骤或祖先资格已变化"));
            }
            guards.extend(facts.versions);
        }
        if let Some((binding, controller, epoch)) = &self.controller {
            let keys = [
                ResourceKey::new("governance", binding),
                ResourceKey::new("controller_authority", controller),
            ];
            let rows = store.read_resources(&keys)?;
            require(
                rows.len() == 2 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
                "控制器原资格读取不完整",
            )?;
            let binding = rows[0]
                .value
                .as_ref()
                .ok_or_else(|| err("not_found", "原控制器绑定缺失"))?;
            let controller = rows[1]
                .value
                .as_ref()
                .ok_or_else(|| err("not_found", "原控制器缺失"))?;
            if binding["type"] != "controller_session"
                || binding["definition_id"] != session.id
                || binding["data"]["epoch"] != *epoch
                || controller["type"] != "workflow_controller"
                || controller["data"]["session_id"] != session.id
                || controller["data"]["epoch"] != *epoch
                || controller["data"]["status"] != "active"
            {
                return Err(err("revision_conflict", "原控制器会话或代次已撤销"));
            }
            guards.extend(rows.iter().map(version));
        }
        Ok(Some(guards))
    }
}

impl Engine {
    pub(crate) fn resume_conversation_dispatch(
        &self,
        source: &ConversationStartSource,
    ) -> Result<bool> {
        let session = &source.session;
        let turn = &source.turn;
        let key = ResourceKey::new("conversation_open_seed", &turn.id);
        let rows = self.store.read_resources(std::slice::from_ref(&key))?;
        require(
            rows.len() == 1 && rows[0].resource == key,
            "原会话启动种子读取不完整",
        )?;
        let Some(value) = rows[0].value.as_ref() else {
            return Ok(false);
        };
        let seed = ConfigurationRecord::from_storage(value.clone())?;
        if seed.data["status"] == "prepared" {
            require(
                seed.data["basis"]["session_revision"] == session.revision
                    && seed.data["basis"]["turn_revision"] == turn.revision,
                "启动轮询不能替换原版本",
            )?;
            let request: SessionStart =
                serde_json::from_value(self.store.restore_runtime_value(&seed.data["request"])?)
                    .map_err(|e| err("storage", e.to_string()))?;
            require(
                configuration_hash(&request) == seed.data["request_digest"],
                "启动轮询不能替换原请求",
            )?;
            let mut start = ConversationStart {
                request,
                turn: turn.clone(),
                authority: serde_json::from_value(seed.data["authority"].clone())
                    .map_err(|e| err("storage", e.to_string()))?,
                open_intent: ResourceVersion {
                    resource: ResourceKey::new("governance", &seed.id),
                    revision: rows[0].revision,
                },
                execution_revision: 1,
                fingerprint: text(&seed.data, "fingerprint")?,
                context_references: source.context_references.clone(),
            };
            let opened = self
                .harness
                .poll_session_start(&start.request.session, &start.request.execution);
            if matches!(opened, Ok(None)) {
                return Ok(true);
            }
            let result = opened.and_then(|value| {
                value.ok_or_else(|| err("needs_reconciliation", "原生启动未返回回执"))
            });
            let observed = self.journal_conversation_start(&start, "open", &json!(result))?;
            match result {
                Ok(receipt) => {
                    self.admit_conversation_open(&mut start, &receipt, &observed)?;
                    if let Some(authority) = start
                        .authority
                        .live(self.store.as_ref(), &start.request.session)?
                    {
                        self.dispatch_conversation_input(&mut start, authority)?;
                    }
                }
                Err(error) => {
                    self.journal_conversation_failure(session, &turn.id, &error)?;
                    if error.execution_failure.is_some_and(|f| {
                        f.kind == ExecutionFailureKind::Technical
                            && f.effect == ExecutionFailureEffect::NotApplied
                    }) {
                        self.finish_conversation(
                            session,
                            &turn.id,
                            turn.revision,
                            TurnStatus::Failed,
                            &error.code,
                            true,
                        )?;
                    } else {
                        self.unknown_conversation(
                            session,
                            &turn.id,
                            turn.revision,
                            &error.to_string(),
                        )?;
                    }
                }
            }
            return Ok(true);
        }
        if seed.data["status"] != "completed" {
            return Ok(false);
        }
        require(
            seed.data["session_id"] == session.id && seed.data["turn_id"] == turn.id,
            "派发恢复跨原会话",
        )?;
        if seed.data["after_open"]["session_revision"] != session.revision
            || seed.data["after_open"]["turn_revision"] != turn.revision
        {
            return Err(err("revision_conflict", "恢复不能更换打开会话后的原版本"));
        }
        let authority: ConversationAuthority =
            serde_json::from_value(seed.data["authority"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        let Some(guards) = authority.live(self.store.as_ref(), session)? else {
            return Ok(true);
        };
        require(
            seed.data["task_record_id"] == json!(source.task_record_id)
                && seed.data["context_record_id"] == json!(source.context_record_id),
            "派发恢复不能替换原任务或上下文",
        )?;
        let specifications = source
            .snapshot
            .as_ref()
            .map(|s| serde_json::from_str::<Value>(&s.content))
            .transpose()
            .map_err(|e| err("invalid_configuration", e.to_string()))?
            .and_then(|v| v.get("specifications").cloned())
            .unwrap_or(json!([]));
        let execution_key = ResourceKey::new("execution", text(&seed.data, "execution_id")?);
        let workers = self
            .store
            .read_resources(std::slice::from_ref(&execution_key))?;
        require(
            workers.len() == 1 && workers[0].resource == execution_key,
            "原启动 worker 读取不完整",
        )?;
        let execution: RuntimeExecution = serde_json::from_value(
            self.store.restore_runtime_value(
                workers[0]
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "原 worker 缺失"))?,
            )?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        require(
            execution.session_id == session.id
                && execution.operation_id == seed.data["worker_operation_id"],
            "恢复不能替换原 worker",
        )?;
        let mut input = ConversationInput {
            request: TurnDispatch {
                session: session.clone(),
                turn: turn.clone(),
                configured_task: source.configured_task.clone(),
                specifications: serde_json::from_value(specifications)
                    .map_err(|e| err("invalid_configuration", e.to_string()))?,
                context_references: source.context_references.clone(),
            },
            execution,
            execution_revision: seed.data["after_open"]["execution_revision"]
                .as_u64()
                .ok_or_else(|| err("storage", "原 worker 版本缺失"))?,
            fingerprint: text(&seed.data, "fingerprint")?,
            open_intent: ResourceVersion {
                resource: ResourceKey::new("governance", &seed.id),
                revision: seed.revision,
            },
        };
        if let Err(error) = self.dispatch_native_input(&mut input, guards) {
            self.journal_conversation_failure(session, &turn.id, &error)?;
            if matches!(error.code.as_str(), "revision_conflict" | "invalid_scope") {
                return Err(error);
            }
            self.unknown_conversation(session, &turn.id, turn.revision, &error.to_string())?;
        }
        Ok(true)
    }
    pub(crate) fn start_conversation(
        &self,
        source: ConversationStartSource,
        limit: usize,
    ) -> Result<bool> {
        let Some(mut start) = self.prepare_conversation_start(&source, limit)? else {
            return Ok(false);
        };
        let work = (|| -> Result<()> {
            let opened = self.harness.begin_session(&start.request);
            if matches!(opened, Ok(None)) {
                return Ok(());
            }
            let opened = opened.and_then(|value| {
                value.ok_or_else(|| err("needs_reconciliation", "原生启动未返回回执"))
            });
            let observation = self.journal_conversation_start(&start, "open", &json!(opened))?;
            let receipt = opened?;
            self.admit_conversation_open(&mut start, &receipt, &observation)?;
            let Some(authority) = start
                .authority
                .live(self.store.as_ref(), &start.request.session)?
            else {
                // The opened worker remains owned, without consuming input.
                // A separate dispatch intent will only be prepared on resume.
                return Err(err(
                    "conversation_dispatch_suspended",
                    "原任务暂停，输入尚未派发",
                ));
            };
            self.dispatch_conversation_input(&mut start, authority)
        })();
        if let Err(error) = work {
            self.journal_conversation_failure(&start.request.session, &start.turn.id, &error)?;
            if matches!(error.code.as_str(), "revision_conflict" | "invalid_scope") {
                return Err(error);
            }
            if error.code == "conversation_dispatch_suspended" {
                return Ok(true);
            }
            if error.execution_failure.is_some_and(|f| {
                f.kind == ExecutionFailureKind::Technical
                    && f.effect == ExecutionFailureEffect::NotApplied
            }) {
                self.finish_conversation(
                    &start.request.session,
                    &start.turn.id,
                    start.turn.revision,
                    TurnStatus::Failed,
                    &error.code,
                    true,
                )?;
            } else {
                self.unknown_conversation(
                    &start.request.session,
                    &start.turn.id,
                    start.turn.revision,
                    &error.to_string(),
                )?;
            }
        }
        Ok(true)
    }

    fn prepare_conversation_start(
        &self,
        source: &ConversationStartSource,
        limit: usize,
    ) -> Result<Option<ConversationStart>> {
        let session = &source.session;
        let turn = &source.turn;
        let binding = session
            .runtime_binding
            .as_ref()
            .ok_or_else(|| err("blocked", "会话缺少冻结运行锁"))?;
        let image = match &binding.installation.artifact {
            RuntimeArtifact::DockerImage { image_id } => image_id.clone(),
            _ => return Err(err("unsupported", "讨论需要固定镜像")),
        };
        let configured_task = source.configured_task.clone();
        let authority = source.authority.clone();
        let Some(mut guards) = authority.live(self.store.as_ref(), session)? else {
            return Ok(None);
        };
        let digest = configuration_hash(&(
            "conversation-start/v1",
            self.store.namespace(),
            self.store.root(),
            &session.id,
            session.revision,
            &turn.id,
            turn.revision,
        ));
        let command = format!("conversation-open-prepare:{digest}");
        // Recovery never reopens an admitted operation, even when no observation
        // was committed. Startup reconciliation owns that unknown effect.
        if self.store.replay_commit(&command, &digest)?.is_some() {
            return Ok(None);
        }
        let execution = RuntimeExecution {
            id: stable_id((&digest, "execution"))?,
            operation_id: stable_id((&digest, "operation"))?,
            session_id: session.id.clone(),
            permissions: binding.permissions.clone(),
            kind: RuntimeExecutionKind::Harness,
            container_id: None,
            image_digest: image,
            status: Status::Prepared,
            writer_revoked: false,
            started: Some(now()),
            finished: None,
            protocol_artifact: None,
        };
        let snapshot = source.snapshot.clone();
        let documents = source.documents.clone();
        let agent = source.agent.clone();
        let skills = source.skills.clone();
        let specifications = source.specifications.clone();
        let mut request = SessionStart {
            session: session.clone(),
            agent,
            skills,
            specifications,
            configured_task,
            code_view: turn.code_view.clone(),
            execution,
            project_root: self.store.root(),
            snapshot,
            documents,
            reconciliation_turn: None,
        };
        let pid = session
            .project_id
            .as_ref()
            .ok_or_else(|| err("invalid_scope", "会话缺少项目命名空间"))?;
        let intent_key = ResourceKey::new("governance", format!("conversation-open:{digest}"));
        let execution_key = ResourceKey::new("execution", &request.execution.id);
        let mut keys = vec![
            ResourceKey::new("session", &session.id),
            ResourceKey::new("turn", &turn.id),
            ResourceKey::new("session_live_workers", &session.id),
            ResourceKey::new("conversation_capacity", pid),
            ResourceKey::new("conversation_writers", pid),
            intent_key.clone(),
            execution_key.clone(),
        ];
        if session.scope != Some(SessionScope::Global) {
            keys.push(ResourceKey::new("project", pid));
        }
        keys.push(ResourceKey::new("source", &request.agent.id));
        keys.extend(
            request
                .skills
                .iter()
                .map(|d| ResourceKey::new("source", &d.id)),
        );
        keys.extend(
            request
                .specifications
                .iter()
                .map(|d| ResourceKey::new("source", &d.id)),
        );
        keys.extend(
            request
                .documents
                .iter()
                .map(|d| ResourceKey::new("immutable_document", &d.id)),
        );
        if let Some(snapshot) = &request.snapshot {
            keys.push(ResourceKey::new("immutable_snapshot", &snapshot.id));
        }
        keys.sort();
        keys.dedup();
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == keys.len() && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "启动接纳事实不完整",
        )?;
        let facts = rows
            .iter()
            .map(|r| (&r.resource, r))
            .collect::<std::collections::BTreeMap<_, _>>();
        let session_row = facts[&ResourceKey::new("session", &session.id)];
        let turn_row = facts[&ResourceKey::new("turn", &turn.id)];
        if session_row.revision != session.revision || turn_row.revision != turn.revision {
            return Err(err("revision_conflict", "启动的原会话或轮次已变化"));
        }
        let mut raw_session = session_row
            .value
            .clone()
            .ok_or_else(|| err("not_found", "原会话缺失"))?;
        let mut raw_turn = turn_row
            .value
            .clone()
            .ok_or_else(|| err("not_found", "原轮次缺失"))?;
        require(
            raw_turn["session_id"] == session.id
                && raw_turn["project_id"] == *pid
                && raw_turn["input_digest"] == turn.input_digest
                && raw_turn["status"] == "queued"
                && raw_turn["cancel_requested"] != true,
            "原输入身份或排队资格不符",
        )?;
        require(
            self.store
                .restore_runtime_value(&raw_session["runtime_binding"])?
                == json!(binding),
            "启动改变了原冻结运行锁",
        )?;
        let workers = facts[&ResourceKey::new("session_live_workers", &session.id)]
            .value
            .as_ref()
            .and_then(Value::as_array)
            .ok_or_else(|| err("storage", "原 worker 谓词缺失"))?;
        if !workers.is_empty() {
            return Ok(None);
        }
        let project_limit = facts
            .get(&ResourceKey::new("project", pid))
            .and_then(|r| r.value.as_ref())
            .and_then(|v| v["concurrency"].as_u64())
            .map(|v| v as usize)
            .unwrap_or(limit);
        let active = facts[&ResourceKey::new("conversation_capacity", pid)]
            .value
            .as_ref()
            .and_then(|v| v["active"].as_u64())
            .ok_or_else(|| err("storage", "准入容量谓词缺失"))? as usize;
        if active >= limit.min(project_limit) {
            return Ok(None);
        }
        if binding.tool_policy != ToolPolicy::ReadOnly
            && request.configured_task.is_some()
            && let Some(code) = &turn.code_view
        {
            for writer in facts[&ResourceKey::new("conversation_writers", pid)]
                .value
                .as_ref()
                .and_then(Value::as_array)
                .ok_or_else(|| err("storage", "工作区准入谓词缺失"))?
            {
                if writer["worktree"] == code.context.source.worktree {
                    if writer["workspace_operation"].is_string() {
                        return Ok(None);
                    }
                    let original: RuntimeBinding = serde_json::from_value(
                        self.store
                            .restore_runtime_value(&writer["runtime_binding"])?,
                    )
                    .map_err(|e| err("storage", e.to_string()))?;
                    if original.tool_policy != ToolPolicy::ReadOnly {
                        return Ok(None);
                    }
                }
            }
        }
        for row in &rows {
            if matches!(
                row.resource.kind.as_str(),
                "source" | "immutable_document" | "immutable_snapshot"
            ) {
                require(row.value.is_some(), "启动的精确源或材料缺失")?;
            }
        }
        raw_session["lifecycle"] = json!(SessionLifecycle::Active);
        raw_session["revision"] = json!(session.revision + 1);
        raw_turn["status"] = json!(TurnStatus::Dispatching);
        raw_turn["revision"] = json!(turn.revision + 1);
        raw_turn["started"] = json!(request.execution.started);
        request.session.lifecycle = Some(SessionLifecycle::Active);
        request.session.revision += 1;
        let mut next_turn = turn.clone();
        next_turn.status = TurnStatus::Dispatching;
        next_turn.revision += 1;
        next_turn.started = request.execution.started;
        let mut intent = record(
            "conversation_open_intent",
            turn.id.clone(),
            scope(session)?,
            "打开原生会话".into(),
            1,
            None,
            json!({"status":"prepared","session_id":session.id,"turn_id":turn.id,"worker_operation_id":request.execution.operation_id,"execution_id":request.execution.id,"request_digest":configuration_hash(&request),"request":self.store.encode_runtime_value(&json!(request))?,"input_digest":turn.input_digest,"fingerprint":digest,
                "authority":authority,"task_record_id":source.task_record_id,"context_record_id":source.context_record_id,"specification_snapshot_id":request.snapshot.as_ref().map(|s|&s.id),
                "basis":{"session_revision":request.session.revision,"turn_revision":next_turn.revision},"origin":{"kind":"runtime_coordinator","session_id":session.id,"turn_id":turn.id}}),
            false,
        );
        intent.id = intent_key.id.clone();
        guards.extend(source.read_set.versions().iter().cloned());
        guards.extend(rows.iter().map(version));
        let outcome = self.store.submit_changes(&PreparedChangeSet {
            command_id: command,
            fingerprint: digest.clone(),
            read_set: original_read_set(guards)?,
            changes: vec![
                ResourceChange {
                    resource: session_row.resource.clone(),
                    value: raw_session,
                },
                ResourceChange {
                    resource: turn_row.resource.clone(),
                    value: raw_turn,
                },
                ResourceChange {
                    resource: execution_key,
                    value: self.store.encode_runtime_value(&json!(request.execution))?,
                },
                ResourceChange {
                    resource: intent_key.clone(),
                    value: intent.to_storage(),
                },
            ],
            result: json!({"id":intent.id}),
            time: now() as i64,
        })?;
        Ok(outcome.applied.then_some(ConversationStart {
            request,
            turn: next_turn,
            authority,
            open_intent: ResourceVersion {
                resource: intent_key,
                revision: 1,
            },
            execution_revision: 1,
            fingerprint: digest,
            context_references: source.context_references.clone(),
        }))
    }

    fn journal_conversation_start(
        &self,
        start: &ConversationStart,
        phase: &str,
        result: &Value,
    ) -> Result<ResourceVersion> {
        let key = ResourceKey::new(
            "governance",
            format!("conversation-{phase}-observation:{}", start.fingerprint),
        );
        let mut observed = record(
            "conversation_start_observation",
            start.open_intent.resource.id.clone(),
            scope(&start.request.session)?,
            format!("原生 {phase} 观察"),
            1,
            None,
            json!({"phase":phase,"session_id":start.request.session.id,"turn_id":start.turn.id,"worker_operation_id":start.request.execution.operation_id,"value":{"result":result}}),
            true,
        );
        observed.id = key.id.clone();
        let receipt = self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("conversation-{phase}-journal:{}", start.fingerprint),
            fingerprint: configuration_hash(&observed.data),
            read_set: ReadSet::new(vec![ResourceVersion {
                resource: key.clone(),
                revision: 0,
            }])?,
            changes: vec![ResourceChange {
                resource: key.clone(),
                value: observed.to_storage(),
            }],
            result: json!({"id":observed.id}),
            time: now() as i64,
        })?;
        receipt
            .versions
            .into_iter()
            .find(|v| v.resource == key)
            .ok_or_else(|| err("storage", "启动观察缺少原版本"))
    }

    fn admit_conversation_open(
        &self,
        start: &mut ConversationStart,
        receipt: &ports::SessionReceipt,
        observed: &ResourceVersion,
    ) -> Result<()> {
        require(
            receipt.operation_id == start.request.execution.operation_id
                && receipt.image_digest == start.request.execution.image_digest
                && start
                    .request
                    .session
                    .external_session
                    .as_ref()
                    .is_none_or(|id| id == &receipt.external_session),
            "原生会话回执改变了原身份",
        )?;
        let keys = [
            ResourceKey::new("session", &start.request.session.id),
            ResourceKey::new("turn", &start.turn.id),
            ResourceKey::new("execution", &start.request.execution.id),
            start.open_intent.resource.clone(),
            ResourceKey::new("tool_worker", &start.request.execution.operation_id),
        ];
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == keys.len() && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "会话打开回调事实不完整",
        )?;
        let mut session = rows[0]
            .value
            .clone()
            .ok_or_else(|| err("not_found", "原会话缺失"))?;
        let mut worker = rows[2]
            .value
            .clone()
            .ok_or_else(|| err("not_found", "原 worker 缺失"))?;
        let mut intent = ConfigurationRecord::from_storage(
            rows[3]
                .value
                .clone()
                .ok_or_else(|| err("not_found", "原打开意图缺失"))?,
        )?;
        let mut guards = vec![
            ResourceVersion {
                resource: keys[0].clone(),
                revision: start.request.session.revision,
            },
            ResourceVersion {
                resource: keys[1].clone(),
                revision: start.turn.revision,
            },
            ResourceVersion {
                resource: keys[2].clone(),
                revision: start.execution_revision,
            },
            start.open_intent.clone(),
            ResourceVersion {
                resource: keys[4].clone(),
                revision: 1,
            },
            observed.clone(),
        ];
        session["external_session"] = json!(receipt.external_session);
        session["revision"] = json!(start.request.session.revision + 1);
        worker["container_id"] = json!(receipt.container_id);
        worker["status"] = json!(Status::Running);
        let mut data = intent.data.clone();
        data["status"] = json!("completed");
        data["observation_id"] = json!(observed.resource.id);
        data["after_open"] = json!({"session_revision":start.request.session.revision+1,"turn_revision":start.turn.revision,"execution_revision":start.execution_revision+1});
        update_record(&mut intent, data);
        self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("conversation-open-admit:{}", start.fingerprint),
            fingerprint: configuration_hash(&(
                "conversation-open-admit/v1",
                &start.fingerprint,
                observed,
            )),
            read_set: ReadSet::new(std::mem::take(&mut guards))?,
            changes: vec![
                ResourceChange {
                    resource: keys[0].clone(),
                    value: session,
                },
                ResourceChange {
                    resource: keys[2].clone(),
                    value: worker,
                },
                ResourceChange {
                    resource: keys[3].clone(),
                    value: intent.to_storage(),
                },
            ],
            result: json!({"id":start.request.session.id}),
            time: now() as i64,
        })?;
        start.request.session.revision += 1;
        start.request.session.external_session = Some(receipt.external_session.clone());
        start.request.execution.container_id = Some(receipt.container_id.clone());
        start.request.execution.status = Status::Running;
        start.execution_revision += 1;
        start.open_intent.revision += 1;
        Ok(())
    }

    fn dispatch_conversation_input(
        &self,
        start: &mut ConversationStart,
        authority: Vec<ResourceVersion>,
    ) -> Result<()> {
        let specifications = start
            .request
            .snapshot
            .as_ref()
            .map(|s| serde_json::from_str::<Value>(&s.content))
            .transpose()
            .map_err(|e| err("invalid_configuration", e.to_string()))?
            .and_then(|v| v.get("specifications").cloned())
            .unwrap_or(json!([]));
        let dispatch = TurnDispatch {
            session: start.request.session.clone(),
            turn: start.turn.clone(),
            configured_task: start.request.configured_task.clone(),
            specifications: serde_json::from_value(specifications)
                .map_err(|e| err("invalid_configuration", e.to_string()))?,
            context_references: start.context_references.clone(),
        };
        let mut input = ConversationInput {
            request: dispatch,
            execution: start.request.execution.clone(),
            execution_revision: start.execution_revision,
            fingerprint: start.fingerprint.clone(),
            open_intent: start.open_intent.clone(),
        };
        let result = self.dispatch_native_input(&mut input, authority);
        start.turn = input.request.turn;
        result
    }
    fn dispatch_native_input(
        &self,
        input: &mut ConversationInput,
        authority: Vec<ResourceVersion>,
    ) -> Result<()> {
        let dispatch = &input.request;
        let key = ResourceKey::new(
            "governance",
            format!("conversation-dispatch:{}", input.fingerprint),
        );
        let mut intent = record(
            "conversation_dispatch_intent",
            input.request.turn.id.clone(),
            scope(&dispatch.session)?,
            "派发原输入".into(),
            1,
            None,
            json!({"status":"prepared","session_id":dispatch.session.id,"turn_id":dispatch.turn.id,"worker_operation_id":input.execution.operation_id,"request_digest":configuration_hash(&dispatch),"basis":{"session_revision":dispatch.session.revision,"turn_revision":dispatch.turn.revision,"execution_revision":input.execution_revision},"origin":{"kind":"runtime_coordinator","session_id":dispatch.session.id,"turn_id":dispatch.turn.id}}),
            false,
        );
        intent.id = key.id.clone();
        let mut guards = authority;
        guards.extend([
            input.open_intent.clone(),
            ResourceVersion {
                resource: ResourceKey::new("session", &dispatch.session.id),
                revision: dispatch.session.revision,
            },
            ResourceVersion {
                resource: ResourceKey::new("turn", &dispatch.turn.id),
                revision: dispatch.turn.revision,
            },
            ResourceVersion {
                resource: ResourceKey::new("execution", &input.execution.id),
                revision: input.execution_revision,
            },
            ResourceVersion {
                resource: ResourceKey::new("tool_worker", &input.execution.operation_id),
                revision: 1,
            },
            ResourceVersion {
                resource: key.clone(),
                revision: 0,
            },
        ]);
        intent.data["read_set"] = json!(ReadSet::new(guards.clone())?);
        intent.content_hash = content_digest(&intent.refs, &intent.data);
        let granted = self.store.submit_changes(&PreparedChangeSet {
            command_id: format!("conversation-dispatch-prepare:{}", input.fingerprint),
            fingerprint: configuration_hash(&(
                "conversation-dispatch-prepare/v1",
                &input.fingerprint,
            )),
            read_set: ReadSet::new(guards.clone())?,
            changes: vec![ResourceChange {
                resource: key.clone(),
                value: intent.to_storage(),
            }],
            result: json!({"id":intent.id}),
            time: now() as i64,
        })?;
        let observed = if granted.applied {
            let result = self.harness.dispatch_turn(dispatch);
            let observed = self.journal_conversation_input(input, &json!(result))?;
            result?;
            observed
        } else {
            let observation_key = ResourceKey::new(
                "governance",
                format!("conversation-dispatch-observation:{}", input.fingerprint),
            );
            let rows = self
                .store
                .read_resources(&[key.clone(), observation_key.clone()])?;
            require(rows.len() == 2, "原输入派发事实读取不完整")?;
            intent = ConfigurationRecord::from_storage(
                rows[0]
                    .value
                    .clone()
                    .ok_or_else(|| err("not_found", "原派发意图缺失"))?,
            )?;
            require(
                intent.data["request_digest"] == configuration_hash(dispatch),
                "输入恢复不能替换原冻结派发请求",
            )?;
            if intent.data["status"] == "completed" {
                input.request.turn.revision += 1;
                input.request.turn.status = TurnStatus::Running;
                return Ok(());
            }
            let original: ReadSet = serde_json::from_value(intent.data["read_set"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            guards = original.versions().to_vec();
            let observation = rows[1].value.as_ref().ok_or_else(|| {
                err(
                    "conversation_dispatch_unresolved",
                    "原输入已有派发尝试但无回执，不能重发",
                )
            })?;
            require(
                observation["type"] == "conversation_start_observation"
                    && observation["data"]["phase"] == "dispatch"
                    && observation["data"]["session_id"] == dispatch.session.id
                    && observation["data"]["turn_id"] == dispatch.turn.id
                    && observation["data"]["worker_operation_id"] == input.execution.operation_id,
                "原输入回执身份不符",
            )?;
            let result: Result<()> =
                serde_json::from_value(observation["data"]["value"]["result"].clone())
                    .map_err(|e| err("storage", e.to_string()))?;
            result?;
            version(&rows[1])
        };
        let turn_key = ResourceKey::new("turn", &input.request.turn.id);
        let rows = self.store.read_resources(std::slice::from_ref(&turn_key))?;
        require(
            rows.len() == 1 && rows[0].resource == turn_key,
            "输入派发回调读取不完整",
        )?;
        let mut turn = rows[0]
            .value
            .clone()
            .ok_or_else(|| err("not_found", "原轮次缺失"))?;
        turn["status"] = json!(TurnStatus::Running);
        turn["revision"] = json!(input.request.turn.revision + 1);
        let mut data = intent.data.clone();
        data["status"] = json!("completed");
        data["observation_id"] = json!(observed.resource.id);
        update_record(&mut intent, data);
        guards.retain(|v| v.resource != key);
        guards.push(ResourceVersion {
            resource: key.clone(),
            revision: 1,
        });
        guards.push(observed);
        self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("conversation-dispatch-admit:{}", input.fingerprint),
            fingerprint: configuration_hash(&(
                "conversation-dispatch-admit/v1",
                &input.fingerprint,
            )),
            read_set: ReadSet::new(guards)?,
            changes: vec![
                ResourceChange {
                    resource: turn_key,
                    value: turn,
                },
                ResourceChange {
                    resource: key,
                    value: intent.to_storage(),
                },
            ],
            result: json!({"id":input.request.turn.id}),
            time: now() as i64,
        })?;
        input.request.turn.revision += 1;
        input.request.turn.status = TurnStatus::Running;
        Ok(())
    }
    fn journal_conversation_input(
        &self,
        input: &ConversationInput,
        result: &Value,
    ) -> Result<ResourceVersion> {
        let key = ResourceKey::new(
            "governance",
            format!("conversation-dispatch-observation:{}", input.fingerprint),
        );
        let mut observed = record(
            "conversation_start_observation",
            input.open_intent.resource.id.clone(),
            scope(&input.request.session)?,
            "原生 dispatch 观察".into(),
            1,
            None,
            json!({"phase":"dispatch","session_id":input.request.session.id,"turn_id":input.request.turn.id,"worker_operation_id":input.execution.operation_id,"value":{"result":result}}),
            true,
        );
        observed.id = key.id.clone();
        let receipt = self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("conversation-dispatch-journal:{}", input.fingerprint),
            fingerprint: configuration_hash(&observed.data),
            read_set: ReadSet::new(vec![ResourceVersion {
                resource: key.clone(),
                revision: 0,
            }])?,
            changes: vec![ResourceChange {
                resource: key.clone(),
                value: observed.to_storage(),
            }],
            result: json!({"id":observed.id}),
            time: now() as i64,
        })?;
        receipt
            .versions
            .into_iter()
            .find(|v| v.resource == key)
            .ok_or_else(|| err("storage", "派发观察缺少原版本"))
    }
}
