//! Completed conversation snapshots are derived from explicit, bounded facts.
//! Durable capture attempts keep artifact I/O out of business transactions.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    *,
};

pub struct ConversationCaptureFacts {
    pub read_set: ReadSet,
    pub content: Value,
    pub documents: Vec<Id>,
    pub execution_ids: Vec<Id>,
    pub link: Option<ConfigurationRecord>,
    pub workflow: Option<ConfigurationRecord>,
    pub project_title: String,
    pub feature_title: Option<String>,
    pub work_time: Option<u64>,
}
fn version(row: &ResourceSnapshot) -> ResourceVersion {
    ResourceVersion {
        resource: row.resource.clone(),
        revision: row.revision,
    }
}
fn decode(
    store: &dyn ports::EngineeringStore,
    row: &ResourceSnapshot,
) -> Result<ConfigurationRecord> {
    serde_json::from_value(
        store.restore_runtime_record(
            row.value
                .as_ref()
                .ok_or_else(|| err("not_found", "捕获意图缺失"))?,
        )?,
    )
    .map_err(|e| err("storage", e.to_string()))
}
fn material_owner(session: &Session) -> Result<MaterialOwner> {
    Ok(
        match session
            .scope
            .ok_or_else(|| err("invalid_scope", "捕获缺少归属"))?
        {
            SessionScope::Global => MaterialOwner::Global,
            SessionScope::Project => MaterialOwner::Project {
                id: session
                    .project_id
                    .clone()
                    .ok_or_else(|| err("invalid_scope", "捕获缺少项目"))?,
            },
            SessionScope::Feature => MaterialOwner::Feature {
                id: session
                    .feature_id
                    .clone()
                    .ok_or_else(|| err("invalid_scope", "捕获缺少 Feature"))?,
            },
            SessionScope::Task => MaterialOwner::Task {
                feature: session
                    .feature_id
                    .clone()
                    .ok_or_else(|| err("invalid_scope", "捕获缺少 Feature"))?,
                id: session
                    .task_id
                    .clone()
                    .ok_or_else(|| err("invalid_scope", "捕获缺少 Task"))?,
            },
        },
    )
}

impl Engine {
    pub(crate) fn captured_snapshot_experience(
        &self,
        facts: &ConversationCaptureFacts,
        session: &Session,
        snapshot: &Snapshot,
        artifact: &ArtifactSeal,
        turn: Option<&str>,
    ) -> Result<Option<ResourceChange>> {
        let Some(link) = &facts.link else {
            return Ok(None);
        };
        let link = ConfigurationRecord::from_storage(
            self.store.restore_runtime_record(&link.to_storage())?,
        )?;
        let workflow = facts
            .workflow
            .as_ref()
            .map(|r| r.data.clone())
            .unwrap_or(Value::Null);
        let key = ResourceKey::new("governance", format!("captured-experience:{}", snapshot.id));
        let scope = if session.scope == Some(SessionScope::Global) {
            Scope::global()
        } else {
            Scope::project(
                session
                    .project_id
                    .as_ref()
                    .ok_or_else(|| err("invalid_scope", "经验投影缺少项目"))?,
            )
        };
        let mut intent = record(
            "captured_experience_intent",
            session.id.clone(),
            scope,
            "已封存上下文的经验投影".into(),
            1,
            None,
            json!({"status":"captured","session":session,"turn_id":turn,"snapshot":{"id":snapshot.id},"artifact_id":artifact.artifact.id,
                "experience":{"agent_instance_id":link.data["agent_instance_id"],"agent_revision_id":link.data["agent_revision_id"],"snapshot_id":snapshot.id,"session_id":session.id,"project_id":session.project_id,"project_name":facts.project_title,"feature_id":session.feature_id,"feature_name":facts.feature_title.clone().unwrap_or_default(),"task_id":session.task_id,"work_owner":link.data["work_owner"],
                    "workflow_instance_id":workflow["instance_id"],"step_execution_id":workflow["execution_id"],"plan_revision_id":workflow["plan_revision_id"],"content_digest":configuration_hash(snapshot),"captured_at":snapshot.capture.captured_at,"work_time":facts.work_time,"binding":link.data["binding"],"kind":snapshot.kind}}),
            false,
        );
        intent.id = key.id.clone();
        Ok(Some(ResourceChange {
            resource: key,
            value: self.store.encode_runtime_record(&intent)?,
        }))
    }
    pub(crate) fn drive_conversation_captures(&self) -> Result<()> {
        let after = self
            .conversation_capture_cursor
            .lock()
            .map_err(|e| err("storage", e.to_string()))?
            .clone();
        let mut page = self.store.conversation_capture_page(after.as_deref())?;
        if page.ids.is_empty() && after.is_some() {
            page = self.store.conversation_capture_page(None)?;
        }
        let started = std::time::Instant::now();
        let mut visited = 0;
        let mut last = None;
        for id in &page.ids {
            if visited > 0 && started.elapsed() >= std::time::Duration::from_millis(25) {
                break;
            }
            visited += 1;
            last = Some(id.clone());
            let key = ResourceKey::new("governance", id);
            let original = self
                .store
                .read_resources(std::slice::from_ref(&key))?
                .into_iter()
                .next()
                .ok_or_else(|| err("storage", "捕获意图读取不完整"))?;
            if let Err(error) = self.capture_original_conversation(&original) {
                if matches!(
                    error.code.as_str(),
                    "storage_busy" | "revision_conflict" | "resource_exhausted"
                ) {
                    continue;
                }
                let Some(value) = original.value.as_ref() else {
                    continue;
                };
                let mut row = ConfigurationRecord::from_storage(value.clone())?;
                let mut data = row.data.clone();
                data["status"] = json!("needs_reconciliation");
                data["error"] = json!(error);
                update_record(&mut row, data);
                let outcome = self.store.commit_changes(&PreparedChangeSet {
                    command_id: format!("conversation-capture-failed:{id}:{}", original.revision),
                    fingerprint: configuration_hash(&(
                        "conversation-capture-failed/v1",
                        version(&original),
                        &error,
                    )),
                    read_set: ReadSet::new(vec![version(&original)])?,
                    changes: vec![ResourceChange {
                        resource: key,
                        value: row.to_storage(),
                    }],
                    result: json!({"id":id,"status":"needs_reconciliation"}),
                    time: now() as i64,
                });
                if let Err(error) = outcome
                    && !matches!(error.code.as_str(), "revision_conflict" | "storage_busy")
                {
                    return Err(error);
                }
            }
        }
        *self
            .conversation_capture_cursor
            .lock()
            .map_err(|e| err("storage", e.to_string()))? = if visited < page.ids.len() {
            last
        } else {
            page.next_after
        };
        Ok(())
    }

    fn capture_original_conversation(&self, original: &ResourceSnapshot) -> Result<()> {
        let mut saved = original.clone();
        let mut intent = decode(self.store.as_ref(), &saved)?;
        require(
            intent.r#type == "conversation_capture_intent"
                || (intent.r#type == "captured_experience_intent"
                    && matches!(
                        intent.data["status"].as_str(),
                        Some("captured" | "completed")
                    )),
            "不是原会话捕获或已封存经验投影",
        )?;
        if intent.data["status"] == "completed" {
            return Ok(());
        }
        let session: Session = serde_json::from_value(intent.data["session"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        require(
            intent.definition_id == session.id
                && self
                    .store
                    .project_id()
                    .is_none_or(|id| session.project_id.as_ref() == Some(&id)),
            "会话捕获跨范围",
        )?;
        let digest = configuration_hash(&("conversation-capture/v1", &intent.id));
        let observation_key = ResourceKey::new(
            "governance",
            format!("conversation-capture-observation:{digest}"),
        );
        if intent.data["status"] == "prepared" {
            let tid = text(&intent.data, "turn_id")?;
            let facts = self.store.conversation_capture_facts(
                &session,
                &tid,
                &text(&intent.data, "terminal_event_id")?,
                intent.data["execution_boundary"]
                    .as_u64()
                    .ok_or_else(|| err("storage", "缺少 worker 捕获边界"))?,
            )?;
            let mut guards = facts.read_set.versions().to_vec();
            guards.push(version(&saved));
            let mut data = intent.data.clone();
            let mut snapshot = None;
            if let Some(link) = facts.link.as_ref() {
                let link: ConfigurationRecord =
                    serde_json::from_value(self.store.restore_runtime_record(&link.to_storage())?)
                        .map_err(|e| err("storage", e.to_string()))?;
                let workflow = facts
                    .workflow
                    .as_ref()
                    .map(|r| r.data.clone())
                    .unwrap_or(Value::Null);
                let sid = uuid::Uuid::parse_str(&digest[..32])
                    .map_err(|e| err("storage", e.to_string()))?
                    .to_string();
                let snap = Snapshot {
                    id: sid.clone(),
                    project_id: session
                        .project_id
                        .clone()
                        .filter(|_| session.scope == Some(SessionScope::Project)),
                    feature_id: session.feature_id.clone(),
                    task_id: session.task_id.clone(),
                    kind: "conversation_completed".into(),
                    content: serde_json::to_string(&facts.content)
                        .map_err(|e| err("serialization", e.to_string()))?,
                    documents: facts.documents,
                    inherited_from: session.snapshot_id.clone(),
                    capture: SnapshotCapture {
                        step_execution_ids: workflow["execution_id"]
                            .as_str()
                            .map(str::to_owned)
                            .into_iter()
                            .collect(),
                        sessions: vec![session.id.clone()],
                        captured_at: Some(now()),
                        reason: "Completed discussion and sealed runtime evidence".into(),
                    },
                    runtime_bindings: session.runtime_binding.iter().cloned().collect(),
                    execution_instances: facts.execution_ids,
                };
                data["status"] = json!("capturing");
                data["snapshot"] = json!(snap);
                data["experience"] = json!({"agent_instance_id":link.data["agent_instance_id"],"agent_revision_id":link.data["agent_revision_id"],"snapshot_id":sid,"session_id":session.id,"project_id":session.project_id,"project_name":facts.project_title,"feature_id":session.feature_id,"feature_name":facts.feature_title.clone().unwrap_or_default(),"task_id":session.task_id,"work_owner":link.data["work_owner"],"workflow_instance_id":workflow["instance_id"],"step_execution_id":workflow["execution_id"],"plan_revision_id":workflow["plan_revision_id"],"content_digest":configuration_hash(&snap),"captured_at":snap.capture.captured_at,"work_time":facts.work_time,"binding":link.data["binding"],"kind":snap.kind});
                let body = serde_json::to_string_pretty(&snap)
                    .map_err(|e| err("serialization", e.to_string()))?;
                let artifact_id = uuid::Uuid::parse_str(
                    &configuration_hash(&("conversation-capture-artifact/v1", &intent.id))[..32],
                )
                .map_err(|e| err("storage", e.to_string()))?
                .to_string();
                let seal = ArtifactSeal::prepare(
                    artifact_id,
                    &material_owner(&session)?,
                    "snapshots",
                    &body,
                    Origin::External,
                )?;
                data["artifact_seal"] = json!(seal);
                snapshot = Some((seal, body));
            } else {
                data["status"] = json!("completed");
                data["skipped"] = json!("not_an_agent_session");
            }
            update_record(&mut intent, data);
            let outcome = self.store.submit_changes(&PreparedChangeSet {
                command_id: format!("conversation-capture-prepare:{digest}"),
                fingerprint: configuration_hash(&("conversation-capture-prepare/v1", &intent.id)),
                read_set: ReadSet::new(guards)?,
                changes: vec![ResourceChange {
                    resource: saved.resource.clone(),
                    value: self.store.encode_runtime_record(&intent)?,
                }],
                result: json!({"id":intent.id}),
                time: now() as i64,
            })?;
            if outcome.applied
                && let Some((seal, body)) = snapshot
            {
                let artifact = self.artifacts.seal_prepared(&seal, &body);
                let mut observed = record(
                    "conversation_capture_observation",
                    intent.id.clone(),
                    intent.scope.clone(),
                    "会话快照封存观察".into(),
                    1,
                    None,
                    json!({"value":{"result":artifact}}),
                    true,
                );
                observed.id = observation_key.id.clone();
                self.store.commit_changes(&PreparedChangeSet {
                    command_id: format!("conversation-capture-journal:{digest}"),
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
            saved = self
                .store
                .read_resources(std::slice::from_ref(&saved.resource))?
                .into_iter()
                .next()
                .ok_or_else(|| err("storage", "原捕获意图缺失"))?;
            intent = decode(self.store.as_ref(), &saved)?;
        }
        if intent.data["status"] == "completed" {
            return Ok(());
        }
        if intent.data["status"] == "capturing" {
            let mut observations = self
                .store
                .read_resources(std::slice::from_ref(&observation_key))?;
            require(observations.len() == 1, "封存观察读取不完整")?;
            let result = observations[0]
                .value
                .as_ref()
                .map(|observed| {
                    serde_json::from_value::<Result<Artifact>>(
                        observed["data"]["value"]["result"].clone(),
                    )
                    .map_err(|e| err("storage", e.to_string()))
                })
                .transpose()?;
            let mut accepted_key = observation_key.clone();
            let artifact = if let Some(Ok(artifact)) = result {
                artifact
            } else {
                let seal: ArtifactSeal =
                    serde_json::from_value(intent.data["artifact_seal"].clone())
                        .map_err(|e| err("storage", e.to_string()))?;
                let artifact = self.artifacts.inspect_prepared(&seal)?.ok_or_else(|| {
                    err(
                        "conversation_capture_unresolved",
                        "原身份文件尚未核实，保留原封存意图及观察",
                    )
                })?;
                accepted_key = ResourceKey::new(
                    "governance",
                    format!("conversation-capture-reconciled:{digest}"),
                );
                let result: Result<Artifact> = Ok(artifact.clone());
                let mut proof = record(
                    "conversation_capture_observation",
                    intent.id.clone(),
                    intent.scope.clone(),
                    "核对原身份快照文件".into(),
                    1,
                    None,
                    json!({"reconciles":observation_key.id,"value":{"result":result}}),
                    true,
                );
                proof.id = accepted_key.id.clone();
                self.store.commit_changes(&PreparedChangeSet {
                    command_id: format!("conversation-capture-reconcile:{digest}"),
                    fingerprint: configuration_hash(&(
                        "conversation-capture-reconcile/v1",
                        &intent.id,
                        &seal.artifact.id,
                    )),
                    read_set: ReadSet::new(vec![ResourceVersion {
                        resource: accepted_key.clone(),
                        revision: 0,
                    }])?,
                    changes: vec![ResourceChange {
                        resource: accepted_key.clone(),
                        value: proof.to_storage(),
                    }],
                    result: json!({"id":proof.id}),
                    time: now() as i64,
                })?;
                observations = self
                    .store
                    .read_resources(std::slice::from_ref(&accepted_key))?;
                require(observations.len() == 1, "封存核对证明读取不完整")?;
                artifact
            };
            let snapshot_id = text(&intent.data["snapshot"], "id")?;
            let snapshot_key = ResourceKey::new("snapshot", &snapshot_id);
            let artifact_key = ResourceKey::new("artifact", &artifact.id);
            let raw = saved.value.as_ref().unwrap();
            let snapshot = raw["data"]["snapshot"].clone();
            let mut data = intent.data.clone();
            data["status"] = json!("captured");
            data["snapshot"] = json!({"id":snapshot_id});
            data["artifact_id"] = json!(artifact.id);
            data["observation_id"] = json!(accepted_key.id);
            update_record(&mut intent, data);
            self.store.commit_changes(&PreparedChangeSet {
                command_id: format!("conversation-capture-admit:{digest}"),
                fingerprint: configuration_hash(&(
                    "conversation-capture-admit/v1",
                    &intent.id,
                    &accepted_key,
                )),
                read_set: ReadSet::new(vec![
                    version(&saved),
                    version(&observations[0]),
                    ResourceVersion {
                        resource: snapshot_key.clone(),
                        revision: 0,
                    },
                    ResourceVersion {
                        resource: artifact_key.clone(),
                        revision: 0,
                    },
                ])?,
                changes: vec![
                    ResourceChange {
                        resource: saved.resource.clone(),
                        value: self.store.encode_runtime_record(&intent)?,
                    },
                    ResourceChange {
                        resource: snapshot_key,
                        value: snapshot,
                    },
                    ResourceChange {
                        resource: artifact_key,
                        value: json!(artifact),
                    },
                ],
                result: json!({"id":intent.id,"snapshot_id":snapshot_id}),
                time: now() as i64,
            })?;
            saved = self
                .store
                .read_resources(std::slice::from_ref(&saved.resource))?
                .into_iter()
                .next()
                .ok_or_else(|| err("storage", "已捕获意图缺失"))?;
            intent = decode(self.store.as_ref(), &saved)?;
        }
        if intent.data["status"] == "captured" {
            self.project_captured_experience(&saved, &intent)?;
        }
        Ok(())
    }

    fn project_captured_experience(
        &self,
        saved: &ResourceSnapshot,
        intent: &ConfigurationRecord,
    ) -> Result<()> {
        let mut entry_data = intent.data["experience"].clone();
        if let Some(turn_id) = intent.data["turn_id"].as_str() {
            entry_data["turn_id"] = json!(turn_id);
        }
        let data = &entry_data;
        let snapshot_id = text(data, "snapshot_id")?;
        let object = data["work_owner"]
            .as_object()
            .filter(|owner| owner.get("kind").and_then(Value::as_str) == Some("entity"))
            .and_then(|owner| owner.get("id").and_then(Value::as_str));
        let feature = data["feature_id"].as_str();
        let identity = if let Some(object) = object {
            format!(
                "experience:{}",
                configuration_hash(&json!([
                    data["agent_instance_id"],
                    data["project_id"],
                    "entity",
                    object
                ]))
            )
        } else if let Some(feature) = feature {
            format!(
                "experience:{}",
                configuration_hash(&json!([
                    data["agent_instance_id"],
                    data["project_id"],
                    feature
                ]))
            )
        } else {
            format!(
                "experience:{}",
                configuration_hash(&json!([
                    data["agent_instance_id"],
                    snapshot_id,
                    data["session_id"]
                ]))
            )
        };
        let key = ResourceKey::new("governance", &identity);
        let previous = self
            .store
            .read_resources(std::slice::from_ref(&key))?
            .into_iter()
            .next()
            .ok_or_else(|| err("storage", "经验头读取不完整"))?;
        let mut entry = record(
            "experience",
            object.or(feature).unwrap_or(&snapshot_id).into(),
            intent.scope.clone(),
            data["feature_name"]
                .as_str()
                .filter(|v| !v.is_empty())
                .or_else(|| data["project_name"].as_str())
                .unwrap_or("会话经验")
                .into(),
            1,
            None,
            data.clone(),
            feature.is_none(),
        );
        entry.id = identity;
        let mut changes = Vec::new();
        let mut guards = vec![version(saved), version(&previous)];
        if previous.value.is_some() {
            let mut original = decode(self.store.as_ref(), &previous)?;
            require(
                original.r#type == "experience" && original.scope == intent.scope,
                "经验投影改变了原归属",
            )?;
            let already = original.data["snapshot_id"] == snapshot_id
                || original.data["history"]
                    .as_array()
                    .is_some_and(|rows| rows.iter().any(|r| r["snapshot_id"] == snapshot_id));
            if !already {
                require(
                    feature.is_some() && !original.immutable,
                    "不可变经验身份不能覆盖",
                )?;
                let mut prior = original.data.clone();
                let mut history = prior
                    .as_object_mut()
                    .and_then(|m| m.remove("history"))
                    .and_then(|v| v.as_array().cloned())
                    .unwrap_or_default();
                let newer = data["captured_at"].as_u64().unwrap_or(0)
                    >= prior["captured_at"].as_u64().unwrap_or(0);
                let mut next = if newer {
                    history.push(prior);
                    data.clone()
                } else {
                    history.push(data.clone());
                    prior
                };
                next["history"] = json!(history);
                update_record(&mut original, next);
                changes.push(ResourceChange {
                    resource: key,
                    value: self.store.encode_runtime_record(&original)?,
                });
            }
        } else {
            changes.push(ResourceChange {
                resource: key,
                value: self.store.encode_runtime_record(&entry)?,
            });
        }
        let mut next = intent.clone();
        let mut data = next.data.clone();
        data["status"] = json!("completed");
        data["experience_id"] = json!(entry.id);
        update_record(&mut next, data);
        changes.push(ResourceChange {
            resource: saved.resource.clone(),
            value: self.store.encode_runtime_record(&next)?,
        });
        self.store.commit_changes(&PreparedChangeSet {
            command_id: format!(
                "conversation-experience:{}:{}",
                intent.id, previous.revision
            ),
            fingerprint: configuration_hash(&(
                "conversation-experience/v1",
                &intent.id,
                previous.revision,
            )),
            read_set: ReadSet::new(std::mem::take(&mut guards))?,
            changes,
            result: json!({"id":intent.id,"experience_id":entry.id}),
            time: now() as i64,
        })?;
        Ok(())
    }
}
