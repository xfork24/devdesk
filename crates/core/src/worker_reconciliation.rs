//! Explicit lifecycle recovery receives a distinct once-only stop grant.
//! Recovery of that grant inspects its original worker and never resends it.
use crate::{
    concurrency::*,
    governance::record,
    platform_facts::{one, restored},
    workflow_resources::version,
    *,
};
impl Engine {
    pub(crate) fn reconcile_authorized_workers(
        &self,
        session: &Session,
        tid: &str,
        authorization: &ResourceSnapshot,
        basis: &ReadSet,
    ) -> Result<()> {
        require(
            session.project_id.as_deref() == self.store.project_id().as_deref(),
            "worker 核对不能跨项目",
        )?;
        let auth = authorization
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "显式核对授权缺失"))?;
        require(
            matches!(
                auth["type"].as_str(),
                Some("workflow_reconciliation_attempt" | "session_reconciliation_attempt")
            ),
            "缺少实际生命周期核对授权",
        )?;
        let scope = session
            .project_id
            .as_deref()
            .map(Scope::project)
            .unwrap_or_else(Scope::global);
        let original = one(self.store.as_ref(), "session_live_workers", &session.id)?;
        let list = original
            .value
            .as_ref()
            .and_then(Value::as_array)
            .ok_or_else(|| err("storage", "原 worker 集合缺失"))?;
        require(list.len() <= 64, "核对 worker 超过预算")?;
        let original_ids = list
            .iter()
            .map(|v| text(v, "id"))
            .collect::<Result<std::collections::BTreeSet<_>>>()?;
        for raw in list {
            let workers = one(self.store.as_ref(), "session_live_workers", &session.id)?;
            let current = workers
                .value
                .as_ref()
                .and_then(Value::as_array)
                .ok_or_else(|| err("storage", "当前 worker 集合缺失"))?;
            require(
                current
                    .iter()
                    .all(|w| w["id"].as_str().is_some_and(|id| original_ids.contains(id))),
                "核对期间出现新的 worker；不能扩张原授权",
            )?;
            let worker = one(self.store.as_ref(), "execution", &text(raw, "id")?)?;
            let actual: RuntimeExecution = serde_json::from_value(
                self.store.restore_runtime_value(
                    worker
                        .value
                        .as_ref()
                        .ok_or_else(|| err("not_found", "原 worker 缺失"))?,
                )?,
            )
            .map_err(|e| err("storage", e.to_string()))?;
            require(actual.session_id == session.id, "核对不能替换实际 worker")?;
            if actual.writer_revoked {
                continue;
            }
            let digest = configuration_hash(&(
                "explicit-worker-reconciliation/v1",
                self.store.namespace(),
                self.store.root(),
                &authorization.resource,
                &actual.id,
                &actual.operation_id,
            ));
            let key = ResourceKey::new("governance", format!("worker-reconciliation:{digest}"));
            let previous = one(self.store.as_ref(), "governance", &key.id)?;
            let mut grant = record(
                "worker_reconciliation_attempt",
                actual.id.clone(),
                scope.clone(),
                "显式核对原 worker 生命周期".into(),
                1,
                None,
                json!({"authorization_id":authorization.resource.id,"worker_basis":version(&worker),"session":session,"worker":actual,"turn_id":tid,"origin":{"kind":"runtime_coordinator","authorization_id":authorization.resource.id}}),
                true,
            );
            grant.id = key.id.clone();
            let fresh = if previous.value.is_some() {
                grant = restored(self, &previous)?;
                require(
                    grant.r#type == "worker_reconciliation_attempt"
                        && grant.data["authorization_id"] == authorization.resource.id
                        && grant.data["worker"]["id"] == actual.id
                        && grant.data["worker"]["operation_id"] == actual.operation_id,
                    "原核对不能换 worker 或授权",
                )?;
                false
            } else {
                self.store
                    .submit_changes(&PreparedChangeSet {
                        command_id: key.id.clone(),
                        fingerprint: configuration_hash(&grant.data),
                        read_set: basis.merge([
                            version(authorization),
                            version(&workers),
                            version(&worker),
                            version(&previous),
                        ])?,
                        changes: vec![ResourceChange {
                            resource: key,
                            value: self.store.encode_runtime_record(&grant)?,
                        }],
                        result: json!({"id":grant.id}),
                        time: now() as i64,
                    })?
                    .applied
            };
            let frozen: RuntimeExecution = serde_json::from_value(grant.data["worker"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            let frozen_session: Session = serde_json::from_value(grant.data["session"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            let first = ResourceKey::new(
                "governance",
                format!("worker-reconciliation-observation:{digest}"),
            );
            if fresh {
                self.journal_reconciled_worker_stop(
                    &grant,
                    &first,
                    self.harness.stop_session(&frozen_session, &frozen),
                )?;
            }
            let mut observed = one(self.store.as_ref(), "governance", &first.id)?;
            let positive = |row: &ResourceSnapshot| {
                row.value
                    .as_ref()
                    .and_then(|v| {
                        serde_json::from_value::<Result<ports::WorkerStop>>(
                            v["data"]["value"]["result"].clone(),
                        )
                        .ok()
                    })
                    .is_some_and(|r| {
                        r.is_ok_and(|p| p.operation_id == frozen.operation_id && p.writer_revoked)
                    })
            };
            if !positive(&observed) {
                let inspected = ResourceKey::new(
                    "governance",
                    format!("worker-reconciliation-inspected:{digest}"),
                );
                let saved = one(self.store.as_ref(), "governance", &inspected.id)?;
                if saved.value.is_none() {
                    let proof = self
                        .harness
                        .inspect_worker_stop(&frozen_session, &frozen)?
                        .ok_or_else(|| {
                            err(
                                "worker_stop_unresolved",
                                "原 worker 停止结果未确认；不会重发",
                            )
                        })?;
                    require(
                        proof.operation_id == frozen.operation_id && proof.writer_revoked,
                        "核对未证明原 worker 已撤销写权限",
                    )?;
                    self.journal_reconciled_worker_stop(&grant, &inspected, Ok(proof))?;
                }
                observed = one(self.store.as_ref(), "governance", &inspected.id)?;
            }
            let raw = observed
                .value
                .as_ref()
                .ok_or_else(|| err("worker_stop_unresolved", "原 worker 没有实际停止证明"))?;
            require(
                raw["type"] == "worker_reconciliation_stop"
                    && raw["data"]["grant_id"] == grant.id
                    && raw["data"]["worker_basis"] == grant.data["worker_basis"]
                    && positive(&observed),
                "停止观察不能更换原授权或版本",
            )?;
            let proof: Result<ports::WorkerStop> =
                serde_json::from_value(raw["data"]["value"]["result"].clone())
                    .map_err(|e| err("storage", e.to_string()))?;
            let proof = proof?;
            let original: ResourceVersion =
                serde_json::from_value(grant.data["worker_basis"].clone())
                    .map_err(|e| err("storage", e.to_string()))?;
            let accept = format!("worker-reconciliation-admit:{digest}");
            let fingerprint = configuration_hash(&(
                "worker-reconciliation-admit/v1",
                &grant.id,
                &observed.resource,
            ));
            if self.store.replay_commit(&accept, &fingerprint)?.is_none() {
                let mut next = grant.data["worker"].clone();
                next["writer_revoked"] = json!(true);
                next["status"] = json!(Status::Released);
                next["finished"] = json!(now());
                let event = ResourceKey::new(
                    "conversation_event",
                    format!("worker-reconciliation-proof:{digest}"),
                );
                let value = ConversationEvent {
                    id: event.id.clone(),
                    session_id: session.id.clone(),
                    turn_id: tid.into(),
                    sequence: 0,
                    deduplication_key: format!("{}:stop-proof", actual.id),
                    kind: ConversationEventKind::Status,
                    content: serde_json::to_string(&proof)
                        .map_err(|e| err("serialization", e.to_string()))?,
                    artifact_id: None,
                    time: now(),
                    mode: MessageMode::Full,
                    provider_item: None,
                };
                let work = self.worker_cleanup_work(
                    &frozen_session,
                    &frozen,
                    &digest,
                    &observed.resource,
                )?;
                let work_key = ResourceKey::new("governance", &work.id);
                self.store.commit_changes(&PreparedChangeSet {
                    command_id: accept,
                    fingerprint,
                    read_set: ReadSet::new(vec![
                        original.clone(),
                        version(&observed),
                        ResourceVersion {
                            resource: event.clone(),
                            revision: 0,
                        },
                        ResourceVersion {
                            resource: work_key.clone(),
                            revision: 0,
                        },
                    ])?,
                    changes: vec![
                        ResourceChange {
                            resource: original.resource,
                            value: self.store.encode_runtime_value(&next)?,
                        },
                        ResourceChange {
                            resource: event,
                            value: json!(value),
                        },
                        ResourceChange {
                            resource: work_key,
                            value: self.store.encode_runtime_record(&work)?,
                        },
                    ],
                    result: json!({"id":actual.id,"writer_revoked":true}),
                    time: now() as i64,
                })?;
            }
            self.cleanup_worker_from_proof(&frozen_session, &frozen, &digest, &observed.resource)?;
        }
        Ok(())
    }
    fn journal_reconciled_worker_stop(
        &self,
        grant: &ConfigurationRecord,
        key: &ResourceKey,
        result: Result<ports::WorkerStop>,
    ) -> Result<()> {
        let mut row = record(
            "worker_reconciliation_stop",
            grant.definition_id.clone(),
            grant.scope.clone(),
            "原 worker 核对停止观察".into(),
            1,
            None,
            json!({"grant_id":grant.id,"worker_basis":grant.data["worker_basis"],"value":{"result":result},"issuer":"native_worker_lifecycle"}),
            true,
        );
        row.id = key.id.clone();
        self.store.commit_changes(&PreparedChangeSet {
            command_id: key.id.clone(),
            fingerprint: configuration_hash(&row.data),
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
        Ok(())
    }
}
