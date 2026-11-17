//! Human answers are delivered by the actual native worker, once per original
//! answer revision. An unknown reply never acquires another external attempt.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    provider_tools::ProviderPoll,
    *,
};

impl Engine {
    pub(crate) fn deliver_interaction_answer(
        &self,
        poll: &ProviderPoll,
        interaction: &PendingInteraction,
    ) -> Result<()> {
        poll.require_store(self.store.as_ref())?;
        require(
            interaction.status == InteractionStatus::Answered
                && interaction.answer.is_some()
                && interaction.session_id == poll.session.id
                && interaction.turn_id == poll.turn.id
                && interaction.project_id == poll.turn.project_id,
            "原生回答改变了原问题或归属",
        )?;
        let digest = configuration_hash(&(
            "interaction-reply/v1",
            self.store.namespace(),
            self.store.root(),
            &interaction.id,
            interaction.revision,
            &interaction.answer,
            &poll.worker,
        ));
        let command = format!("interaction-reply-prepare:{digest}");
        if self.store.replay_commit(&command, &digest)?.is_some() {
            return Ok(());
        }
        let Some(mut guards) = poll.authority.live(self.store.as_ref(), &poll.session)? else {
            return Ok(());
        };
        let key = ResourceKey::new("governance", format!("interaction-reply:{digest}"));
        let scope = if poll.session.scope == Some(SessionScope::Global) {
            Scope::global()
        } else {
            Scope::project(&poll.turn.project_id)
        };
        let mut intent = record(
            "interaction_reply_intent",
            interaction.id.clone(),
            scope,
            "返回原交互回答".into(),
            1,
            None,
            json!({"status":"prepared","digest":digest,"basis":poll.basis(),"interaction_revision":interaction.revision,"value":{"answer":interaction.answer},"origin":{"kind":"native_worker","session_id":poll.session.id,"turn_id":poll.turn.id,"worker_operation_id":poll.worker}}),
            false,
        );
        intent.id = key.id.clone();
        guards.extend(poll.guards()?.versions().iter().cloned());
        guards.push(ResourceVersion {
            resource: ResourceKey::new("interaction", &interaction.id),
            revision: interaction.revision,
        });
        guards.push(ResourceVersion {
            resource: key.clone(),
            revision: 0,
        });
        let outcome = self.store.submit_changes(&PreparedChangeSet {
            command_id: command,
            fingerprint: digest.clone(),
            read_set: ReadSet::new(guards.clone())?,
            changes: vec![ResourceChange {
                resource: key.clone(),
                value: intent.to_storage(),
            }],
            result: json!({"id":intent.id}),
            time: now() as i64,
        })?;
        if !outcome.applied {
            return Ok(());
        }
        let result = self.harness.answer_interaction(&poll.session, interaction);
        let observation_key = ResourceKey::new(
            "governance",
            format!("interaction-reply-observation:{digest}"),
        );
        let mut observation = record(
            "interaction_reply_observation",
            intent.id.clone(),
            intent.scope.clone(),
            "原交互回答观察".into(),
            1,
            None,
            json!({"basis":poll.basis(),"value":{"result":result}}),
            true,
        );
        observation.id = observation_key.id.clone();
        let saved = self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("interaction-reply-journal:{digest}"),
            fingerprint: configuration_hash(&observation.data),
            read_set: ReadSet::new(vec![ResourceVersion {
                resource: observation_key.clone(),
                revision: 0,
            }])?,
            changes: vec![ResourceChange {
                resource: observation_key.clone(),
                value: observation.to_storage(),
            }],
            result: json!({"id":observation.id}),
            time: now() as i64,
        })?;
        let _ = saved;
        self.admit_interaction_reply(&key, &observation_key)?;
        result
    }

    fn admit_interaction_reply(&self, key: &ResourceKey, observed_key: &ResourceKey) -> Result<()> {
        let rows = self
            .store
            .read_resources(&[key.clone(), observed_key.clone()])?;
        require(
            rows.len() == 2 && rows[0].resource == *key && rows[1].resource == *observed_key,
            "回答接纳事实不完整",
        )?;
        let mut intent = ConfigurationRecord::from_storage(
            rows[0]
                .value
                .clone()
                .ok_or_else(|| err("not_found", "原回答意图缺失"))?,
        )?;
        if intent.data["status"] != "prepared" {
            return Ok(());
        }
        let Some(observed) = &rows[1].value else {
            return Ok(());
        };
        require(
            intent.r#type == "interaction_reply_intent"
                && observed["type"] == "interaction_reply_observation"
                && observed["definition_id"] == intent.id
                && observed["data"]["basis"] == intent.data["basis"],
            "回答观察改变了原意图",
        )?;
        let result: Result<()> =
            serde_json::from_value(observed["data"]["value"]["result"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        let mut data = intent.data.clone();
        data["status"] = json!(if result.is_ok() {
            "delivered"
        } else {
            "needs_reconciliation"
        });
        data["observation_id"] = json!(observed_key.id);
        update_record(&mut intent, data);
        let digest = text(&intent.data, "digest")?;
        // This is acknowledgement of a factual delivery, not permission to
        // deliver again. A newer Turn decision must not discard the evidence.
        self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("interaction-reply-admit:{digest}"),
            fingerprint: configuration_hash(&("interaction-reply-admit/v1", &digest)),
            read_set: ReadSet::new(
                rows.iter()
                    .map(|r| ResourceVersion {
                        resource: r.resource.clone(),
                        revision: r.revision,
                    })
                    .collect(),
            )?,
            changes: vec![ResourceChange {
                resource: key.clone(),
                value: intent.to_storage(),
            }],
            result: json!({"id":intent.id,"delivered":result.is_ok()}),
            time: now() as i64,
        })?;
        Ok(())
    }

    pub(crate) fn resume_interaction_replies(&self) -> Result<()> {
        for row in self.pending_runtime_records("interaction_reply_intent", "prepared")? {
            let raw = row
                .value
                .as_ref()
                .ok_or_else(|| err("storage", "回答恢复意图缺失"))?;
            let digest = text(&raw["data"], "digest")?;
            let observation = ResourceKey::new(
                "governance",
                format!("interaction-reply-observation:{digest}"),
            );
            match self.admit_interaction_reply(&row.resource, &observation) {
                Err(e) if matches!(e.code.as_str(), "revision_conflict" | "storage_busy") => {
                    continue;
                }
                result => result?,
            }
        }
        Ok(())
    }
}
