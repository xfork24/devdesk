//! Conversation commands only persist intent. Runtime I/O belongs to the worker coordinator.
use crate::*;

fn check_revision(expected: Option<u64>, actual: u64) -> Result<()> {
    if expected == Some(actual) {
        Ok(())
    } else {
        Err(err(
            "revision_conflict",
            format!("当前 revision={actual}，请刷新后重试"),
        ))
    }
}
fn session_instance<'a>(state: &'a EngineeringState, session: &'a Session) -> Option<&'a str> {
    session
        .runtime_binding
        .as_ref()
        .and_then(|b| b.agent_configuration.as_ref())
        .map(|a| a.agent_instance_id.as_str())
        .or_else(|| {
            state
                .governance
                .iter()
                .find(|r| r.kind == "agent_session" && r.definition_id == session.id)
                .and_then(|r| r.data["agent_instance_id"].as_str())
        })
}
fn session_owner(session: &Session) -> Result<MaterialOwner> {
    match session
        .scope
        .ok_or_else(|| err("blocked", "历史 Run Session 不接受讨论输入"))?
    {
        SessionScope::Global => Ok(MaterialOwner::Global),
        SessionScope::Project => Ok(MaterialOwner::Project {
            id: session.project_id.clone().unwrap(),
        }),
        SessionScope::Feature => Ok(MaterialOwner::Feature {
            id: session.feature_id.clone().unwrap(),
        }),
        SessionScope::Task => Ok(MaterialOwner::Task {
            feature: session.feature_id.clone().unwrap(),
            id: session.task_id.clone().unwrap(),
        }),
    }
}
impl Engine {
    pub(crate) fn conversation_command(
        &self,
        method: &str,
        command: &Command,
        s: &mut EngineeringState,
    ) -> Result<Option<Value>> {
        let p = &command.payload;
        let target = command.target.as_deref().unwrap_or("");
        let result = match method {
            "session.open" => {
                let (project, feature, task, scope) = if target == "global" {
                    require(s.projects.is_empty(), "全局讨论必须使用独立的全局讨论库")?;
                    (
                        GLOBAL_DISCUSSION_NAMESPACE.into(),
                        None,
                        None,
                        SessionScope::Global,
                    )
                } else if let Some(project) = s.projects.iter().find(|v| v.id == target) {
                    (project.id.clone(), None, None, SessionScope::Project)
                } else if let Some(feature) = s.features.iter().find(|v| v.id == target) {
                    (
                        feature.project_id.clone(),
                        Some(feature.id.clone()),
                        None,
                        SessionScope::Feature,
                    )
                } else if let Some(task) = s.tasks.iter().find(|v| v.id == target) {
                    (
                        s.feature(&task.feature_id)?.project_id.clone(),
                        Some(task.feature_id.clone()),
                        Some(task.id.clone()),
                        SessionScope::Task,
                    )
                } else {
                    return Err(err("not_found", "Session owner 不存在"));
                };
                let agent = text(p, "agent")?;
                require(
                    s.definitions
                        .iter()
                        .any(|d| d.id == agent && d.agent_profile.is_some()),
                    "需要明确 Agent 角色修订",
                )?;
                let binding: RuntimeBinding = serde_json::from_value(p["binding"].clone())
                    .map_err(|_| err("invalid_params", "缺少完整运行时绑定"))?;
                require(
                    binding.work_type != WorkType::Implementation || scope == SessionScope::Task,
                    "代码实施必须使用 Task Session",
                )?;
                let task_open = p["workflow_step_instance_id"].is_string()
                    && feature
                        .as_ref()
                        .and_then(|id| s.feature_configuration(id))
                        .is_some_and(|l| l.compiler_version >= 3);
                let mut snapshot = opt(p, "snapshot_id");
                if let Some(id) = &snapshot {
                    let snap = s
                        .snapshots
                        .iter()
                        .find(|v| &v.id == id)
                        .ok_or_else(|| err("not_found", "继承 Snapshot 不存在"))?;
                    require(
                        s.material_project(&material_owner(
                            snap.project_id.as_deref(),
                            snap.feature_id.as_deref(),
                            snap.task_id.as_deref(),
                        )?)? == project,
                        "Session 不能跨项目继承上下文",
                    )?;
                }
                let mut predecessor = opt(p, "predecessor");
                if let Some(id) = &predecessor {
                    require(snapshot.is_some(), "更换会话必须明确继承的 Snapshot")?;
                    require(
                        s.sessions
                            .iter()
                            .any(|v| &v.id == id && v.project_id.as_ref() == Some(&project)),
                        "后继 Session 不能跨项目",
                    )?;
                }
                let identity = s.agent_identity(&agent)?;
                if let Some(existing) = s
                    .sessions
                    .iter()
                    .rev()
                    .find(|v| {
                        v.scope == Some(scope)
                            && v.owner_id == target
                            && s.agent_identity(&v.agent)
                                .is_ok_and(|root| root == identity)
                    })
                    .filter(|v| {
                        p["force_new"] != true
                            && !task_open
                            && v.project_id.as_ref() == Some(&project)
                            && v.scope == Some(scope)
                            && !matches!(
                                v.lifecycle,
                                Some(
                                    SessionLifecycle::RequiresSuccessor | SessionLifecycle::Closed
                                )
                            )
                            && v.owner_id == target
                            && v.agent == agent
                            && opt(p, "agent_instance_id").is_none_or(|instance| {
                                s.governance.iter().any(|r| {
                                    r.kind == "agent_session"
                                        && r.definition_id == v.id
                                        && r.data["agent_instance_id"] == instance
                                })
                            })
                            && v.runtime_binding.as_ref() == Some(&binding)
                            && snapshot
                                .as_ref()
                                .is_none_or(|id| v.snapshot_id.as_ref() == Some(id))
                            && predecessor.is_none()
                    })
                {
                    json!({"id":existing.id,"reused":true,"revision":existing.revision})
                } else {
                    if predecessor.is_none() && !task_open {
                        let identity = s.agent_identity(&agent)?;
                        if let Some(previous) = s
                            .sessions
                            .iter()
                            .rev()
                            .find(|v| {
                                v.scope == Some(scope)
                                    && v.owner_id == target
                                    && opt(p, "agent_instance_id").is_none_or(|instance| {
                                        s.governance.iter().any(|r| {
                                            r.kind == "agent_session"
                                                && r.definition_id == v.id
                                                && r.data["agent_instance_id"] == instance
                                        })
                                    })
                                    && s.agent_identity(&v.agent).is_ok_and(|id| id == identity)
                            })
                            .cloned()
                        {
                            require(
                                !s.turns
                                    .iter()
                                    .any(|t| t.session_id == previous.id && t.status.active()),
                                "请先结束或核对原会话的活动轮次",
                            )?;
                            if snapshot.is_none() {
                                snapshot = Some(self.capture_conversation(
                                    s,
                                    &previous,
                                    "conversation_handoff",
                                    None,
                                    None,
                                )?);
                            }
                            predecessor = Some(previous.id);
                        }
                    }
                    let sid = id();
                    s.sessions.push(Session {
                        id: sid.clone(),
                        owner_id: target.into(),
                        agent,
                        harness: binding.installation.id.clone(),
                        sandbox: format!("docker-image:{}", binding.installation.id),
                        snapshot_id: snapshot,
                        instance: None,
                        origin: Origin::External,
                        project_id: Some(project),
                        feature_id: feature,
                        task_id: task,
                        scope: Some(scope),
                        lifecycle: Some(SessionLifecycle::Prepared),
                        runtime_binding: Some(binding),
                        external_session: None,
                        predecessor,
                        revision: 1,
                    });
                    json!({"id":sid,"reused":false,"revision":1})
                }
            }
            "conversation.send" => {
                if ["task", "frozen_task", "task_instructions", "task_source"]
                    .iter()
                    .any(|key| p.get(key).is_some())
                {
                    return Err(err(
                        "invalid_params",
                        "任务指令与来源只能从冻结 Workflow 解析",
                    ));
                }
                if s.governance.iter().any(|r| {
                    r.kind == "permission_switch"
                        && r.definition_id == target
                        && r.data["status"] != "adopted"
                }) {
                    return Err(err(
                        "permission_switch_pending",
                        "原会话正在切换，请恢复已有切换请求",
                    ));
                }
                let session = s
                    .sessions
                    .iter()
                    .find(|v| v.id == target)
                    .cloned()
                    .ok_or_else(|| err("not_found", "Session 不存在"))?;
                let owner = session_owner(&session)?;
                require(
                    session.runtime_binding.as_ref().is_some_and(|b| {
                        [WorkType::Discussion, WorkType::Design].contains(&b.work_type)
                    }),
                    "工程专用 Session 只接受正式 Run 输入",
                )?;
                require(
                    !s.runs
                        .iter()
                        .any(|r| r.session_id == session.id && r.status == Status::Running),
                    "Session 正在执行正式 Run",
                )?;
                require(
                    !matches!(
                        session.lifecycle,
                        Some(
                            SessionLifecycle::RequiresSuccessor
                                | SessionLifecycle::Closed
                                | SessionLifecycle::NeedsReconciliation
                        )
                    ),
                    "原会话已关闭或需创建带快照的后继 Session",
                )?;
                let input = text(p, "input")?;
                let client = text(p, "client_message_id")?;
                require(
                    input.len() <= 65536 && client.len() <= 160,
                    "讨论输入或客户端消息 ID 超出限制",
                )?;
                if let Some(previous) = s
                    .turns
                    .iter()
                    .find(|t| t.session_id == session.id && t.client_message_id == client)
                {
                    require(previous.input == input, "客户端消息 ID 被不同输入复用")?;
                    return Ok(Some(
                        json!({"id":previous.id,"operation_id":previous.id,"replayed":true}),
                    ));
                }
                check_revision(command.expected_revision, session.revision)?;
                require(
                    !s.turns
                        .iter()
                        .any(|t| t.session_id == session.id && t.status.active()),
                    "Session 正忙或上个轮次等待核对",
                )?;
                let mut artifact = self.artifacts.seal_scoped(&owner, "conversation", &input)?;
                artifact.origin = Origin::Human;
                let input_digest = artifact.sha256.clone();
                let artifact_id = artifact.id.clone();
                s.artifacts.push(artifact);
                let input_snapshot = Some(self.capture_conversation(
                    s,
                    &session,
                    "conversation_input",
                    None,
                    None,
                )?);
                let source_revision = revision(s, &session.owner_id);
                let code_view = self.discussion_code_view(s, &session)?;
                let tid = id();
                let sequence = s
                    .conversation_events
                    .iter()
                    .filter(|e| e.session_id == session.id)
                    .map(|e| e.sequence)
                    .max()
                    .unwrap_or(0)
                    + 1;
                s.turns.push(ConversationTurn {
                    id: tid.clone(),
                    project_id: session.project_id.clone().unwrap(),
                    session_id: session.id.clone(),
                    client_message_id: client.clone(),
                    input: input.clone(),
                    input_digest,
                    input_snapshot,
                    code_view,
                    source_revision,
                    status: TurnStatus::Queued,
                    external_message: None,
                    external_turn: None,
                    consumed: false,
                    cancel_requested: false,
                    started: None,
                    finished: None,
                    run_id: None,
                    revision: 1,
                });
                s.conversation_events.push(ConversationEvent {
                    mode: MessageMode::Full,
                    provider_item: None,
                    id: id(),
                    session_id: session.id.clone(),
                    turn_id: tid.clone(),
                    sequence,
                    deduplication_key: format!("input:{client}"),
                    kind: ConversationEventKind::User,
                    content: input,
                    artifact_id: Some(artifact_id),
                    time: now(),
                });
                s.sessions
                    .iter_mut()
                    .find(|v| v.id == session.id)
                    .unwrap()
                    .revision += 1;
                json!({"id":tid,"operation_id":tid,"accepted":true})
            }
            "execution.retry_cleanup" => {
                let execution = s
                    .executions
                    .iter_mut()
                    .find(|e| e.id == target)
                    .ok_or_else(|| err("not_found", "运行实例不存在"))?;
                require(
                    execution.status == Status::CleanupFailed
                        && execution.writer_revoked
                        && execution.protocol_artifact.is_some(),
                    "仅已停止且已封存的清理失败实例可重试",
                )?;
                execution.status = Status::Succeeded;
                event(
                    s,
                    target,
                    "execution.cleanup_retry_requested",
                    "prior failure retained in events; runtime/image is never deleted",
                );
                json!({"id":target})
            }
            "conversation.cancel" => {
                let turn = s
                    .turns
                    .iter_mut()
                    .find(|v| v.id == target)
                    .ok_or_else(|| err("not_found", "讨论轮次不存在"))?;
                check_revision(command.expected_revision, turn.revision)?;
                require(turn.status.active(), "讨论轮次已经结束")?;
                turn.cancel_requested = true;
                if turn.status == TurnStatus::Queued {
                    turn.status = TurnStatus::Cancelled;
                    turn.finished = Some(now());
                }
                turn.revision += 1;
                for interaction in s
                    .interactions
                    .iter_mut()
                    .filter(|i| i.turn_id == target && i.status == InteractionStatus::Pending)
                {
                    interaction.status = InteractionStatus::Cancelled;
                    interaction.revision += 1;
                }
                json!({"id":target,"cancel_requested":true})
            }
            "interaction.answer" => {
                let interaction = s
                    .interactions
                    .iter()
                    .find(|i| i.id == target)
                    .cloned()
                    .ok_or_else(|| err("not_found", "交互请求不存在"))?;
                check_revision(command.expected_revision, interaction.revision)?;
                let turn = s
                    .turns
                    .iter()
                    .find(|t| t.id == interaction.turn_id)
                    .unwrap();
                require(
                    interaction.status == InteractionStatus::Pending
                        && turn.status == TurnStatus::WaitingInteraction
                        && !turn.cancel_requested,
                    "交互已结束、已取消或不再等待回答",
                )?;
                require(
                    p["session_id"].as_str() == Some(&interaction.session_id)
                        && p["turn_id"].as_str() == Some(&interaction.turn_id),
                    "回答 Session/Turn 不匹配",
                )?;
                let answer: InteractionAnswer = serde_json::from_value(p["answer"].clone())
                    .map_err(|_| err("invalid_params", "无效交互回答"))?;
                match (&answer, interaction.kind) {
                    (InteractionAnswer::ToolApproval { .. }, InteractionKind::ToolApproval) => {}
                    (
                        InteractionAnswer::UserQuestions { answers, cancelled },
                        InteractionKind::AgentQuestion,
                    ) => {
                        if *cancelled {
                            require(answers.is_empty(), "取消问题不应包含回答")?;
                        } else {
                            require(
                                answers.len() == interaction.questions.len(),
                                "必须逐项回答所有问题",
                            )?;
                            let mut seen = std::collections::HashSet::new();
                            for answer in answers {
                                require(seen.insert(&answer.id), "问题回答不可重复")?;
                                let question = interaction
                                    .questions
                                    .iter()
                                    .find(|q| q.id == answer.id)
                                    .ok_or_else(|| err("invalid_params", "问题 ID 无效"))?;
                                require(
                                    (question.multi_select || answer.selected.len() <= 1)
                                        && answer
                                            .selected
                                            .iter()
                                            .collect::<std::collections::HashSet<_>>()
                                            .len()
                                            == answer.selected.len()
                                        && answer
                                            .selected
                                            .iter()
                                            .all(|s| question.options.contains(s))
                                        && (!answer.selected.is_empty()
                                            || answer
                                                .custom
                                                .as_ref()
                                                .is_some_and(|c| !c.trim().is_empty())),
                                    "问题选项或自定义回答无效",
                                )?;
                            }
                        }
                    }
                    _ => return Err(err("invalid_params", "回答类型与交互请求不一致")),
                }
                let row = s.interactions.iter_mut().find(|i| i.id == target).unwrap();
                row.answer = Some(answer);
                row.status = InteractionStatus::Answered;
                row.revision += 1;
                s.turns
                    .iter_mut()
                    .find(|t| t.id == interaction.turn_id)
                    .unwrap()
                    .status = TurnStatus::Running;
                json!({"id":target,"accepted":true})
            }
            _ => return Ok(None),
        };
        Ok(Some(result))
    }
    pub(crate) fn capture_conversation(
        &self,
        s: &mut EngineeringState,
        session: &Session,
        kind: &str,
        documents_override: Option<&[Id]>,
        binding_override: Option<&BindingLock>,
    ) -> Result<Id> {
        let owner = session_owner(session)?;
        let binding = match &owner {
            MaterialOwner::Global => BindingLock {
                agent: session.agent.clone(),
                skills: vec![],
                standard: String::new(),
                harness: session.harness.clone(),
                sandbox: session.sandbox.clone(),
            },
            MaterialOwner::Project { id } => s
                .projects
                .iter()
                .find(|p| &p.id == id)
                .unwrap()
                .binding
                .clone(),
            MaterialOwner::Feature { id } => s.feature(id)?.binding.clone(),
            MaterialOwner::Task { id, .. } => s.task(id)?.binding.clone(),
        };
        let binding = if let Some(binding) = binding_override {
            binding.clone()
        } else if session.origin == Origin::External {
            self.live_binding(s, &session.id, &binding.standard)?
        } else {
            binding
        };
        let mut messages: Vec<_> = s
            .conversation_events
            .iter()
            .filter(|e| e.session_id == session.id)
            .rev()
            .take(100)
            .cloned()
            .collect();
        messages.reverse();
        while messages.iter().map(|e| e.content.len()).sum::<usize>() > 65536 {
            messages.remove(0);
        }
        let specification_source = s
            .governance
            .iter()
            .rev()
            .find(|r| r.kind == "execution_specification" && r.definition_id == session.id)
            .map(|r| r.data.clone());
        let specifications: Vec<_> = s
            .governance
            .iter()
            .rev()
            .find(|r| r.kind == "execution_specification" && r.definition_id == session.id)
            .into_iter()
            .flat_map(|r| r.data["spec_refs"].as_array().into_iter().flatten())
            .filter_map(|id| {
                s.definitions
                    .iter()
                    .find(|d| Some(d.id.as_str()) == id.as_str())
            })
            .collect();
        let content =
            serde_json::to_string(&json!({"specifications":specifications,"specification_source":specification_source,"source_session":session.id,"inherited_context_excerpt":session.snapshot_id.as_ref().and_then(|id|s.snapshots.iter().find(|snap|&snap.id==id)).map(|snap|json!({"snapshot_id":snap.id,"kind":snap.kind,"content_excerpt":snap.content.chars().take(16384).collect::<String>()})),"messages":messages,"turns":s.turns.iter().filter(|t|t.session_id==session.id).rev().take(50).map(|t|json!({"id":t.id,"status":t.status,"consumed":t.consumed,"cancel_requested":t.cancel_requested})).collect::<Vec<_>>()}))
                .map_err(|e| err("serialization", e.to_string()))?;
        let mut documents = match &owner {
            MaterialOwner::Global => s
                .documents
                .iter()
                .filter(|d| d.project_id.is_none() && d.feature_id.is_none() && d.task_id.is_none())
                .map(|d| d.id.clone())
                .collect(),
            MaterialOwner::Project { id } => s
                .documents
                .iter()
                .filter(|d| d.project_id.as_ref() == Some(id))
                .map(|d| d.id.clone())
                .collect(),
            MaterialOwner::Feature { id } => document_ids(s, id, None),
            MaterialOwner::Task { feature, id } => document_ids(s, feature, Some(id)),
        };
        if let Some(ids) = documents_override {
            documents = ids.to_vec();
        }
        let snapshot = Snapshot {
            execution_instances: s
                .executions
                .iter()
                .filter(|e| e.session_id == session.id)
                .map(|e| e.id.clone())
                .collect(),
            id: id(),
            project_id: session
                .project_id
                .clone()
                .filter(|_| session.scope == Some(SessionScope::Project)),
            feature_id: session.feature_id.clone(),
            task_id: session.task_id.clone(),
            kind: kind.into(),
            content,
            binding,
            documents,
            delivery_id: None,
            inherited_from: session.snapshot_id.clone(),
            capture: SnapshotCapture {
                attempt: None,
                runs: s
                    .runs
                    .iter()
                    .filter(|r| r.session_id == session.id)
                    .rev()
                    .take(50)
                    .map(|r| r.id.clone())
                    .collect(),
                sessions: vec![session.id.clone()],
                resources: vec![],
                captured_at: Some(now()),
                reason: if kind == "conversation_handoff" {
                    "Explicit successor after runtime/configuration change"
                } else if kind == "conversation_completed" {
                    "Completed discussion and sealed runtime evidence"
                } else {
                    "Frozen input before dispatch"
                }
                .into(),
            },
            runtime_bindings: session.runtime_binding.iter().cloned().collect(),
        };
        let mut artifact = self.artifacts.seal_scoped(
            &owner,
            "snapshots",
            &serde_json::to_string_pretty(&snapshot)
                .map_err(|e| err("serialization", e.to_string()))?,
        )?;
        artifact.origin = Origin::External;
        let id = snapshot.id.clone();
        s.artifacts.push(artifact);
        s.snapshots.push(snapshot);
        self.index_experience(s, &id)?;
        Ok(id)
    }
    pub(crate) fn conversation_query(&self, method: &str, p: &Value) -> Option<Result<Value>> {
        if !["query.sessions", "query.session"].contains(&method) {
            return None;
        }
        Some((|| {
            let s = self.store.snapshot()?;
            let limit = p["limit"].as_u64().unwrap_or(50).clamp(1, 100) as usize;
            let offset = p["offset"].as_u64().unwrap_or(0) as usize;
            if method == "query.sessions" {
                let rows: Vec<_> = s
                    .sessions
                    .iter()
                    .filter(|v| {
                        v.scope.is_some()
                            && p["owner_id"]
                                .as_str()
                                .is_none_or(|owner| v.owner_id == owner)
                            && p["agent_instance_id"].as_str().is_none_or(|instance| {
                                v.runtime_binding
                                    .as_ref()
                                    .and_then(|b| b.agent_configuration.as_ref())
                                    .is_some_and(|a| a.agent_instance_id == instance)
                                    || s.governance.iter().any(|r| {
                                        r.kind == "agent_session"
                                            && r.definition_id == v.id
                                            && r.data["agent_instance_id"] == instance
                                    })
                            })
                    })
                    .map(|session| {
                        let mut value = json!(session);
                        value["agent_instance_id"] = json!(session_instance(&s, session));
                        value
                    })
                    .collect();
                return Ok(
                    json!({"sessions":rows.iter().rev().skip(offset).take(limit).collect::<Vec<_>>(),"total":rows.len()}),
                );
            }
            let sid = p["id"]
                .as_str()
                .or_else(|| p["session_id"].as_str())
                .ok_or_else(|| err("invalid_params", "需要 Session ID"))?;
            let session = s
                .sessions
                .iter()
                .find(|v| v.id == sid && v.scope.is_some())
                .ok_or_else(|| err("not_found", "讨论 Session 不存在"))?;
            if let Some(project) = p["project_id"].as_str() {
                require(
                    session.project_id.as_deref() == Some(project),
                    "Session 跨项目",
                )?;
            }
            let cursor = p["cursor"].as_u64().unwrap_or(0);
            let events: Vec<_> = s
                .conversation_events
                .iter()
                .filter(|e| e.session_id == sid && e.sequence > cursor)
                .take(limit)
                .collect();
            let next = events.last().map_or(cursor, |e| e.sequence);
            let turns: Vec<_> = s.turns.iter().filter(|t| t.session_id == sid).collect();
            let output_contract = s.governance.iter().find(|r| r.kind == "execution_task_binding" && r.definition_id == sid)
                .and_then(|r| serde_json::from_value::<WorkflowTaskDefinition>(r.data["definition"].clone()).ok()
                    .and_then(|definition| definition.decode_step().ok())
                    .map(|step| agent_task_output_contract(&step, r.data["recipe_node"].as_str())));
            Ok(
                json!({"session":session,"output_contract":output_contract,"agent_instance_id":session_instance(&s,session),"turns":turns.iter().rev().skip(offset).take(limit).collect::<Vec<_>>(),"total_turns":turns.len(),"events":events,"next_cursor":next,"interactions":s.interactions.iter().filter(|i|i.session_id==sid).rev().take(limit).collect::<Vec<_>>(),"proposals":s.proposals.iter().filter(|v|v.session_id==sid).rev().take(limit).collect::<Vec<_>>(),"executions":s.executions.iter().filter(|e|e.session_id==sid).rev().take(limit).collect::<Vec<_>>()}),
            )
        })())
    }
}
