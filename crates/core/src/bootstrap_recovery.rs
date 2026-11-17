//! Opening a dataset publishes only missing configuration sources and marks
//! disconnected original Turns unknown. It never reissues external operations.
use crate::{
    concurrency::*, governance::record, platform_facts::one, workflow_resources::version, *,
};
impl Engine {
    pub fn bootstrap(&self) -> Result<()> {
        let before = self.store.configuration_snapshot()?;
        if before
            .state
            .projects
            .iter()
            .any(|p| p.execution_mode != "live")
        {
            return Ok(());
        }
        let mut next = before.state.as_ref().clone();
        seed_configuration(&mut next);
        let candidate =
            ConfigurationCatalogSnapshot::new(next, before.revision, before.repository_ready)?;
        let publication =
            prepare_configuration_publication(self.store.as_ref(), &before, &candidate)?;
        if !publication.changes.is_empty() {
            let fingerprint = configuration_hash(&(
                "configuration-bootstrap/v1",
                before.revision,
                &candidate.fingerprint,
            ));
            self.store.commit_changes(&PreparedChangeSet {
                command_id: format!("configuration-bootstrap:{fingerprint}"),
                fingerprint,
                read_set: publication.read_set,
                changes: publication.changes,
                result: json!({"seeded":true}),
                time: now() as i64,
            })?;
        }
        if matches!(self.store.namespace(), StoreNamespace::Configuration) {
            return Ok(());
        }
        let startup = id();
        let mut after = None;
        loop {
            let page = self.store.conversation_page(false, after.as_deref())?;
            for id in &page.ids {
                let turn = one(self.store.as_ref(), "turn", id)?;
                let Some(raw) = &turn.value else {
                    continue;
                };
                if !matches!(
                    raw["status"].as_str(),
                    Some("dispatching" | "running" | "waiting_interaction")
                ) {
                    continue;
                }
                let session = one(self.store.as_ref(), "session", &text(raw, "session_id")?)?;
                let Some(value) = &session.value else {
                    return Err(err("storage", "启动核对原会话缺失"));
                };
                let mut next = raw.clone();
                next["status"] = json!("needs_reconciliation");
                next["revision"] = json!(turn.revision + 1);
                let mut owner = value.clone();
                owner["lifecycle"] = json!(SessionLifecycle::NeedsReconciliation);
                owner["revision"] = json!(session.revision + 1);
                let mut observation = record(
                    "startup_runtime_observation",
                    id.clone(),
                    if value["scope"] == "global" {
                        Scope::global()
                    } else {
                        Scope::project(text(raw, "project_id")?)
                    },
                    "原进程断开后的未知执行状态".into(),
                    1,
                    None,
                    json!({"startup_id":startup,"turn_version":version(&turn),"session_version":version(&session),"outcome":"unknown","issuer":"kernel_lifecycle"}),
                    true,
                );
                observation.id = format!("startup-observation:{startup}:{id}");
                let key = ResourceKey::new("governance", &observation.id);
                self.store.commit_changes(&PreparedChangeSet {
                    command_id: observation.id.clone(),
                    fingerprint: configuration_hash(&observation.data),
                    read_set: ReadSet::new(vec![
                        version(&turn),
                        version(&session),
                        ResourceVersion {
                            resource: key.clone(),
                            revision: 0,
                        },
                    ])?,
                    changes: vec![
                        ResourceChange {
                            resource: turn.resource,
                            value: next,
                        },
                        ResourceChange {
                            resource: session.resource,
                            value: owner,
                        },
                        ResourceChange {
                            resource: key,
                            value: observation.to_storage(),
                        },
                    ],
                    result: json!({"id":id}),
                    time: now() as i64,
                })?;
            }
            match page.next_after {
                Some(id) => after = Some(id),
                None => break,
            }
        }
        self.recover_tool_dispatches()?;
        self.recover_agent_context_calls()?;
        Ok(())
    }
}
