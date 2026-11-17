use crate::{
    concurrency::*,
    governance::{record, update_record},
    platform_facts::{one, restored},
    workflow_resources::version,
    *,
};
impl Engine {
    pub fn stop_owned_runtime(&self) -> Result<()> {
        if !matches!(
            self.store.namespace(),
            crate::concurrency::StoreNamespace::Engineering { .. }
        ) {
            return Ok(());
        }
        let mut after = None;
        let mut failure = None;
        loop {
            let page = self.store.conversation_page(false, after.as_deref())?;
            for tid in &page.ids {
                let Some(facts) = self.store.conversation_runtime_facts(tid)? else {
                    continue;
                };
                let result = self.finish_conversation(
                    &facts.session,
                    tid,
                    facts.turn.revision,
                    if facts.turn.cancel_requested {
                        TurnStatus::Cancelled
                    } else {
                        TurnStatus::NeedsReconciliation
                    },
                    "host_shutdown_stopped_writer_outcome_unknown",
                    true,
                );
                if let Err(error) = result
                    && failure.is_none()
                {
                    failure = Some(error);
                }
            }
            match page.next_after {
                Some(cursor) => after = Some(cursor),
                None => break,
            }
        }
        for result in [
            self.stop_owned_external_tools(),
            self.stop_owned_platform_tests(),
            self.resume_worker_cleanups(),
        ] {
            if let Err(error) = result
                && failure.is_none()
            {
                failure = Some(error);
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    fn stop_owned_external_tools(&self) -> Result<()> {
        let Some(executor) = self.tool_executor.as_deref() else {
            return Ok(());
        };
        for status in ["prepared", "dispatched", "needs_reconciliation"] {
            let mut after = None;
            loop {
                let page = self
                    .store
                    .runtime_record_page("tool_call", status, after.as_deref())?;
                for id in page.ids {
                    let selected = one(self.store.as_ref(), "governance", &id)?;
                    let row = restored(self, &selected)?;
                    if row.data["cancel_requested"] != true {
                        let mut next = row.clone();
                        let mut data = row.data.clone();
                        data["cancel_requested"] = json!(true);
                        if status == "prepared" && data["dispatch_id"].is_null() {
                            data["status"] = json!("cancelled");
                            data["remote_effect"] = json!("not_applied");
                            data["local_rejection"] =
                                json!(err("cancelled", "Host 关闭前原工具尚未派发"));
                        }
                        let mut origin = record(
                            "runtime_shutdown_intent",
                            row.id.clone(),
                            row.scope.clone(),
                            "Host 关闭原工具执行资格".into(),
                            1,
                            None,
                            json!({"basis":version(&selected),"issuer":"kernel_lifecycle"}),
                            true,
                        );
                        origin.id = format!("shutdown-tool:{}:{}", id, selected.revision);
                        data["control_context"] = json!(origin.id);
                        update_record(&mut next, data);
                        self.store.commit_changes(&PreparedChangeSet {
                            command_id: origin.id.clone(),
                            fingerprint: configuration_hash(&origin.data),
                            read_set: ReadSet::new(vec![
                                version(&selected),
                                ResourceVersion {
                                    resource: ResourceKey::new("governance", &origin.id),
                                    revision: 0,
                                },
                            ])?,
                            changes: vec![
                                ResourceChange {
                                    resource: selected.resource,
                                    value: self.store.encode_runtime_record(&next)?,
                                },
                                ResourceChange {
                                    resource: ResourceKey::new("governance", &origin.id),
                                    value: origin.to_storage(),
                                },
                            ],
                            result: json!({"id":id}),
                            time: now() as i64,
                        })?;
                    }
                    let call = crate::tool_calls::ToolCall::read(self.store.as_ref(), &id)?;
                    if !call.ended()
                        && let Some(stop) = call.claim_cancel(self.store.as_ref())?
                    {
                        let observed = executor.cancel(stop.invocation());
                        stop.observe(self.store.as_ref(), &observed)?;
                    }
                }
                match page.next_after {
                    Some(next) => after = Some(next),
                    None => break,
                }
            }
        }
        Ok(())
    }
}
