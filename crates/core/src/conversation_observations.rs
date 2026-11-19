//! Original worker observations are admitted against finite conversation facts.
//! No conversation history or partial EngineeringState is used by this path.
use crate::{concurrency::*, provider_tools::ProviderPoll, *};
use ports::ConversationObservation as Obs;
use std::collections::BTreeMap;

fn lookup(kind: &str, session: &str, external: &str) -> Result<ResourceKey> {
    let key = ResourceKey::new(kind, json!([session, external]).to_string());
    key.validate()?;
    Ok(key)
}

pub(crate) fn checkpoint(turn: &Value) -> Value {
    json!({"turn_revision":turn["revision"],"status":turn["status"],"external_message":turn["external_message"],"external_turn":turn["external_turn"],"consumed":turn["consumed"],"cancel_requested":turn["cancel_requested"]})
}

pub(crate) struct ConversationTransition<'a> {
    pub session: &'a Session,
    pub turn: &'a str,
    pub revision: u64,
    pub status: TurnStatus,
    pub reason: &'a str,
    pub terminal: bool,
    pub forced: bool,
}
impl ConversationTransition<'_> {
    pub fn fingerprint(&self, store: &dyn ports::EngineeringStore) -> String {
        configuration_hash(&(
            "conversation-transition/v1",
            store.namespace(),
            store.root(),
            &self.session.id,
            self.session.revision,
            self.turn,
            self.revision,
            self.status,
            self.reason,
            self.terminal,
            self.forced,
        ))
    }
}

impl Engine {
    /// Status and terminal admissions use the original Session/Turn versions.
    /// Evidence and worker revocation are retained independently on a conflict.
    pub(crate) fn commit_conversation_transition(
        &self,
        transition: &ConversationTransition<'_>,
        extra: &[ResourceVersion],
    ) -> Result<Value> {
        let ConversationTransition {
            session,
            turn: tid,
            revision: turn_revision,
            status,
            reason,
            terminal,
            forced,
        } = *transition;
        let digest = transition.fingerprint(self.store.as_ref());
        let command = format!("conversation-transition:{digest}");
        if let Some(receipt) = self.store.replay_commit(&command, &digest)? {
            return Ok(receipt.result);
        }
        let keys = [
            ResourceKey::new("session", &session.id),
            ResourceKey::new("turn", tid),
            ResourceKey::new("turn_pending_interactions", tid),
            ResourceKey::new("session_worker_boundary", &session.id),
        ];
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == keys.len() && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "会话状态接纳事实不完整",
        )?;
        if rows[0].revision != session.revision || rows[1].revision != turn_revision {
            if let Some(receipt) = self.store.replay_commit(&command, &digest)? {
                return Ok(receipt.result);
            }
            return Err(err(
                "revision_conflict",
                "原会话或轮次已变化，原终态不能覆盖新决定",
            ));
        }
        let mut session_value = rows[0]
            .value
            .clone()
            .ok_or_else(|| err("not_found", "原会话缺失"))?;
        let mut turn = rows[1]
            .value
            .clone()
            .ok_or_else(|| err("not_found", "原轮次缺失"))?;
        require(
            turn["session_id"] == session.id && session_value["project_id"] == turn["project_id"],
            "状态接纳跨会话",
        )?;
        let previous: TurnStatus = serde_json::from_value(turn["status"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        require(previous.active(), "原轮次已结束")?;
        let accepted = if terminal && turn["cancel_requested"] == true {
            TurnStatus::Cancelled
        } else {
            status
        };
        turn["status"] = json!(accepted);
        turn["revision"] = json!(turn_revision + 1);
        if terminal {
            turn["finished"] = json!(now());
        }
        session_value["lifecycle"] = json!(if !terminal {
            SessionLifecycle::NeedsReconciliation
        } else if forced
            || matches!(
                accepted,
                TurnStatus::Cancelled | TurnStatus::Failed | TurnStatus::NeedsReconciliation
            )
        {
            SessionLifecycle::RequiresSuccessor
        } else {
            SessionLifecycle::Idle
        });
        session_value["revision"] = json!(session.revision + 1);
        let mut guards = rows
            .iter()
            .map(|r| (r.resource.clone(), r.revision))
            .collect::<BTreeMap<_, _>>();
        for original in extra {
            if let Some(existing) = guards.insert(original.resource.clone(), original.revision)
                && existing != original.revision
            {
                return Err(err("revision_conflict", "终态前置条件与原事实不符"));
            }
        }
        let mut changes = vec![
            ResourceChange {
                resource: keys[0].clone(),
                value: session_value,
            },
            ResourceChange {
                resource: keys[1].clone(),
                value: turn.clone(),
            },
        ];
        if terminal {
            let pending = rows[2]
                .value
                .as_ref()
                .and_then(Value::as_array)
                .ok_or_else(|| err("storage", "待处理交互缺失"))?;
            for raw in pending {
                require(
                    raw["session_id"] == session.id && raw["turn_id"] == tid,
                    "结束交互跨会话",
                )?;
                let key = ResourceKey::new("interaction", text(raw, "id")?);
                let revision = raw["revision"]
                    .as_u64()
                    .ok_or_else(|| err("storage", "交互缺少版本"))?;
                guards.insert(key.clone(), revision);
                let mut next = raw.clone();
                next["status"] = json!(InteractionStatus::Cancelled);
                next["answered_at"] = json!(now());
                next["revision"] = json!(revision + 1);
                changes.push(ResourceChange {
                    resource: key,
                    value: next,
                });
            }
        }
        let event_key = ResourceKey::new(
            "conversation_event",
            format!("conversation-transition-event:{digest}"),
        );
        guards.insert(event_key.clone(), 0);
        changes.push(ResourceChange {
            resource: event_key.clone(),
            value: json!(ConversationEvent {
                id: event_key.id.clone(),
                session_id: session.id.clone(),
                turn_id: tid.into(),
                sequence: 0,
                deduplication_key: format!(
                    "{tid}:{}:{digest}",
                    if terminal { "terminal" } else { "unknown" }
                ),
                kind: ConversationEventKind::Status,
                content: reason.into(),
                artifact_id: None,
                time: now(),
                mode: MessageMode::Full,
                provider_item: None
            }),
        });
        if terminal && accepted == TurnStatus::Completed {
            if !forced
                && let Some((checkpoint, reads)) = self.workspace_turn_checkpoint(session, tid)?
            {
                for original in reads.versions() {
                    if let Some(previous) =
                        guards.insert(original.resource.clone(), original.revision)
                    {
                        require(previous == original.revision, "工作区检查点依据发生变化")?;
                    }
                }
                changes.push(ResourceChange {
                    resource: ResourceKey::new("governance", &checkpoint.id),
                    value: self.store.encode_runtime_record(&checkpoint)?,
                });
            }
            let key = ResourceKey::new("governance", format!("conversation-capture:{digest}"));
            guards.insert(key.clone(), 0);
            let scope = if session.scope == Some(SessionScope::Global) {
                Scope::global()
            } else {
                Scope::project(text(&turn, "project_id")?)
            };
            let mut intent = governance::record(
                "conversation_capture_intent",
                session.id.clone(),
                scope,
                "完成会话快照".into(),
                1,
                None,
                json!({"status":"prepared","session":session,"turn_id":tid,"terminal_command_id":command,"terminal_event_id":event_key.id,"execution_boundary":rows[3].value,"session_revision":session.revision+1,"turn_revision":turn_revision+1,"kind":"conversation_completed"}),
                false,
            );
            intent.id = key.id.clone();
            changes.push(ResourceChange {
                resource: key,
                value: self.store.encode_runtime_record(&intent)?,
            });
        }
        let result = checkpoint(&turn);
        self.store
            .commit_changes(&PreparedChangeSet {
                command_id: command,
                fingerprint: digest,
                read_set: ReadSet::new(
                    guards
                        .into_iter()
                        .map(|(resource, revision)| ResourceVersion { resource, revision })
                        .collect(),
                )?,
                changes,
                result,
                time: now() as i64,
            })
            .map(|r| r.result)
    }

    pub(crate) fn journal_conversation_failure(
        &self,
        session: &Session,
        tid: &str,
        error: &Error,
    ) -> Result<()> {
        let digest = configuration_hash(&(
            "conversation-failure/v1",
            self.store.namespace(),
            &session.id,
            tid,
            error,
        ));
        let command = format!("conversation-failure:{digest}");
        if self.store.replay_commit(&command, &digest)?.is_some() {
            return Ok(());
        }
        let key = ResourceKey::new(
            "conversation_event",
            format!("conversation-failure-event:{digest}"),
        );
        let turn_key = ResourceKey::new("turn", tid);
        let rows = self.store.read_resources(std::slice::from_ref(&turn_key))?;
        require(
            rows.len() == 1
                && rows[0].resource == turn_key
                && rows[0].value.as_ref().is_some_and(|t| {
                    t["session_id"] == session.id && t["project_id"] == json!(session.project_id)
                }),
            "失败观察不属于原会话",
        )?;
        let event = ConversationEvent {
            id: key.id.clone(),
            session_id: session.id.clone(),
            turn_id: tid.into(),
            sequence: 0,
            deduplication_key: format!("{tid}:execution-failure:{digest}"),
            kind: ConversationEventKind::Status,
            content: serde_json::to_string(error)
                .map_err(|e| err("serialization", e.to_string()))?,
            artifact_id: None,
            time: now(),
            mode: MessageMode::Full,
            provider_item: None,
        };
        self.store.commit_changes(&PreparedChangeSet {
            command_id: command,
            fingerprint: digest,
            read_set: ReadSet::new(vec![ResourceVersion {
                resource: key.clone(),
                revision: 0,
            }])?,
            changes: vec![ResourceChange {
                resource: key,
                value: json!(event),
            }],
            result: json!({"ok":true}),
            time: now() as i64,
        })?;
        Ok(())
    }

    /// Only external tools use this admission protocol. Context/platform tools
    /// retain their complete-state rules until their own writer family moves.
    pub(crate) fn native_provider_observation(
        &self,
        poll: &ProviderPoll,
        observation: &Obs,
    ) -> bool {
        let Obs::ToolCall { tool, .. } = observation else {
            return true;
        };
        if self
            .provider_context_definition(poll, observation)
            .is_ok_and(|d| d.is_some())
        {
            return true;
        }
        poll.session
            .runtime_binding
            .as_ref()
            .and_then(|b| b.agent_configuration.as_ref())
            .and_then(|a| a.integration_lock.as_ref())
            .is_some_and(|l| l.tools.iter().any(|t| t.definition.qualified_name == *tool))
    }

    pub(crate) fn validate_provider_call_scope(
        &self,
        poll: &ProviderPoll,
        turn: &Value,
        observation: &Obs,
    ) -> Result<()> {
        let Obs::ToolCall {
            turn: tid,
            thread,
            external_turn,
            ..
        } = observation
        else {
            return Err(err("invalid_params", "缺少原生工具观察"));
        };
        let binding = poll
            .session
            .runtime_binding
            .as_ref()
            .ok_or_else(|| err("blocked", "缺少原运行锁"))?;
        let input = if binding.installation.key.provider == HarnessProvider::Dsh {
            &turn["external_message"]
        } else {
            &turn["external_turn"]
        };
        require(
            tid == &poll.turn.id
                && json!(poll.session.external_session) == json!(thread)
                && input == external_turn,
            "原生工具线程或输入身份不符",
        )
    }

    fn provider_invocation(
        &self,
        poll: &ProviderPoll,
        turn: &Value,
        observation: &Obs,
    ) -> Result<ToolInvocation> {
        let Obs::ToolCall {
            turn: tid,
            request,
            thread,
            external_turn,
            call,
            tool,
            arguments,
            worktree,
        } = observation
        else {
            return Err(err("invalid_params", "缺少原生工具观察"));
        };
        let session = &poll.session;
        let binding = session
            .runtime_binding
            .as_ref()
            .ok_or_else(|| err("invalid_configuration", "缺少原运行锁"))?;
        let input = if binding.installation.key.provider == HarnessProvider::Dsh {
            &turn["external_message"]
        } else {
            &turn["external_turn"]
        };
        require(
            tid == &poll.turn.id
                && json!(session.external_session) == json!(thread)
                && input == external_turn,
            "原生工具线程或输入身份不符",
        )?;
        let config = binding
            .agent_configuration
            .as_ref()
            .ok_or_else(|| err("invalid_configuration", "缺少原 Agent 锁"))?;
        let bound = config
            .integration_lock
            .as_ref()
            .and_then(|l| {
                l.tools
                    .iter()
                    .find(|t| t.definition.qualified_name == *tool)
            })
            .ok_or_else(|| err("tool_not_authorized", "工具不属于原冻结 Agent"))?;
        let image_id = match &binding.installation.artifact {
            RuntimeArtifact::DockerImage { image_id } => image_id.clone(),
            _ => return Err(err("unsupported", "工具执行要求固定镜像")),
        };
        let identity = configuration_hash(&json!([session.id, tid, poll.worker, call]));
        Ok(ToolInvocation {
            id: format!("tool-call-{}", &identity[..32]),
            project_id: session.project_id.clone(),
            feature_id: session.feature_id.clone(),
            task_id: session.task_id.clone(),
            agent_instance_id: Some(config.agent_instance_id.clone()),
            session_id: Some(session.id.clone()),
            turn_id: Some(tid.clone()),
            operation_id: Some(poll.worker.clone()),
            request_id: Some(request.clone()),
            call_id: call.clone(),
            tool: bound.clone(),
            arguments: arguments.clone(),
            permissions: binding
                .permissions
                .clone()
                .ok_or_else(|| err("invalid_configuration", "缺少原权限锁"))?,
            image_id,
            worktree: worktree.clone(),
            input_hash: configuration_hash(arguments),
        })
    }

    pub(crate) fn admit_provider_observations(
        &self,
        poll: &ProviderPoll,
        observations: &[Obs],
        command: &str,
        fingerprint: &str,
    ) -> Result<Value> {
        poll.require_store(self.store.as_ref())?;
        if let Some(receipt) = self.store.replay_commit(command, fingerprint)? {
            return Ok(receipt.result);
        }
        let turn_key = ResourceKey::new("turn", &poll.turn.id);
        let mut keys = BTreeMap::<ResourceKey, ()>::new();
        keys.insert(turn_key.clone(), ());
        for version in poll.guards()?.versions() {
            keys.insert(version.resource.clone(), ());
        }
        for observation in observations {
            match observation {
                Obs::Message { key, .. } => {
                    keys.insert(lookup("conversation_event_key", &poll.session.id, key)?, ());
                }
                Obs::Interaction { request, .. } => {
                    keys.insert(
                        lookup("interaction_request", &poll.session.id, request)?,
                        (),
                    );
                }
                Obs::ToolCall { call, .. } => {
                    if self
                        .provider_context_definition(poll, observation)?
                        .is_some()
                    {
                        let identity = format!(
                            "agent-context-call-{}",
                            configuration_hash(&(
                                poll.session.id.as_str(),
                                poll.turn.id.as_str(),
                                poll.worker.as_str(),
                                call
                            ))
                        );
                        keys.insert(ResourceKey::new("governance", identity), ());
                        let configuration = poll
                            .session
                            .runtime_binding
                            .as_ref()
                            .and_then(|b| b.agent_configuration.as_ref())
                            .ok_or_else(|| err("blocked", "缺少原 Agent 身份"))?;
                        keys.insert(
                            ResourceKey::new("agent_scope", &configuration.agent_instance_id),
                            (),
                        );
                    } else {
                        let identity = configuration_hash(&json!([
                            poll.session.id,
                            poll.turn.id,
                            poll.worker,
                            call
                        ]));
                        keys.insert(
                            ResourceKey::new(
                                "governance",
                                format!("provider-tool-intent:tool-call-{}", &identity[..32]),
                            ),
                            (),
                        );
                    }
                }
                Obs::Terminal { .. } => return Err(err("invalid_params", "终态需要停止接纳协议")),
                _ => {}
            }
        }
        let keys = keys.into_keys().collect::<Vec<_>>();
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == keys.len() && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "原生观察事实不完整",
        )?;
        let rows = rows
            .into_iter()
            .map(|r| (r.resource.clone(), r))
            .collect::<BTreeMap<_, _>>();
        for original in poll.guards()?.versions() {
            if rows[&original.resource].revision != original.revision {
                if let Some(receipt) = self.store.replay_commit(command, fingerprint)? {
                    return Ok(receipt.result);
                }
                return Err(err(
                    "revision_conflict",
                    "原生观察的原会话、轮次或 worker 已变化",
                ));
            }
        }
        let raw = rows[&turn_key]
            .value
            .clone()
            .ok_or_else(|| err("not_found", "原轮次缺失"))?;
        require(
            raw["id"] == poll.turn.id
                && raw["session_id"] == poll.session.id
                && raw["project_id"] == poll.turn.project_id,
            "原生观察跨作用域",
        )?;
        if raw["revision"] != poll.turn.revision
            || raw["cancel_requested"] == true
            || !poll.turn.status.active()
        {
            return Err(err("revision_conflict", "原轮次已取消或替换"));
        }
        let mut turn = raw.clone();
        let mut changes = BTreeMap::<ResourceKey, Value>::new();
        let mut guards = rows
            .iter()
            .map(|(k, r)| (k.clone(), r.revision))
            .collect::<BTreeMap<_, _>>();
        let mut messages = BTreeMap::<String, Value>::new();
        let mut message_order = Vec::new();
        let mut interactions = BTreeMap::<String, Value>::new();
        let mut tools = BTreeMap::<String, (ToolInvocation, &Obs)>::new();
        let mut contexts = Vec::new();
        for observation in observations {
            match observation {
                Obs::Admitted {
                    turn: tid,
                    message,
                    external_turn,
                } => {
                    require(tid == &poll.turn.id, "输入回执跨轮次")?;
                    for (field, incoming) in [
                        ("external_message", message),
                        ("external_turn", external_turn),
                    ] {
                        if let Some(incoming) = incoming {
                            require(
                                turn[field].is_null() || turn[field] == *incoming,
                                "供应商输入身份不能替换",
                            )?;
                            turn[field] = json!(incoming);
                        }
                    }
                }
                Obs::Consumed { turn: tid, message } => {
                    require(
                        tid == &poll.turn.id
                            && message
                                .as_ref()
                                .is_none_or(|m| turn["external_message"] == *m),
                        "消费事实跨轮次或输入身份不符",
                    )?;
                    turn["consumed"] = json!(true);
                }
                Obs::Message {
                    turn: tid,
                    key,
                    content,
                    kind,
                    mode,
                    item,
                } => {
                    require(
                        tid == &poll.turn.id && content.len() <= 1024 * 1024,
                        "消息跨轮次或过大",
                    )?;
                    let lookup = lookup("conversation_event_key", &poll.session.id, key)?;
                    if let Some(previous) =
                        rows[&lookup].value.as_ref().or_else(|| messages.get(key))
                    {
                        require(
                            previous["turn_id"] == *tid
                                && previous["content"] == *content
                                && previous["kind"] == json!(kind)
                                && previous["mode"] == json!(mode)
                                && previous["provider_item"] == *item,
                            "供应商重复消息改变了原事实",
                        )?;
                        continue;
                    }
                    let event = json!(ConversationEvent {
                        id: id(),
                        session_id: poll.session.id.clone(),
                        turn_id: tid.clone(),
                        sequence: 0,
                        deduplication_key: key.clone(),
                        kind: *kind,
                        content: content.clone(),
                        artifact_id: None,
                        time: now(),
                        mode: *mode,
                        provider_item: Some(item.clone())
                    });
                    messages.insert(key.clone(), event);
                    message_order.push(key.clone());
                }
                Obs::Interaction {
                    turn: tid,
                    request,
                    kind,
                    questions,
                    tool,
                    reason,
                } => {
                    require(tid == &poll.turn.id, "交互请求跨轮次")?;
                    let lookup = lookup("interaction_request", &poll.session.id, request)?;
                    if let Some(previous) = rows[&lookup]
                        .value
                        .as_ref()
                        .or_else(|| interactions.get(request))
                    {
                        require(
                            previous["turn_id"] == *tid
                                && previous["kind"] == json!(kind)
                                && previous["questions"] == json!(questions)
                                && previous["tool"] == json!(tool)
                                && previous["reason"] == *reason,
                            "供应商重复请求改变了原交互",
                        )?;
                        // A duplicate question must never undo a human's answer.
                        if previous["status"] == "pending" {
                            turn["status"] = json!(TurnStatus::WaitingInteraction);
                        }
                        continue;
                    }
                    let interaction = PendingInteraction {
                        id: id(),
                        project_id: poll.turn.project_id.clone(),
                        session_id: poll.session.id.clone(),
                        turn_id: tid.clone(),
                        external_request: request.clone(),
                        kind: *kind,
                        status: InteractionStatus::Pending,
                        questions: questions.clone(),
                        tool: tool.clone(),
                        reason: reason.clone(),
                        answer: None,
                        created_at: Some(now()),
                        answered_at: None,
                        revision: 1,
                    };
                    interactions.insert(request.clone(), json!(interaction));
                    turn["status"] = json!(TurnStatus::WaitingInteraction);
                }
                Obs::ToolCall { .. } => {
                    self.validate_provider_call_scope(poll, &turn, observation)?;
                    if let Some(definition) = self.provider_context_definition(poll, observation)? {
                        contexts.push((observation, definition));
                    } else {
                        let invocation = self.provider_invocation(poll, &turn, observation)?;
                        if let Some((previous, original)) = tools.get(&invocation.id) {
                            require(
                                json!(previous) == json!(invocation)
                                    && json!(original) == json!(observation),
                                "供应商重复调用改变了原请求",
                            )?;
                        } else {
                            tools.insert(invocation.id.clone(), (invocation, observation));
                        }
                    }
                }
                Obs::Terminal { .. } => unreachable!(),
            }
        }
        let next_revision = if turn != raw {
            poll.turn.revision + 1
        } else {
            poll.turn.revision
        };
        if turn != raw {
            turn["revision"] = json!(next_revision);
            changes.insert(turn_key, turn.clone());
        }
        let mut event_changes = Vec::new();
        for external in message_order {
            let event = messages.remove(&external).unwrap();
            let key = ResourceKey::new("conversation_event", text(&event, "id")?);
            guards.insert(key.clone(), 0);
            event_changes.push(ResourceChange {
                resource: key,
                value: event,
            });
        }
        for interaction in interactions.into_values() {
            let key = ResourceKey::new("interaction", text(&interaction, "id")?);
            guards.insert(key.clone(), 0);
            changes.insert(key, interaction);
        }
        for (invocation, observation) in tools.into_values() {
            let key = ResourceKey::new(
                "governance",
                format!("provider-tool-intent:{}", invocation.id),
            );
            if let Some(previous) = &rows[&key].value {
                let previous = self.store.restore_runtime_record(previous)?;
                require(
                    previous["type"] == "provider_tool_intent"
                        && previous["data"]["invocation"] == json!(invocation),
                    "原生调用身份被不同请求复用",
                )?;
                continue;
            }
            let record =
                self.provider_tool_intent_record(poll, &invocation, observation, next_revision)?;
            changes.insert(key, self.store.encode_runtime_record(&record)?);
        }
        for (observation, definition) in contexts {
            let configuration = poll
                .session
                .runtime_binding
                .as_ref()
                .and_then(|b| b.agent_configuration.as_ref())
                .ok_or_else(|| err("blocked", "缺少原 Agent 身份"))?;
            let origin = &rows[&ResourceKey::new("agent_scope", &configuration.agent_instance_id)];
            let value = origin
                .value
                .as_ref()
                .ok_or_else(|| err("not_found", "原 Agent 实例身份缺失"))?;
            require(
                value["agent_revision_id"] == configuration.agent_revision_id,
                "上下文调用改变了原 Agent 绑定",
            )?;
            let scope: Scope = serde_json::from_value(value["scope"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            let record =
                self.provider_context_record(poll, observation, definition, &scope, next_revision)?;
            let key = ResourceKey::new("governance", &record.id);
            let previous = if let Some(value) = changes.get(&key) {
                Some(self.store.restore_runtime_record(value)?)
            } else {
                rows[&key]
                    .value
                    .as_ref()
                    .map(|r| self.store.restore_runtime_record(r))
                    .transpose()?
            };
            if let Some(previous) = previous {
                require(
                    previous["type"] == "agent_context_call"
                        && previous["data"]["invocation"] == record.data["invocation"]
                        && previous["data"]["request_id"] == record.data["request_id"],
                    "上下文调用身份被不同原请求复用",
                )?;
                continue;
            }
            changes.insert(key, self.store.encode_runtime_record(&record)?);
        }
        let result = checkpoint(&turn);
        // Even a duplicate-only batch gets its own immutable checkpoint receipt;
        // later recovery uses that original continuation rather than rebasing.
        let ack_key = ResourceKey::new(
            "governance",
            format!(
                "provider-batch:{}",
                configuration_hash(&(command, fingerprint))
            ),
        );
        let scope = if poll.session.scope == Some(SessionScope::Global) {
            Scope::global()
        } else {
            Scope::project(&poll.turn.project_id)
        };
        let mut ack = governance::record(
            "provider_observation_batch",
            poll.turn.id.clone(),
            scope,
            "原生观察接纳".into(),
            1,
            None,
            json!({"poll_id":poll.journal.as_ref().map(|r| &r.resource.id),"basis":poll.basis(),"checkpoint":result,"fingerprint_digest":fingerprint}),
            true,
        );
        ack.id = ack_key.id.clone();
        guards.insert(ack_key.clone(), 0);
        changes.insert(ack_key, ack.to_storage());
        self.store
            .commit_changes(&PreparedChangeSet {
                command_id: command.into(),
                fingerprint: fingerprint.into(),
                read_set: ReadSet::new(
                    guards
                        .into_iter()
                        .map(|(resource, revision)| ResourceVersion { resource, revision })
                        .collect(),
                )?,
                changes: changes
                    .into_iter()
                    .map(|(resource, value)| ResourceChange { resource, value })
                    .chain(event_changes)
                    .collect(),
                result,
                time: now() as i64,
            })
            .map(|r| r.result)
    }
}
