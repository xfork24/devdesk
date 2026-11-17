//! A worker stop is a durable, once-admitted external attempt. Its original
//! proof retires only that worker, independently of later Turn decisions.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    *,
};

fn scope(session: &Session) -> Result<Scope> {
    if session.scope == Some(SessionScope::Global) {
        Ok(Scope::global())
    } else {
        Ok(Scope::project(
            session
                .project_id
                .as_ref()
                .ok_or_else(|| err("invalid_scope", "worker 缺少项目"))?,
        ))
    }
}
fn owner(session: &Session) -> Result<MaterialOwner> {
    Ok(
        match session
            .scope
            .ok_or_else(|| err("invalid_scope", "worker 缺少讨论归属"))?
        {
            SessionScope::Global => MaterialOwner::Global,
            SessionScope::Project => MaterialOwner::Project {
                id: session
                    .project_id
                    .clone()
                    .ok_or_else(|| err("invalid_scope", "缺少项目"))?,
            },
            SessionScope::Feature => MaterialOwner::Feature {
                id: session
                    .feature_id
                    .clone()
                    .ok_or_else(|| err("invalid_scope", "缺少 Feature"))?,
            },
            SessionScope::Task => MaterialOwner::Task {
                feature: session
                    .feature_id
                    .clone()
                    .ok_or_else(|| err("invalid_scope", "缺少 Feature"))?,
                id: session
                    .task_id
                    .clone()
                    .ok_or_else(|| err("invalid_scope", "缺少 Task"))?,
            },
        },
    )
}
fn version(row: &ResourceSnapshot) -> ResourceVersion {
    ResourceVersion {
        resource: row.resource.clone(),
        revision: row.revision,
    }
}

impl Engine {
    pub(crate) fn stop_conversation_workers(
        &self,
        session: &Session,
        tid: &str,
        authority: &ReadSet,
    ) -> Result<ReadSet> {
        let workers_key = ResourceKey::new("session_live_workers", &session.id);
        let rows = self
            .store
            .read_resources(std::slice::from_ref(&workers_key))?;
        require(
            rows.len() == 1 && rows[0].resource == workers_key,
            "worker 列表读取不完整",
        )?;
        let workers = rows[0]
            .value
            .as_ref()
            .and_then(Value::as_array)
            .ok_or_else(|| err("storage", "worker 列表缺失"))?;
        for raw in workers {
            let key = ResourceKey::new("execution", text(raw, "id")?);
            let worker = self
                .store
                .read_resources(std::slice::from_ref(&key))?
                .into_iter()
                .next()
                .ok_or_else(|| err("storage", "worker 事实缺失"))?;
            self.stop_original_worker(session, tid, &worker, authority)?;
        }
        let rows = self
            .store
            .read_resources(std::slice::from_ref(&workers_key))?;
        require(
            rows.len() == 1 && rows[0].resource == workers_key,
            "worker 停止谓词读取不完整",
        )?;
        if !rows[0]
            .value
            .as_ref()
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
        {
            return Err(err("revision_conflict", "停止期间出现了新的活动 worker"));
        }
        let exports = self
            .store
            .read_resources(&[ResourceKey::new("session_protocol_exports", &session.id)])?
            .remove(0);
        if exports.value.as_ref().is_some_and(|v| v["pending"] == true) {
            return Err(err(
                "worker_log_unavailable",
                "原 worker 已停止，等待只读导出协议证据",
            ));
        }
        ReadSet::new(vec![version(&rows[0]), version(&exports)])
    }

    fn stop_original_worker(
        &self,
        session: &Session,
        tid: &str,
        worker: &ResourceSnapshot,
        authority: &ReadSet,
    ) -> Result<()> {
        let raw = worker
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "原 worker 缺失"))?;
        let execution: RuntimeExecution =
            serde_json::from_value(self.store.restore_runtime_value(raw)?)
                .map_err(|e| err("storage", e.to_string()))?;
        require(
            execution.session_id == session.id
                && worker.resource == ResourceKey::new("execution", &execution.id),
            "停止请求跨原 worker",
        )?;
        if execution.writer_revoked {
            return Ok(());
        }
        let digest = configuration_hash(&(
            "worker-stop/v1",
            self.store.namespace(),
            self.store.root(),
            &execution.id,
            &execution.operation_id,
        ));
        let intent_key = ResourceKey::new("governance", format!("worker-stop:{digest}"));
        let command = format!("worker-stop-prepare:{digest}");
        let fingerprint = configuration_hash(&(
            "worker-stop-prepare/v1",
            &intent_key,
            &session.id,
            tid,
            authority,
        ));
        let mut intent = record(
            "worker_stop_intent",
            execution.id.clone(),
            scope(session)?,
            "停止原 worker".into(),
            1,
            None,
            json!({"status":"prepared","session_id":session.id,"turn_id":tid,"worker_operation_id":execution.operation_id,"basis":version(worker),"authority":authority,"origin":{"kind":"runtime_coordinator","session_id":session.id,"worker_operation_id":execution.operation_id}}),
            false,
        );
        intent.id = intent_key.id.clone();
        let mut guards = authority.versions().to_vec();
        guards.push(version(worker));
        guards.push(ResourceVersion {
            resource: intent_key.clone(),
            revision: 0,
        });
        let existing = self
            .store
            .read_resources(std::slice::from_ref(&intent_key))?;
        require(
            existing.len() == 1 && existing[0].resource == intent_key,
            "停止意图读取不完整",
        )?;
        let granted = if let Some(raw) = &existing[0].value {
            let original = ConfigurationRecord::from_storage(raw.clone())?;
            require(
                original.r#type == "worker_stop_intent"
                    && original.data["session_id"] == session.id
                    && original.data["turn_id"] == tid
                    && original.data["basis"] == json!(version(worker))
                    && original.data["worker_operation_id"] == execution.operation_id,
                "停止意图不能更改原 worker 或轮次",
            )?;
            false
        } else {
            self.store
                .submit_changes(&PreparedChangeSet {
                    command_id: command,
                    fingerprint,
                    read_set: ReadSet::new(guards)?,
                    changes: vec![ResourceChange {
                        resource: intent_key.clone(),
                        value: intent.to_storage(),
                    }],
                    result: json!({"id":intent_key.id}),
                    time: now() as i64,
                })?
                .applied
        };
        let mut observation_key =
            ResourceKey::new("governance", format!("worker-stop-observation:{digest}"));
        if granted {
            let stopped = self.harness.stop_session(session, &execution);
            // A logging failure must not erase a received revocation proof.
            let log = if stopped
                .as_ref()
                .is_ok_and(|p| p.operation_id == execution.operation_id && p.writer_revoked)
            {
                self.harness.session_log(&execution).and_then(|log| {
                    log.map(|text| {
                        self.artifacts
                            .seal_scoped(&owner(session)?, "protocol", &text)
                    })
                    .transpose()
                })
            } else {
                Ok(None)
            };
            let (artifact, log_error) = match log {
                Ok(artifact) => (
                    artifact.map(|mut a| {
                        a.origin = Origin::External;
                        a
                    }),
                    None,
                ),
                Err(error) => (None, Some(error)),
            };
            let mut observed = record(
                "worker_stop_observation",
                intent.id.clone(),
                intent.scope.clone(),
                "原 worker 停止观察".into(),
                1,
                None,
                json!({"intent_id":intent.id,"worker_basis":version(worker),"value":{"result":stopped,"artifact":artifact,"log_error":log_error}}),
                true,
            );
            observed.id = observation_key.id.clone();
            self.store.commit_changes(&PreparedChangeSet {
                command_id: format!("worker-stop-journal:{digest}"),
                fingerprint: configuration_hash(&observed.data),
                read_set: ReadSet::new(vec![ResourceVersion {
                    resource: observation_key.clone(),
                    revision: 0,
                }])?,
                changes: vec![ResourceChange {
                    resource: observation_key.clone(),
                    value: observed.to_storage(),
                }],
                result: json!({"id":observed.id}),
                time: now() as i64,
            })?;
        }
        let mut observations = self
            .store
            .read_resources(&[observation_key.clone(), intent_key.clone()])?;
        require(observations.len() == 2, "worker 停止观察读取不完整")?;
        let has_proof = observations[0]
            .value
            .as_ref()
            .and_then(|v| {
                serde_json::from_value::<Result<ports::WorkerStop>>(
                    v["data"]["value"]["result"].clone(),
                )
                .ok()
            })
            .is_some_and(|result| {
                result.is_ok_and(|p| p.operation_id == execution.operation_id && p.writer_revoked)
            });
        if !has_proof {
            // Inspect the original owned container. This read grants no stop,
            // recreation or input retry and keeps the initial failure intact.
            let reconciled_key =
                ResourceKey::new("governance", format!("worker-stop-reconciled:{digest}"));
            let known = self
                .store
                .read_resources(std::slice::from_ref(&reconciled_key))?;
            require(
                known.len() == 1 && known[0].resource == reconciled_key,
                "停止核对读取不完整",
            )?;
            if known[0].value.is_none() {
                let proof = self
                    .harness
                    .inspect_worker_stop(session, &execution)?
                    .ok_or_else(|| {
                        err(
                            "worker_stop_unresolved",
                            "原 worker 仍在运行，停止结果未确认；不能重发",
                        )
                    })?;
                require(
                    proof.operation_id == execution.operation_id && proof.writer_revoked,
                    "只读核对没有原 worker 写撤销证明",
                )?;
                let result: Result<ports::WorkerStop> = Ok(proof);
                let log = self.harness.session_log(&execution).and_then(|log| {
                    log.map(|body| {
                        self.artifacts
                            .seal_scoped(&owner(session)?, "protocol", &body)
                    })
                    .transpose()
                });
                let (artifact, log_error) = match log {
                    Ok(artifact) => (
                        artifact.map(|mut artifact| {
                            artifact.origin = Origin::External;
                            artifact
                        }),
                        None,
                    ),
                    Err(error) => (None, Some(error)),
                };
                let mut reconciled = record(
                    "worker_stop_observation",
                    intent_key.id.clone(),
                    scope(session)?,
                    "只读核对原 worker 停止".into(),
                    1,
                    None,
                    json!({"intent_id":intent_key.id,"worker_basis":version(worker),"reconciles":observation_key.id,"value":{"result":result,"artifact":artifact,"log_error":log_error}}),
                    true,
                );
                reconciled.id = reconciled_key.id.clone();
                self.store.commit_changes(&PreparedChangeSet {
                    command_id: format!("worker-stop-reconcile-journal:{digest}"),
                    fingerprint: configuration_hash(&(
                        "worker-stop-reconcile/v1",
                        &digest,
                        version(worker),
                    )),
                    read_set: ReadSet::new(vec![ResourceVersion {
                        resource: reconciled_key.clone(),
                        revision: 0,
                    }])?,
                    changes: vec![ResourceChange {
                        resource: reconciled_key.clone(),
                        value: reconciled.to_storage(),
                    }],
                    result: json!({"id":reconciled.id}),
                    time: now() as i64,
                })?;
            }
            observation_key = reconciled_key;
            observations = self
                .store
                .read_resources(&[observation_key.clone(), intent_key.clone()])?;
            require(observations.len() == 2, "停止核对接纳事实不完整")?;
        }
        let observed = observations[0].value.as_ref().ok_or_else(|| {
            err(
                "worker_stop_unresolved",
                "停止意图已有外部尝试但尚无回执，必须核对，不能重发",
            )
        })?;
        let mut intent = ConfigurationRecord::from_storage(
            observations[1]
                .value
                .clone()
                .ok_or_else(|| err("not_found", "原停止意图缺失"))?,
        )?;
        if intent.data["status"] == "completed" {
            return Ok(());
        }
        require(
            observed["type"] == "worker_stop_observation"
                && observed["data"]["intent_id"] == intent.id
                && observed["data"]["worker_basis"] == json!(version(worker)),
            "停止观察改变了原 worker 依据",
        )?;
        let stopped: Result<ports::WorkerStop> =
            serde_json::from_value(observed["data"]["value"]["result"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        let proof = stopped?;
        require(
            proof.operation_id == execution.operation_id && proof.writer_revoked,
            "没有停止原 worker 并撤销写权限的证明",
        )?;
        let artifact: Option<Artifact> =
            serde_json::from_value(observed["data"]["value"]["artifact"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        let mut next = raw.clone();
        next["writer_revoked"] = json!(true);
        next["status"] = json!(Status::Released);
        next["finished"] = json!(now());
        next["protocol_artifact"] = json!(artifact.as_ref().map(|a| &a.id));
        let event_key =
            ResourceKey::new("conversation_event", format!("worker-stop-event:{digest}"));
        let event = ConversationEvent {
            id: event_key.id.clone(),
            session_id: session.id.clone(),
            turn_id: tid.into(),
            sequence: 0,
            deduplication_key: format!("{}:stop-proof", execution.id),
            kind: ConversationEventKind::Status,
            content: serde_json::to_string(&proof)
                .map_err(|e| err("serialization", e.to_string()))?,
            artifact_id: None,
            time: now(),
            mode: MessageMode::Full,
            provider_item: None,
        };
        let mut data = intent.data.clone();
        data["status"] = json!("completed");
        data["observation_id"] = json!(observation_key.id);
        update_record(&mut intent, data);
        let work = self.worker_cleanup_work(
            session,
            &execution,
            &digest,
            &ResourceKey::new("governance", &intent.id),
        )?;
        let work_key = ResourceKey::new("governance", &work.id);
        let mut guards = vec![
            version(worker),
            version(&observations[0]),
            version(&observations[1]),
            ResourceVersion {
                resource: event_key.clone(),
                revision: 0,
            },
            ResourceVersion {
                resource: work_key.clone(),
                revision: 0,
            },
        ];
        let mut changes = vec![
            ResourceChange {
                resource: work_key,
                value: self.store.encode_runtime_record(&work)?,
            },
            ResourceChange {
                resource: worker.resource.clone(),
                value: next,
            },
            ResourceChange {
                resource: intent_key,
                value: intent.to_storage(),
            },
            ResourceChange {
                resource: event_key,
                value: json!(event),
            },
        ];
        if let Some(artifact) = artifact {
            let key = ResourceKey::new("artifact", &artifact.id);
            guards.push(ResourceVersion {
                resource: key.clone(),
                revision: 0,
            });
            changes.push(ResourceChange {
                resource: key,
                value: json!(artifact),
            });
        }
        self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("worker-stop-admit:{digest}"),
            fingerprint: configuration_hash(&(
                "worker-stop-admit/v1",
                version(worker),
                &observation_key,
            )),
            read_set: ReadSet::new(guards)?,
            changes,
            result: json!({"id":execution.id,"writer_revoked":true}),
            time: now() as i64,
        })?;
        self.cleanup_stopped_worker(session, &execution, &digest)
    }

    fn cleanup_stopped_worker(
        &self,
        session: &Session,
        execution: &RuntimeExecution,
        digest: &str,
    ) -> Result<()> {
        self.cleanup_worker_from_proof(
            session,
            execution,
            digest,
            &ResourceKey::new("governance", format!("worker-stop:{digest}")),
        )
    }
    pub(crate) fn cleanup_worker_from_proof(
        &self,
        session: &Session,
        execution: &RuntimeExecution,
        digest: &str,
        stop_key: &ResourceKey,
    ) -> Result<()> {
        let intent_key = ResourceKey::new("governance", format!("worker-cleanup:{digest}"));
        let worker_key = ResourceKey::new("execution", &execution.id);
        let mut originals = self.store.read_resources(&[
            worker_key.clone(),
            stop_key.clone(),
            intent_key.clone(),
        ])?;
        require(originals.len() == 3, "清理原事实读取不完整")?;
        if originals[2].value.is_some() {
            return self.admit_worker_cleanup(&originals[2]);
        }
        let worker = originals[0]
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "清理原 worker 缺失"))?;
        let stop = originals[1]
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "清理的实际停止证明缺失"))?;
        let normal = stop["type"] == "worker_stop_intent"
            && stop["data"]["status"] == "completed"
            && stop["definition_id"] == execution.id;
        let reconciled = if stop["type"] == "worker_reconciliation_stop"
            && stop["immutable"] == true
            && stop["definition_id"] == execution.id
        {
            serde_json::from_value::<Result<ports::WorkerStop>>(
                stop["data"]["value"]["result"].clone(),
            )
            .is_ok_and(|r| {
                r.is_ok_and(|r| r.operation_id == execution.operation_id && r.writer_revoked)
            })
        } else {
            false
        };
        require(
            worker["operation_id"] == execution.operation_id
                && worker["session_id"] == session.id
                && worker["writer_revoked"] == true
                && (normal || reconciled),
            "清理需要该实际 worker 的已接纳停止证明",
        )?;
        // Revocation is accepted independently of logging, but evidence must
        // be exported before a container can be deleted. Read-only recovery
        // never grants another stop or replays the worker's input.
        originals[0] =
            self.capture_stopped_worker_protocol(session, execution, digest, &originals[0])?;
        let worker = originals[0]
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "原 worker 缺失"))?;
        let mut intent = record(
            "worker_cleanup_intent",
            execution.id.clone(),
            scope(session)?,
            "清理已停止 worker".into(),
            1,
            None,
            json!({"status":"prepared","digest":digest,"session_id":session.id,"worker":worker,"worker_basis":version(&originals[0]),"worker_operation_id":execution.operation_id,"stop_intent_id":stop_key.id}),
            false,
        );
        intent.id = intent_key.id.clone();
        let outcome = self.store.submit_changes(&PreparedChangeSet {
            command_id: format!("worker-cleanup-prepare:{digest}"),
            fingerprint: configuration_hash(&(
                "worker-cleanup-prepare/v2",
                &execution.id,
                &execution.operation_id,
                digest,
            )),
            read_set: ReadSet::new(originals.iter().map(version).collect())?,
            changes: vec![ResourceChange {
                resource: intent_key.clone(),
                value: self.store.encode_runtime_record(&intent)?,
            }],
            result: json!({"id":intent.id}),
            time: now() as i64,
        })?;
        if outcome.applied {
            let result = self.harness.cleanup_worker(session, execution);
            let observed_key =
                ResourceKey::new("governance", format!("worker-cleanup-observation:{digest}"));
            let mut observed = record(
                "worker_cleanup_observation",
                intent.id.clone(),
                intent.scope.clone(),
                "worker 清理观察".into(),
                1,
                None,
                json!({"worker_basis":intent.data["worker_basis"],"value":{"result":result}}),
                true,
            );
            observed.id = observed_key.id.clone();
            self.store.commit_changes(&PreparedChangeSet {
                command_id: format!("worker-cleanup-journal:{digest}"),
                fingerprint: configuration_hash(&observed.data),
                read_set: ReadSet::new(vec![ResourceVersion {
                    resource: observed_key.clone(),
                    revision: 0,
                }])?,
                changes: vec![ResourceChange {
                    resource: observed_key,
                    value: observed.to_storage(),
                }],
                result: json!({"id":observed.id}),
                time: now() as i64,
            })?;
        }
        let saved = self
            .store
            .read_resources(std::slice::from_ref(&intent_key))?
            .into_iter()
            .next()
            .ok_or_else(|| err("storage", "原清理意图缺失"))?;
        self.admit_worker_cleanup(&saved)
    }

    fn capture_stopped_worker_protocol(
        &self,
        session: &Session,
        execution: &RuntimeExecution,
        digest: &str,
        worker: &ResourceSnapshot,
    ) -> Result<ResourceSnapshot> {
        if worker
            .value
            .as_ref()
            .is_some_and(|raw| raw["protocol_artifact"].is_string())
        {
            return Ok(worker.clone());
        }
        let key = ResourceKey::new(
            "governance",
            format!("worker-protocol-observation:{digest}"),
        );
        let mut observation = self
            .store
            .read_resources(std::slice::from_ref(&key))?
            .remove(0);
        if observation.value.is_none() {
            let body = self.harness.session_log(execution).map_err(|error| {
                err(
                    "worker_log_unavailable",
                    format!("原 worker 日志尚未导出，保留容器等待只读恢复：{error}"),
                )
            })?;
            let artifact = body
                .map(|body| {
                    self.artifacts
                        .seal_scoped(&owner(session)?, "protocol", &body)
                        .map(|mut artifact| {
                            artifact.origin = Origin::External;
                            artifact
                        })
                })
                .transpose()?;
            let mut row = record(
                "worker_protocol_observation",
                execution.id.clone(),
                scope(session)?,
                "已停止 worker 日志的只读导出".into(),
                1,
                None,
                json!({"worker_basis":version(worker),"worker_operation_id":execution.operation_id,"artifact":artifact}),
                true,
            );
            row.id = key.id.clone();
            self.store.commit_changes(&PreparedChangeSet {
                command_id: format!("worker-protocol-journal:{digest}"),
                fingerprint: configuration_hash(&row.data),
                read_set: ReadSet::new(vec![version(&observation)])?,
                changes: vec![ResourceChange {
                    resource: key.clone(),
                    value: row.to_storage(),
                }],
                result: json!({"id":row.id}),
                time: now() as i64,
            })?;
            observation = self
                .store
                .read_resources(std::slice::from_ref(&key))?
                .remove(0);
        }
        let raw = observation
            .value
            .as_ref()
            .ok_or_else(|| err("storage", "原日志观察缺失"))?;
        require(
            raw["type"] == "worker_protocol_observation"
                && raw["immutable"] == true
                && raw["definition_id"] == execution.id
                && raw["data"]["worker_operation_id"] == execution.operation_id
                && raw["data"]["worker_basis"] == json!(version(worker)),
            "日志导出不能替换原 worker 或版本",
        )?;
        let artifact: Option<Artifact> = serde_json::from_value(raw["data"]["artifact"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let Some(artifact) = artifact else {
            return Ok(worker.clone());
        };
        let artifact_key = ResourceKey::new("artifact", &artifact.id);
        let mut next = worker
            .value
            .clone()
            .ok_or_else(|| err("not_found", "原 worker 缺失"))?;
        next["protocol_artifact"] = json!(artifact.id);
        let receipt = self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("worker-protocol-admit:{digest}"),
            fingerprint: configuration_hash(&(
                "worker-protocol-admit/v1",
                version(worker),
                &observation.resource,
            )),
            read_set: ReadSet::new(vec![
                version(worker),
                version(&observation),
                ResourceVersion {
                    resource: artifact_key.clone(),
                    revision: 0,
                },
            ])?,
            changes: vec![
                ResourceChange {
                    resource: worker.resource.clone(),
                    value: next.clone(),
                },
                ResourceChange {
                    resource: artifact_key,
                    value: json!(artifact),
                },
            ],
            result: json!({"id":execution.id}),
            time: now() as i64,
        })?;
        let revision = receipt
            .versions
            .iter()
            .find(|v| v.resource == worker.resource)
            .ok_or_else(|| err("storage", "日志接纳缺少原 worker 版本"))?
            .revision;
        Ok(ResourceSnapshot {
            resource: worker.resource.clone(),
            revision,
            value: Some(next),
        })
    }

    fn admit_worker_cleanup(&self, original: &ResourceSnapshot) -> Result<()> {
        let mut intent = ConfigurationRecord::from_storage(
            self.store.restore_runtime_record(
                original
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "原清理意图缺失"))?,
            )?,
        )?;
        if intent.data["status"] != "prepared" {
            return Ok(());
        }
        let digest = text(&intent.data, "digest")?;
        let observed_key =
            ResourceKey::new("governance", format!("worker-cleanup-observation:{digest}"));
        let reconciliation_key =
            ResourceKey::new("governance", format!("worker-cleanup-reconciled:{digest}"));
        let rows = self
            .store
            .read_resources(&[observed_key.clone(), reconciliation_key.clone()])?;
        require(rows.len() == 2, "清理观察读取不完整")?;
        let selected = if rows[0].value.is_some() {
            Some(&rows[0])
        } else if rows[1].value.is_some() {
            Some(&rows[1])
        } else {
            None
        };
        let observed = if let Some(selected) = selected {
            selected.clone()
        } else {
            let sid = text(&intent.data, "session_id")?;
            let session_row = self
                .store
                .read_resources(&[ResourceKey::new("session", &sid)])?
                .into_iter()
                .next()
                .ok_or_else(|| err("storage", "清理核对原会话读取不完整"))?;
            let session: Session = serde_json::from_value(
                self.store.restore_runtime_value(
                    session_row
                        .value
                        .as_ref()
                        .ok_or_else(|| err("not_found", "清理核对原会话缺失"))?,
                )?,
            )
            .map_err(|e| err("storage", e.to_string()))?;
            let worker: RuntimeExecution =
                serde_json::from_value(self.store.restore_runtime_value(&intent.data["worker"])?)
                    .map_err(|e| err("storage", e.to_string()))?;
            require(
                worker.writer_revoked
                    && worker.session_id == sid
                    && intent.data["worker_operation_id"] == worker.operation_id,
                "清理核对不能更换原操作",
            )?;
            if !self.harness.inspect_worker_cleanup(&session, &worker)? {
                return Ok(());
            }
            let result: Result<()> = Ok(());
            let mut proof = record(
                "worker_cleanup_observation",
                intent.id.clone(),
                intent.scope.clone(),
                "只读核对原 worker 已清理".into(),
                1,
                None,
                json!({"worker_basis":intent.data["worker_basis"],"reconciles":observed_key.id,"value":{"result":result}}),
                true,
            );
            proof.id = reconciliation_key.id.clone();
            self.store.commit_changes(&PreparedChangeSet {
                command_id: format!("worker-cleanup-reconcile:{digest}"),
                fingerprint: configuration_hash(&("worker-cleanup-reconcile/v1", &digest)),
                read_set: ReadSet::new(vec![ResourceVersion {
                    resource: reconciliation_key.clone(),
                    revision: 0,
                }])?,
                changes: vec![ResourceChange {
                    resource: reconciliation_key.clone(),
                    value: proof.to_storage(),
                }],
                result: json!({"id":proof.id}),
                time: now() as i64,
            })?;
            self.store
                .read_resources(std::slice::from_ref(&reconciliation_key))?
                .into_iter()
                .next()
                .ok_or_else(|| err("storage", "清理核对证明缺失"))?
        };
        let proof = observed
            .value
            .as_ref()
            .ok_or_else(|| err("storage", "清理观察缺失"))?;
        require(
            proof["type"] == "worker_cleanup_observation"
                && proof["definition_id"] == intent.id
                && proof["data"]["worker_basis"] == intent.data["worker_basis"],
            "清理观察改变了原执行依据",
        )?;
        let result: Result<()> = serde_json::from_value(proof["data"]["value"]["result"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let mut data = intent.data.clone();
        data["status"] = json!(if result.is_ok() {
            "completed"
        } else {
            "failed"
        });
        data["observation_id"] = json!(observed.resource.id);
        update_record(&mut intent, data);
        let mut guards = vec![version(original), version(&observed)];
        let mut changes = vec![ResourceChange {
            resource: original.resource.clone(),
            value: self.store.encode_runtime_record(&intent)?,
        }];
        if result.is_err() {
            let basis: ResourceVersion =
                serde_json::from_value(intent.data["worker_basis"].clone())
                    .map_err(|e| err("storage", e.to_string()))?;
            let mut worker = intent.data["worker"].clone();
            worker["status"] = json!(Status::CleanupFailed);
            guards.push(basis.clone());
            changes.push(ResourceChange {
                resource: basis.resource,
                value: worker,
            });
        }
        self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("worker-cleanup-admit:{digest}"),
            fingerprint: configuration_hash(&("worker-cleanup-admit/v2", &digest)),
            read_set: ReadSet::new(guards)?,
            changes,
            result: json!({"id":intent.definition_id,"cleanup":result.is_ok()}),
            time: now() as i64,
        })?;
        Ok(())
    }

    pub(crate) fn worker_cleanup_work(
        &self,
        session: &Session,
        execution: &RuntimeExecution,
        digest: &str,
        proof: &ResourceKey,
    ) -> Result<ConfigurationRecord> {
        let mut row = record(
            "worker_cleanup_work",
            execution.id.clone(),
            scope(session)?,
            "已停止 worker 的持久清理任务".into(),
            1,
            None,
            json!({"status":"pending","session_id":session.id,"worker":execution,"digest":digest,"proof":proof}),
            false,
        );
        row.id = format!("worker-cleanup-work:{digest}");
        Ok(row)
    }
    pub(crate) fn resume_worker_cleanups(&self) -> Result<()> {
        for selected in self.pending_runtime_records("worker_cleanup_work", "pending")? {
            let mut row = ConfigurationRecord::from_storage(
                self.store.restore_runtime_record(
                    selected
                        .value
                        .as_ref()
                        .ok_or_else(|| err("storage", "清理任务缺失"))?,
                )?,
            )?;
            let sid = text(&row.data, "session_id")?;
            let actual = self
                .store
                .read_resources(&[ResourceKey::new("session", sid)])?
                .remove(0);
            let session: Session = serde_json::from_value(
                self.store.restore_runtime_value(
                    actual
                        .value
                        .as_ref()
                        .ok_or_else(|| err("not_found", "清理会话缺失"))?,
                )?,
            )
            .map_err(|e| err("storage", e.to_string()))?;
            let worker: RuntimeExecution =
                serde_json::from_value(self.store.restore_runtime_value(&row.data["worker"])?)
                    .map_err(|e| err("storage", e.to_string()))?;
            let proof: ResourceKey = serde_json::from_value(row.data["proof"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            match self.cleanup_worker_from_proof(
                &session,
                &worker,
                &text(&row.data, "digest")?,
                &proof,
            ) {
                Ok(()) => {
                    let mut data = row.data.clone();
                    data["status"] = json!("delegated");
                    update_record(&mut row, data);
                    self.store.commit_changes(&PreparedChangeSet {
                        command_id: format!("worker-cleanup-delegated:{}", row.id),
                        fingerprint: configuration_hash(&row.data),
                        read_set: ReadSet::new(vec![version(&selected)])?,
                        changes: vec![ResourceChange {
                            resource: selected.resource,
                            value: self.store.encode_runtime_record(&row)?,
                        }],
                        result: json!({"id":row.id}),
                        time: now() as i64,
                    })?;
                }
                Err(error)
                    if matches!(
                        error.code.as_str(),
                        "storage_busy"
                            | "revision_conflict"
                            | "unsupported"
                            | "worker_log_unavailable"
                    ) =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        for original in self.pending_runtime_records("worker_cleanup_intent", "prepared")? {
            match self.admit_worker_cleanup(&original) {
                Err(error)
                    if matches!(
                        error.code.as_str(),
                        "storage_busy"
                            | "revision_conflict"
                            | "unsupported"
                            | "worker_log_unavailable"
                    ) =>
                {
                    continue;
                }
                result => result?,
            }
        }
        Ok(())
    }
}
