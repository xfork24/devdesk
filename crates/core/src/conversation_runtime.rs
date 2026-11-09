//! Durable intent -> external receipt -> domain projection. Provider I/O never runs in a transaction.
use crate::*;
use ports::{ConversationObservation as Obs, ProviderTerminal, SessionStart, TurnDispatch};
fn owner(s: &Session) -> MaterialOwner {
    match s.scope.unwrap() {
        SessionScope::Global => MaterialOwner::Global,
        SessionScope::Project => MaterialOwner::Project {
            id: s.project_id.clone().unwrap(),
        },
        SessionScope::Feature => MaterialOwner::Feature {
            id: s.feature_id.clone().unwrap(),
        },
        SessionScope::Task => MaterialOwner::Task {
            feature: s.feature_id.clone().unwrap(),
            id: s.task_id.clone().unwrap(),
        },
    }
}
fn append(
    s: &mut EngineeringState,
    session: &Session,
    turn: &str,
    key: &str,
    kind: ConversationEventKind,
    content: &str,
    mode: MessageMode,
    item: Option<String>,
) {
    if s.conversation_events
        .iter()
        .any(|e| e.session_id == session.id && e.deduplication_key == key)
    {
        return;
    }
    let sequence = s
        .conversation_events
        .iter()
        .filter(|e| e.session_id == session.id)
        .map(|e| e.sequence)
        .max()
        .unwrap_or(0)
        + 1;
    s.conversation_events.push(ConversationEvent {
        id: id(),
        session_id: session.id.clone(),
        turn_id: turn.into(),
        sequence,
        deduplication_key: key.into(),
        kind,
        content: content.into(),
        artifact_id: None,
        time: now(),
        mode,
        provider_item: item,
    });
}
impl Engine {
    pub fn conversation_running(&self) -> Result<usize> {
        let state = self.store.snapshot()?;
        let standalone = state
            .turns
            .iter()
            .filter(|t| {
                t.run_id.is_none()
                    && t.status != TurnStatus::Queued
                    && t.status.active()
                    && state
                        .executions
                        .iter()
                        .any(|e| e.session_id == t.session_id && !e.writer_revoked)
            })
            .count();
        let stranded: std::collections::HashSet<_> = state
            .executions
            .iter()
            .filter(|e| !e.writer_revoked)
            .filter_map(|e| e.run_id.as_ref())
            .filter(|id| {
                state
                    .runs
                    .iter()
                    .any(|r| &r.id == *id && r.status != Status::Running)
            })
            .collect();
        Ok(standalone + stranded.len())
    }
    pub fn tick_conversations(&self, limit: usize) -> Result<()> {
        let state = self.store.snapshot()?;
        for turn in state
            .turns
            .iter()
            .filter(|t| t.status.active() && t.status != TurnStatus::Queued)
        {
            let session = state
                .sessions
                .iter()
                .find(|s| s.id == turn.session_id)
                .unwrap();
            if turn.status == TurnStatus::NeedsReconciliation {
                // Restart never resubmits input. Stop the previous owned writer first and retain unknown outcome.
                if turn.cancel_requested {
                    self.finish_conversation(
                        session,
                        turn.id.as_str(),
                        TurnStatus::Cancelled,
                        "cancelled_after_reconciliation",
                        true,
                    )?;
                } else if state.executions.iter().any(|e| {
                    e.kind == RuntimeExecutionKind::Harness
                        && e.session_id == session.id
                        && !e.writer_revoked
                }) {
                    // Stop + seal facts, while retaining the input's unknown outcome. Never resend it.
                    self.finish_conversation(
                        session,
                        &turn.id,
                        TurnStatus::NeedsReconciliation,
                        "recovery_stopped_writer_outcome_unknown",
                        true,
                    )?;
                }
                continue;
            }
            if turn.cancel_requested {
                self.finish_conversation(
                    session,
                    &turn.id,
                    TurnStatus::Cancelled,
                    "user_cancelled",
                    true,
                )?;
                continue;
            }
            let work = (|| -> Result<()> {
                for interaction in state
                    .interactions
                    .iter()
                    .filter(|i| i.turn_id == turn.id && i.status == InteractionStatus::Answered)
                {
                    self.harness.answer_interaction(session, interaction)?;
                }
                let observations = self.harness.poll_session(session)?;
                let mut pending = Vec::new();
                for observation in observations {
                    match observation {
                        Obs::Terminal {
                            turn: tid,
                            outcome,
                            reason,
                            quiescent,
                        } => {
                            self.commit_conversation_batch(session, &turn.id, &pending)?;
                            pending.clear();
                            require(tid == turn.id && quiescent, "终态缺少静止边界")?;
                            let current = self
                                .store
                                .snapshot()?
                                .turns
                                .into_iter()
                                .find(|t| t.id == tid)
                                .unwrap();
                            require(current.consumed, "缺少输入实际消费事实")?;
                            let status = if current.cancel_requested {
                                TurnStatus::Cancelled
                            } else {
                                match outcome {
                                    ProviderTerminal::Completed => TurnStatus::Completed,
                                    ProviderTerminal::Failed => TurnStatus::Failed,
                                    ProviderTerminal::Cancelled => TurnStatus::Cancelled,
                                }
                            };
                            self.finish_conversation(session, &tid, status, &reason, false)?;
                        }
                        other => {
                            pending.push(other);
                            if pending.len() >= 64 {
                                self.commit_conversation_batch(session, &turn.id, &pending)?;
                                pending.clear();
                            }
                        }
                    }
                }
                self.commit_conversation_batch(session, &turn.id, &pending)?;
                Ok(())
            })();
            if let Err(error) = work {
                if ["provider_timeout", "provider_rejected"].contains(&error.code.as_str()) {
                    self.finish_conversation(
                        session,
                        &turn.id,
                        TurnStatus::Failed,
                        &error.code,
                        true,
                    )?;
                } else {
                    self.unknown_conversation(session, &turn.id, &error.code)?;
                }
            }
        }
        let snapshot = self.store.snapshot()?;
        let occupied = snapshot
            .runs
            .iter()
            .filter(|r| r.status == Status::Running)
            .count()
            + self.conversation_running()?;
        let project_limit = snapshot
            .projects
            .first()
            .map(|p| p.concurrency as usize)
            .unwrap_or(limit);
        let mut available = limit.min(project_limit).saturating_sub(occupied);
        for turn in snapshot
            .turns
            .iter()
            .filter(|t| t.status == TurnStatus::Queued)
        {
            if let Some(run) = turn
                .run_id
                .as_ref()
                .and_then(|id| snapshot.runs.iter().find(|r| &r.id == id))
            {
                if run.status != Status::Running || run.cancel_requested {
                    continue;
                }
            } else {
                if available == 0 {
                    continue;
                }
                available -= 1;
            }

            let session = snapshot
                .sessions
                .iter()
                .find(|s| s.id == turn.session_id)
                .unwrap()
                .clone();
            let installation = &session.runtime_binding.as_ref().unwrap().installation;
            let image = match &installation.artifact {
                RuntimeArtifact::DockerImage { image_id } => image_id.clone(),
                _ => return Err(err("unsupported", "讨论执行缺少固定镜像")),
            };
            let execution = RuntimeExecution {
                permissions: session
                    .runtime_binding
                    .as_ref()
                    .and_then(|b| b.permissions.clone()),
                kind: RuntimeExecutionKind::Harness,
                id: id(),
                session_id: session.id.clone(),
                run_id: turn.run_id.clone(),
                operation_id: id(),
                container_id: None,
                image_digest: image,
                status: Status::Prepared,
                writer_revoked: false,
                started: Some(now()),
                finished: None,
                protocol_artifact: None,
            };
            self.store.transact(None, &mut |s| {
                let t = s.turns.iter_mut().find(|t| t.id == turn.id).unwrap();
                require(t.status == TurnStatus::Queued, "轮次已被其他调度消费")?;
                t.status = TurnStatus::Dispatching;
                t.started = Some(now());
                t.revision += 1;
                s.executions.push(execution.clone());
                s.sessions
                    .iter_mut()
                    .find(|s| s.id == session.id)
                    .unwrap()
                    .lifecycle = Some(SessionLifecycle::Active);
                Ok(json!({"ok":true}))
            })?;
            let result = (|| -> Result<()> {
                let request = SessionStart {
                    task_base: self.frozen_turn_task(&snapshot, &turn.id)?.map(|t| t.base),
                    code_view: turn.code_view.clone(),
                    session: session.clone(),
                    agent: snapshot
                        .definitions
                        .iter()
                        .find(|d| d.id == session.agent)
                        .cloned()
                        .ok_or_else(|| err("blocked", "缺少 Agent 修订"))?,
                    skills: snapshot
                        .definitions
                        .iter()
                        .filter(|d| {
                            snapshot
                                .definitions
                                .iter()
                                .find(|a| a.id == session.agent)
                                .and_then(|a| a.agent_profile.as_ref())
                                .is_some_and(|a| a.skill_revisions.contains(&d.id))
                        })
                        .cloned()
                        .collect(),
                    execution: execution.clone(),
                    project_root: self.store.root(),
                    snapshot: turn
                        .input_snapshot
                        .as_ref()
                        .or(session.snapshot_id.as_ref())
                        .and_then(|id| snapshot.snapshots.iter().find(|v| &v.id == id))
                        .cloned(),
                    documents: snapshot
                        .documents
                        .iter()
                        .filter(|d| {
                            turn.input_snapshot.as_ref().is_none_or(|id| {
                                snapshot
                                    .snapshots
                                    .iter()
                                    .find(|snap| &snap.id == id)
                                    .is_some_and(|snap| snap.documents.contains(&d.id))
                            })
                        })
                        .filter(|d| match session.scope.unwrap() {
                            SessionScope::Global => {
                                d.project_id.is_none()
                                    && d.feature_id.is_none()
                                    && d.task_id.is_none()
                            }
                            SessionScope::Project => d.project_id == session.project_id,
                            SessionScope::Feature | SessionScope::Task => snapshot
                                .document_in_feature(d, session.feature_id.as_deref().unwrap()),
                        })
                        .cloned()
                        .collect(),
                    reconciliation_turn: None,
                };
                let receipt = self.harness.open_session(&request)?;
                self.store.transact(None, &mut |s| {
                    let row = s.sessions.iter_mut().find(|s| s.id == session.id).unwrap();
                    require(
                        row.external_session
                            .as_ref()
                            .is_none_or(|id| id == &receipt.external_session),
                        "原生 Session 身份漂移",
                    )?;
                    row.external_session = Some(receipt.external_session.clone());
                    row.revision += 1;
                    let e = s
                        .executions
                        .iter_mut()
                        .find(|e| e.id == execution.id)
                        .unwrap();
                    require(
                        e.operation_id == receipt.operation_id
                            && e.image_digest == receipt.image_digest,
                        "执行回执身份不符",
                    )?;
                    e.container_id = Some(receipt.container_id.clone());
                    e.status = Status::Running;
                    Ok(json!({"ok":true}))
                })?;
                let current = self.store.snapshot()?;
                let t = current.turns.iter().find(|t| t.id == turn.id).unwrap();
                if t.cancel_requested {
                    return self.finish_conversation(
                        &session,
                        &t.id,
                        TurnStatus::Cancelled,
                        "cancelled_before_dispatch",
                        true,
                    );
                }
                let updated = current
                    .sessions
                    .iter()
                    .find(|s| s.id == session.id)
                    .unwrap();
                let specifications = turn
                    .input_snapshot
                    .as_ref()
                    .and_then(|id| snapshot.snapshots.iter().find(|s| &s.id == id))
                    .map(|s| serde_json::from_str::<Value>(&s.content))
                    .transpose()
                    .map_err(|e| err("invalid_configuration", e.to_string()))?
                    .and_then(|v| v.get("specifications").cloned())
                    .unwrap_or(json!([]));
                let specifications = serde_json::from_value(specifications)
                    .map_err(|e| err("invalid_configuration", e.to_string()))?;
                self.harness.dispatch_turn(&TurnDispatch {
                    task: self.frozen_turn_task(&snapshot, &turn.id)?,
                    specifications,
                    session: updated.clone(),
                    turn: t.clone(),
                })?;
                self.store.transact(None, &mut |s| {
                    let t = s.turns.iter_mut().find(|t| t.id == turn.id).unwrap();
                    t.status = TurnStatus::Running;
                    t.revision += 1;
                    Ok(json!({"ok":true}))
                })?;
                Ok(())
            })();
            if let Err(error) = result {
                self.unknown_conversation(&session, &turn.id, &error.code)?;
            }
        }
        Ok(())
    }
    fn unknown_conversation(&self, session: &Session, tid: &str, reason: &str) -> Result<()> {
        self.store.transact(None, &mut |s| {
            let t = s.turns.iter_mut().find(|t| t.id == tid).unwrap();
            if !t.status.active() {
                return Ok(json!({"ok":true}));
            }
            t.status = TurnStatus::NeedsReconciliation;
            t.revision += 1;
            s.sessions
                .iter_mut()
                .find(|v| v.id == session.id)
                .unwrap()
                .lifecycle = Some(SessionLifecycle::NeedsReconciliation);
            append(
                s,
                session,
                tid,
                &format!("{tid}:unknown"),
                ConversationEventKind::Status,
                &format!("needs_reconciliation: {reason}; input will not be resent"),
                MessageMode::Full,
                None,
            );
            Ok(json!({"ok":true}))
        })?;
        Ok(())
    }
    fn commit_conversation_batch(
        &self,
        session: &Session,
        turn: &str,
        observations: &[Obs],
    ) -> Result<()> {
        if observations.is_empty() {
            return Ok(());
        }
        self.store.transact(None, &mut |s| {
            for observation in observations {
                self.project_conversation(s, session, turn, observation)?;
            }
            Ok(json!({"ok":true}))
        })?;
        Ok(())
    }
    fn project_conversation(
        &self,
        s: &mut EngineeringState,
        session: &Session,
        tid: &str,
        observation: &Obs,
    ) -> Result<()> {
        let turn = s
            .turns
            .iter()
            .find(|t| t.id == tid)
            .cloned()
            .ok_or_else(|| err("not_found", "Turn 不存在"))?;
        if !turn.status.active() || turn.cancel_requested {
            return Ok(());
        }
        match observation {
            Obs::ToolCall { .. } => self.project_tool_request(s, session, tid, observation)?,
            Obs::Admitted {
                turn,
                message,
                external_turn,
            } => {
                require(turn == tid, "输入回执跨轮次")?;
                let t = s.turns.iter_mut().find(|t| t.id == tid).unwrap();
                if message.is_some() {
                    t.external_message = message.clone()
                }
                if external_turn.is_some() {
                    t.external_turn = external_turn.clone()
                }
                t.revision += 1;
            }
            Obs::Consumed { turn, message } => {
                require(turn == tid, "消费事件跨轮次")?;
                let t = s.turns.iter_mut().find(|t| t.id == tid).unwrap();
                require(
                    message
                        .as_ref()
                        .is_none_or(|id| t.external_message.as_ref() == Some(id)),
                    "原生输入 ID 不符",
                )?;
                t.consumed = true;
                t.revision += 1;
            }
            Obs::Message {
                turn,
                key,
                content,
                kind,
                mode,
                item,
            } => {
                require(
                    turn == tid && content.len() <= 1024 * 1024,
                    "消息跨轮次或过大",
                )?;
                append(
                    s,
                    session,
                    tid,
                    key,
                    *kind,
                    content,
                    *mode,
                    Some(item.clone()),
                );
            }
            Obs::Interaction {
                turn,
                request,
                kind,
                questions,
                tool,
                reason,
            } => {
                require(turn == tid, "交互请求跨轮次")?;
                if !s
                    .interactions
                    .iter()
                    .any(|i| i.session_id == session.id && i.external_request == *request)
                {
                    s.interactions.push(PendingInteraction {
                        id: id(),
                        project_id: session.project_id.clone().unwrap(),
                        session_id: session.id.clone(),
                        turn_id: tid.into(),
                        external_request: request.clone(),
                        kind: *kind,
                        status: InteractionStatus::Pending,
                        questions: questions.clone(),
                        tool: tool.clone(),
                        reason: reason.clone(),
                        answer: None,
                        revision: 1,
                    });
                }
                s.turns.iter_mut().find(|t| t.id == tid).unwrap().status =
                    TurnStatus::WaitingInteraction;
            }
            Obs::Terminal { .. } => unreachable!(),
        }
        Ok(())
    }
    fn finish_conversation(
        &self,
        session: &Session,
        tid: &str,
        status: TurnStatus,
        reason: &str,
        forced: bool,
    ) -> Result<()> {
        let state = self.store.snapshot()?;
        let executions: Vec<_> = state
            .executions
            .iter()
            .filter(|e| {
                e.session_id == session.id
                    && [Status::Prepared, Status::Running, Status::Interrupted].contains(&e.status)
            })
            .cloned()
            .collect();
        let mut results = vec![];
        for execution in executions {
            let proof = self.harness.stop_session(session, &execution)?;
            require(
                proof.operation_id == execution.operation_id && proof.writer_revoked,
                "没有停止并撤销写权限的证明",
            )?;
            let artifact = if let Some(log) = self.harness.session_log(&execution)? {
                let mut a = self
                    .artifacts
                    .seal_scoped(&owner(session), "protocol", &log)?;
                a.origin = Origin::External;
                Some(a)
            } else {
                None
            };
            results.push((execution, artifact, proof));
        }
        self.store.transact(None, &mut |s| {
            let turn = s.turns.iter_mut().find(|t| t.id == tid).unwrap();
            require(turn.status.active(), "讨论轮次已结束")?;
            // A cancellation racing with provider completion remains an explicit cancellation.
            turn.status = if turn.cancel_requested {
                TurnStatus::Cancelled
            } else {
                status
            };
            turn.finished = Some(now());
            turn.revision += 1;
            let row = s.sessions.iter_mut().find(|v| v.id == session.id).unwrap();
            row.lifecycle = Some(
                if forced || matches!(turn.status, TurnStatus::Cancelled | TurnStatus::Failed) {
                    SessionLifecycle::RequiresSuccessor
                } else {
                    SessionLifecycle::Idle
                },
            );
            row.revision += 1;
            for (execution, artifact, proof) in &results {
                if let Some(a) = artifact {
                    s.artifacts.push(a.clone())
                }
                let row = s
                    .executions
                    .iter_mut()
                    .find(|v| v.id == execution.id)
                    .unwrap();
                row.writer_revoked = true;
                row.finished = Some(now());
                row.status = if execution.run_id.is_some() {
                    Status::Succeeded
                } else {
                    Status::Released
                };
                row.protocol_artifact = artifact.as_ref().map(|a| a.id.clone());
                append(
                    s,
                    session,
                    tid,
                    &format!("{}:stop-proof", execution.id),
                    ConversationEventKind::Status,
                    &serde_json::to_string(proof).unwrap(),
                    MessageMode::Full,
                    None,
                );
            }
            for i in s
                .interactions
                .iter_mut()
                .filter(|i| i.turn_id == tid && i.status == InteractionStatus::Pending)
            {
                i.status = InteractionStatus::Cancelled;
                i.revision += 1;
            }
            append(
                s,
                session,
                tid,
                &format!("{tid}:terminal"),
                ConversationEventKind::Status,
                reason,
                MessageMode::Full,
                None,
            );
            if s.turns
                .iter()
                .any(|t| t.id == tid && t.run_id.is_none() && t.status == TurnStatus::Completed)
                && s.governance
                    .iter()
                    .any(|r| r.kind == "agent_session" && r.definition_id == session.id)
            {
                self.capture_conversation(s, session, "conversation_completed", None, None)?;
            }
            Ok(json!({"ok":true}))
        })?;
        for (execution, _, _) in results {
            if execution.run_id.is_some() {
                continue;
            }
            if let Err(error) = self.harness.cleanup_worker(session, &execution) {
                self.store.transact(None, &mut |s| {
                    s.executions
                        .iter_mut()
                        .find(|e| e.id == execution.id)
                        .unwrap()
                        .status = Status::CleanupFailed;
                    event(s, &session.id, "cleanup_failed", &error.code);
                    Ok(json!({"ok":true}))
                })?;
            }
        }
        Ok(())
    }
}
