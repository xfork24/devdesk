//! Durable intent -> external receipt -> domain projection. Provider I/O never runs in a transaction.
use crate::*;
use ports::{ConversationObservation as Obs, ProviderTerminal};
impl Engine {
    pub fn conversation_running(&self) -> Result<usize> {
        self.store.active_conversation_count()
    }
    pub fn tick_conversations(&self, limit: usize) -> Result<()> {
        self.tick_conversations_with_stop(limit, &|| false)
    }
    pub(crate) fn tick_conversations_with_stop(
        &self,
        limit: usize,
        should_stop: &dyn Fn() -> bool,
    ) -> Result<()> {
        if should_stop() {
            return Ok(());
        }
        if !matches!(
            self.store.namespace(),
            crate::concurrency::StoreNamespace::Engineering { .. }
        ) {
            return Ok(());
        }
        self.drive_conversation_captures()?;
        self.resume_interaction_replies()?;
        self.resume_worker_cleanups()?;
        self.resume_sealed_commands()?;
        self.resume_provider_polls()?;
        self.drive_provider_tool_intents()?;
        let after = self
            .conversation_active_cursor
            .lock()
            .map_err(|e| err("storage", e.to_string()))?
            .clone();
        let mut page = self.store.conversation_page(false, after.as_deref())?;
        if page.ids.is_empty() && after.is_some() {
            page = self.store.conversation_page(false, None)?;
        }
        let started = std::time::Instant::now();
        let mut visited = 0;
        let mut last = None;
        for tid in &page.ids {
            if should_stop()
                || (visited > 0 && started.elapsed() >= std::time::Duration::from_millis(25))
            {
                break;
            }
            visited += 1;
            last = Some(tid.clone());
            // Persist cursor progress before work so one unresolved worker does
            // not repeatedly occupy the head of this project's active page.
            *self
                .conversation_active_cursor
                .lock()
                .map_err(|e| err("storage", e.to_string()))? = last.clone();
            let Some(facts) = self.store.conversation_runtime_facts(tid)? else {
                continue;
            };
            if facts.turn.status == TurnStatus::NeedsReconciliation {
                if facts.turn.cancel_requested || facts.workers.iter().any(|w| !w.writer_revoked) {
                    self.finish_conversation(
                        &facts.session,
                        tid,
                        facts.turn.revision,
                        if facts.turn.cancel_requested {
                            TurnStatus::Cancelled
                        } else {
                            TurnStatus::NeedsReconciliation
                        },
                        if facts.turn.cancel_requested {
                            "cancelled_after_reconciliation"
                        } else {
                            "recovery_stopped_writer_outcome_unknown"
                        },
                        true,
                    )?;
                }
                continue;
            }
            if facts.turn.cancel_requested {
                self.finish_conversation(
                    &facts.session,
                    tid,
                    facts.turn.revision,
                    TurnStatus::Cancelled,
                    "user_cancelled",
                    true,
                )?;
                continue;
            }
            if facts.turn.status == TurnStatus::Dispatching && facts.worker_active {
                if let Some(authority) = &facts.authority
                    && authority
                        .live(self.store.as_ref(), &facts.session)?
                        .is_none()
                {
                    continue;
                }
                let source = self.store.conversation_start_source(tid)?;
                if source.session.revision != facts.session.revision
                    || source.turn.revision != facts.turn.revision
                {
                    return Err(err("revision_conflict", "恢复准备不能替换原轮询版本"));
                }
                if self.resume_conversation_dispatch(&source)? {
                    continue;
                }
            }
            if !facts.worker_active {
                let exports = self
                    .store
                    .read_resources(&[crate::concurrency::ResourceKey::new(
                        "session_protocol_exports",
                        &facts.session.id,
                    )])?
                    .remove(0);
                if exports.value.as_ref().is_some_and(|v| v["pending"] == true) {
                    continue;
                }
                self.unknown_conversation(
                    &facts.session,
                    tid,
                    facts.turn.revision,
                    "original_worker_stopped_or_stop_pending",
                )?;
                continue;
            }
            let pending = self
                .store
                .read_resources(&[crate::concurrency::ResourceKey::new(
                    "provider_turn_pending",
                    tid,
                )])?;
            if pending.first().is_some_and(|r| r.value.is_some()) {
                continue;
            }
            let mut poll = self.prepare_provider_poll(facts)?;
            let mut local_observation_safe = true;
            let work = (|| -> Result<()> {
                for interaction in self.store.conversation_answers_pending(tid)? {
                    self.deliver_interaction_answer(&poll, &interaction)?;
                }
                let observed = self.harness.poll_session(&poll.session);
                local_observation_safe = observed.as_ref().is_ok_and(Vec::is_empty);
                self.journal_provider_poll(&mut poll, &observed)?;
                local_observation_safe |= observed.is_ok() && poll.journal.is_some();
                self.apply_provider_observations(&mut poll, &observed?)?;
                self.settle_provider_poll(&poll, "completed", None)
            })();
            if let Err(error) = work {
                if error.code == "worker_log_unavailable"
                    || (error.code == "storage_busy" && local_observation_safe)
                {
                    // Empty polls changed no provider fact. Nonempty positive
                    // polls have their original durable handoff for recovery.
                    continue;
                }
                self.record_conversation_failure(&poll.session, tid, &error)?;
                if matches!(error.code.as_str(), "revision_conflict" | "invalid_scope") {
                    return Err(error);
                }
                if ["provider_timeout", "provider_rejected"].contains(&error.code.as_str())
                    || error.execution_failure.is_some_and(|f| {
                        f.kind == ExecutionFailureKind::Technical
                            && f.effect == ExecutionFailureEffect::NotApplied
                    })
                {
                    self.finish_conversation_guarded(
                        &poll.session,
                        tid,
                        TurnStatus::Failed,
                        &error.code,
                        true,
                        poll.guards()?,
                    )?;
                } else {
                    self.unknown_conversation(&poll.session, tid, poll.turn.revision, &error.code)?;
                }
            }
        }
        *self
            .conversation_active_cursor
            .lock()
            .map_err(|e| err("storage", e.to_string()))? = if visited < page.ids.len() {
            last
        } else {
            page.next_after
        };
        let mut available = limit.saturating_sub(self.conversation_running()?);
        if available == 0 {
            return Ok(());
        }
        let after = self
            .conversation_queued_cursor
            .lock()
            .map_err(|e| err("storage", e.to_string()))?
            .clone();
        let mut page = self.store.conversation_page(true, after.as_deref())?;
        if page.ids.is_empty() && after.is_some() {
            page = self.store.conversation_page(true, None)?;
        }
        if page.ids.is_empty() {
            *self
                .conversation_queued_cursor
                .lock()
                .map_err(|e| err("storage", e.to_string()))? = None;
            return Ok(());
        }
        let started = std::time::Instant::now();
        let mut visited = 0;
        let mut last = None;
        for tid in &page.ids {
            if should_stop()
                || available == 0
                || (visited > 0 && started.elapsed() >= std::time::Duration::from_millis(25))
            {
                break;
            }
            visited += 1;
            last = Some(tid.clone());
            *self
                .conversation_queued_cursor
                .lock()
                .map_err(|e| err("storage", e.to_string()))? = last.clone();
            let source = self.store.conversation_start_source(tid)?;
            if source.turn.status != TurnStatus::Queued {
                continue;
            }
            if self.start_conversation(source, limit)? {
                available -= 1;
            }
        }
        *self
            .conversation_queued_cursor
            .lock()
            .map_err(|e| err("storage", e.to_string()))? = if visited < page.ids.len() {
            last
        } else {
            page.next_after
        };
        Ok(())
    }
    fn record_conversation_failure(
        &self,
        session: &Session,
        tid: &str,
        error: &Error,
    ) -> Result<()> {
        self.journal_conversation_failure(session, tid, error)
    }
    pub(crate) fn unknown_conversation(
        &self,
        session: &Session,
        tid: &str,
        revision: u64,
        reason: &str,
    ) -> Result<()> {
        let reason = format!("needs_reconciliation: {reason}; input will not be resent");
        self.commit_conversation_transition(
            &crate::conversation_observations::ConversationTransition {
                session,
                turn: tid,
                revision,
                status: TurnStatus::NeedsReconciliation,
                reason: &reason,
                terminal: false,
                forced: true,
            },
            &[],
        )?;
        Ok(())
    }
    pub(crate) fn apply_provider_observations(
        &self,
        poll: &mut crate::provider_tools::ProviderPoll,
        observations: &[Obs],
    ) -> Result<()> {
        let mut pending = Vec::new();
        let mut terminal = false;
        for observation in observations {
            require(!terminal, "终态之后不能继续接纳供应商观察")?;
            if let Obs::Terminal {
                turn,
                outcome,
                reason,
                quiescent,
            } = observation
            {
                self.commit_conversation_batch(poll, &pending)?;
                pending.clear();
                require(
                    turn == &poll.turn.id && *quiescent && poll.turn.consumed,
                    "终态缺少原输入消费或静止边界",
                )?;
                let status = if poll.turn.cancel_requested {
                    TurnStatus::Cancelled
                } else {
                    match outcome {
                        ProviderTerminal::Completed => TurnStatus::Completed,
                        ProviderTerminal::Failed => TurnStatus::Failed,
                        ProviderTerminal::Cancelled => TurnStatus::Cancelled,
                    }
                };
                self.finish_conversation_guarded(
                    &poll.session,
                    turn,
                    status,
                    reason,
                    false,
                    poll.guards()?,
                )?;
                poll.turn.status = status;
                poll.turn.revision += 1;
                poll.session.revision += 1;
                terminal = true;
            } else {
                pending.push(observation.clone());
                if pending.len() >= 64 {
                    self.commit_conversation_batch(poll, &pending)?;
                    pending.clear();
                }
            }
        }
        self.commit_conversation_batch(poll, &pending)
    }
    pub(crate) fn commit_conversation_batch(
        &self,
        poll: &mut crate::provider_tools::ProviderPoll,
        observations: &[Obs],
    ) -> Result<()> {
        let mut offset = 0;
        while offset < observations.len() {
            let native = self.native_provider_observation(poll, &observations[offset]);
            let end = if native {
                offset
                    + observations[offset..]
                        .iter()
                        .take_while(|o| self.native_provider_observation(poll, o))
                        .count()
            } else {
                offset + 1
            };
            self.commit_conversation_segment(poll, &observations[offset..end], native)?;
            offset = end;
        }
        Ok(())
    }
    fn commit_conversation_segment(
        &self,
        poll: &mut crate::provider_tools::ProviderPoll,
        observations: &[Obs],
        native: bool,
    ) -> Result<()> {
        if observations.is_empty() {
            return Ok(());
        }
        poll.require_store(self.store.as_ref())?;
        let journal = poll
            .journal
            .as_ref()
            .ok_or_else(|| err("storage", "供应商事实尚未保留"))?;
        let command = format!(
            "provider-observe-batch:{}:{}",
            journal.resource.id, poll.batch
        );
        let fingerprint =
            configuration_hash(&("provider-observe-batch/v1", poll.basis(), observations));
        let result = if let Some(receipt) = self.store.replay_commit(&command, &fingerprint)? {
            receipt.result
        } else if native {
            self.admit_provider_observations(poll, observations, &command, &fingerprint)?
        } else {
            self.admit_platform_provider_observation(poll, observations, &command, &fingerprint)?
        };
        poll.turn.revision = result["turn_revision"]
            .as_u64()
            .ok_or_else(|| err("storage", "观察接纳缺少原轮次后继版本"))?;
        poll.turn.status = serde_json::from_value(result["status"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        poll.turn.external_message = serde_json::from_value(result["external_message"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        poll.turn.external_turn = serde_json::from_value(result["external_turn"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        poll.turn.consumed = result["consumed"] == true;
        poll.turn.cancel_requested = result["cancel_requested"] == true;
        poll.batch += 1;
        self.drive_provider_tool_intents()?;
        self.drive_agent_context_calls()
    }
    pub(crate) fn finish_conversation(
        &self,
        session: &Session,
        tid: &str,
        revision: u64,
        status: TurnStatus,
        reason: &str,
        forced: bool,
    ) -> Result<()> {
        let guards = crate::concurrency::ReadSet::new(vec![
            crate::concurrency::ResourceVersion {
                resource: crate::concurrency::ResourceKey::new("session", &session.id),
                revision: session.revision,
            },
            crate::concurrency::ResourceVersion {
                resource: crate::concurrency::ResourceKey::new("turn", tid),
                revision,
            },
        ])?;
        self.finish_conversation_guarded(session, tid, status, reason, forced, guards)
    }
    fn finish_conversation_guarded(
        &self,
        session: &Session,
        tid: &str,
        status: TurnStatus,
        reason: &str,
        forced: bool,
        guards: crate::concurrency::ReadSet,
    ) -> Result<()> {
        let revision = guards
            .require(&crate::concurrency::ResourceKey::new("tool_turn", tid))
            .or_else(|_| guards.require(&crate::concurrency::ResourceKey::new("turn", tid)))?;
        let transition = crate::conversation_observations::ConversationTransition {
            session,
            turn: tid,
            revision,
            status,
            reason,
            terminal: true,
            forced,
        };
        let fingerprint = transition.fingerprint(self.store.as_ref());
        if self
            .store
            .replay_commit(
                &format!("conversation-transition:{fingerprint}"),
                &fingerprint,
            )?
            .is_some()
        {
            return Ok(());
        }
        let stopped = self.stop_conversation_workers(session, tid, &guards)?;
        let mut final_versions = guards
            .versions()
            .iter()
            .filter(|v| v.resource.kind != "tool_worker")
            .cloned()
            .collect::<Vec<_>>();
        final_versions.extend(stopped.versions().iter().cloned());
        let final_guards = crate::concurrency::ReadSet::new(final_versions)?;
        self.commit_conversation_transition(&transition, final_guards.versions())?;
        Ok(())
    }
}
