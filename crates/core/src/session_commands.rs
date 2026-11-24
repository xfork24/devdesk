//! Public discussion Session admission uses native ownership, immutable Agent
//! sources and the actual predecessor. Formal sessions retain their own grants.
use crate::{
    concurrency::*,
    governance::record,
    material_commands::{material_parts, stable_object_id},
    work_owners::WorkOwnerFacts,
    *,
};

fn version(row: &ResourceSnapshot) -> ResourceVersion {
    ResourceVersion {
        resource: row.resource.clone(),
        revision: row.revision,
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscussionSource {
    session_id: Id,
    turn_id: Option<Id>,
    turn_revision: Option<u64>,
}

fn deleted_session(id: &str) -> ResourceKey {
    ResourceKey::new("governance", format!("discussion-session-deleted:{id}"))
}

fn deleted_turn(id: &str) -> ResourceKey {
    ResourceKey::new("governance", format!("discussion-turn-deleted:{id}"))
}

impl Engine {
    pub(crate) fn native_session_command(
        &self,
        method: &str,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Option<Value>> {
        if ["session.delete", "conversation.delete_turn"].contains(&method) {
            return self
                .delete_discussion(method, c, fingerprint, context)
                .map(Some);
        }
        if method != "session.open" {
            return Ok(None);
        }
        let continuation = c
            .payload
            .get("continue_from")
            .map(|source| {
                serde_json::from_value::<DiscussionSource>(source.clone())
                    .map_err(|e| err("invalid_params", format!("无效续聊来源：{e}")))
            })
            .transpose()?;
        if continuation.is_some() {
            require(
                c.payload.get("snapshot_id").is_none() && c.payload.get("predecessor").is_none(),
                "续聊快照必须由原会话生成",
            )?;
        }
        if c.payload["controller_id"].is_string()
            || c.payload["configured_execution_id"].is_string()
        {
            return Err(err(
                "permission_denied",
                "正式会话必须消费原生配置冻结，不能使用普通命令绑定旁路",
            ));
        }
        if let Some(result) = self.replay_sealed_command(&c.command_id, fingerprint)? {
            return Ok(Some(result));
        }
        let binding: RuntimeBinding = serde_json::from_value(c.payload["binding"].clone())
            .map_err(|e| err("invalid_params", format!("缺少完整运行时绑定：{e}")))?;
        let configuration = binding.agent_configuration.as_ref().ok_or_else(|| {
            err(
                "configuration_required",
                "Session 需要确切 Agent 实例和完整绑定",
            )
        })?;
        require(
            binding.work_type == WorkType::Discussion,
            "正式 Session 必须消费原准备授权",
        )?;
        require(
            matches!(context.actor(), WorkflowActor::Human),
            "自由讨论会话需要实际 Human 来源",
        )?;
        require(c.payload.get("scenario").is_none(), "不再支持模拟场景参数")?;
        let catalog = self.store.configuration_snapshot()?;
        if let Some(original) = c.payload["prepared_catalog_revision"].as_u64()
            && original != catalog.revision
        {
            return Err(err(
                "revision_conflict",
                "会话准备期间配置已经变化，请保留选择并重新提交",
            ));
        }
        let target = c
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "缺少会话归属"))?;
        let agent = text(&c.payload, "agent")?;
        let source = catalog.state.configuration(&agent)?;
        let instance = catalog
            .state
            .configuration(&configuration.agent_instance_id)?;
        require(
            source.r#type == "agent"
                && instance.r#type == "agent_instance"
                && source.id == configuration.agent_revision_id
                && source.content_hash == configuration.composition_hash
                && instance.data["agent_revision_id"] == agent
                && c.payload["agent_instance_id"] == configuration.agent_instance_id
                && !configuration_retired(&catalog.state, source)
                && !configuration_retired(&catalog.state, instance)
                && catalog.is_head(instance),
            "会话不能替换原 Agent 组合、实例或已停用选择",
        )?;
        let composition = catalog.state.resolve_agent(&agent)?;
        let model: ModelDefinition = catalog
            .state
            .configuration(&composition.model_ref)?
            .decode()?;
        let harness = catalog.state.configuration(&composition.harness_ref)?;
        require(
            configuration.model_revision_id == composition.model_ref
                && configuration.harness_revision_id == composition.harness_ref
                && binding.model == model.model
                && harness.data["provider"] == json!(binding.installation.key.provider),
            "会话运行绑定改变了 Agent 的模型或 Harness",
        )?;
        require(
            binding.model_connection == catalog.state.resolve_model_connection(&model, harness)?,
            "会话运行绑定改变了模型连接",
        )?;
        let permissions = binding
            .permissions
            .as_ref()
            .ok_or_else(|| err("permission_unconfigured", "会话缺少冻结权限"))?;
        validate_frozen_permissions(&catalog.state, permissions)?;
        let mut read_set = ReadSet::new(vec![ResourceVersion {
            resource: ResourceKey::new("catalog_clock", "configuration"),
            revision: catalog.revision,
        }])?;
        let (project, feature, task, session_scope, material, work_owner) = if target == "global" {
            require(
                catalog.state.projects.is_empty(),
                "全局讨论必须使用独立讨论库",
            )?;
            context.require_scope(None)?;
            (
                GLOBAL_DISCUSSION_NAMESPACE.to_owned(),
                None,
                None,
                SessionScope::Global,
                MaterialOwner::Global,
                None,
            )
        } else {
            let owner = WorkOwnerFacts::read(self.store.as_ref(), target)?;
            if continuation.is_none() {
                owner.require_revision(c.expected_revision)?;
            }
            context.require_project(&owner.project)?;
            read_set = read_set.merge(owner.read_set.versions().iter().cloned())?;
            let (_, feature, task) = material_parts(&owner.material);
            let scope = match owner.material {
                MaterialOwner::Project { .. } => SessionScope::Project,
                MaterialOwner::Feature { .. } => SessionScope::Feature,
                MaterialOwner::Task { .. } => SessionScope::Task,
                MaterialOwner::Global => return Err(err("invalid_scope", "无效会话工作范围")),
            };
            let work_owner = owner.work_reference();
            (
                owner.project,
                feature,
                task,
                scope,
                owner.material,
                Some(work_owner),
            )
        };
        let partition_owner = if matches!(work_owner, Some(WorkReference::Entity { .. })) {
            project.as_str()
        } else {
            target
        };
        let selection_scope = if session_scope == SessionScope::Global {
            instance.scope.clone()
        } else {
            Scope::project(&project)
        };
        require(
            instance.scope.visible_from(&selection_scope)
                && configuration_visible(&catalog.state, source, selection_scope.project_id()),
            "Agent 不适用于此讨论范围",
        )?;
        let latest = if continuation.is_none() {
            let family = ResourceKey::new(
                "session_open_candidate",
                json!([
                    partition_owner,
                    source.definition_id,
                    configuration.agent_instance_id,
                    target
                ])
                .to_string(),
            );
            let candidate = self
                .store
                .read_resources(std::slice::from_ref(&family))?
                .into_iter()
                .next()
                .ok_or_else(|| err("storage", "会话候选读取不完整"))?;
            read_set = read_set.merge([version(&candidate)])?;
            candidate
                .value
                .as_ref()
                .map(|raw| {
                    self.store.restore_runtime_value(raw).and_then(|value| {
                        serde_json::from_value::<Session>(value)
                            .map_err(|e| err("storage", e.to_string()))
                    })
                })
                .transpose()?
        } else {
            None
        };
        let mut snapshot_id = opt(&c.payload, "snapshot_id");
        let mut predecessor = opt(&c.payload, "predecessor");
        let provenance = context.prepare_record(method, c);
        if let Some(latest) = &latest {
            let (owner_id, owner_guard) = self.session_work_owner(latest)?;
            read_set = read_set.merge([owner_guard])?;
            require(
                latest.project_id.as_deref() == Some(project.as_str())
                    && latest.scope == Some(session_scope)
                    && owner_id == target,
                "会话候选跨范围",
            )?;
            if continuation.is_none()
                && c.payload["force_new"] != true
                && latest.agent == agent
                && latest.runtime_binding.as_ref() == Some(&binding)
                && snapshot_id
                    .as_ref()
                    .is_none_or(|id| latest.snapshot_id.as_ref() == Some(id))
                && predecessor.is_none()
                && !matches!(
                    latest.lifecycle,
                    Some(SessionLifecycle::RequiresSuccessor | SessionLifecycle::Closed)
                )
            {
                read_set = read_set.merge([
                    ResourceVersion {
                        resource: ResourceKey::new("session", &latest.id),
                        revision: latest.revision,
                    },
                    ResourceVersion {
                        resource: provenance.resource.clone(),
                        revision: 0,
                    },
                ])?;
                return self
                    .store
                    .commit_changes(&PreparedChangeSet {
                        command_id: c.command_id.clone(),
                        fingerprint: fingerprint.into(),
                        read_set,
                        changes: vec![provenance],
                        result: json!({"id":latest.id,"reused":true,"revision":latest.revision}),
                        time: now() as i64,
                    })
                    .map(|r| Some(r.result));
            }
        }
        let mut changes = Vec::new();
        let mut handoff = None;
        let branch = if let Some(source) = &continuation {
            let (original, guards) = self.discussion_source(
                &source.session_id,
                c.expected_revision,
                source.turn_id.as_deref(),
                source.turn_revision,
            )?;
            let (owner_id, owner_guard) = self.session_work_owner(&original)?;
            read_set = read_set.merge([owner_guard])?;
            require(
                owner_id == target
                    && original.project_id.as_deref() == Some(&project)
                    && original.scope == Some(session_scope)
                    && original
                        .runtime_binding
                        .as_ref()
                        .and_then(|b| b.agent_configuration.as_ref())
                        .is_some_and(|a| a.agent_instance_id == configuration.agent_instance_id),
                "续聊需要原会话的归属和 Agent 实例",
            )?;
            read_set = read_set.merge(guards.versions().iter().cloned())?;
            Some(original)
        } else {
            None
        };
        if let Some(previous) = &predecessor {
            require(snapshot_id.is_some(), "显式后继会话需要继承快照")?;
            let key = ResourceKey::new("session", previous);
            let row = self
                .store
                .read_resources(std::slice::from_ref(&key))?
                .into_iter()
                .next()
                .ok_or_else(|| err("storage", "原会话读取不完整"))?;
            let original: Session = serde_json::from_value(
                self.store.restore_runtime_value(
                    row.value
                        .as_ref()
                        .ok_or_else(|| err("not_found", "原会话缺失"))?,
                )?,
            )
            .map_err(|error| err("storage", error.to_string()))?;
            let (owner_id, owner_guard) = self.session_work_owner(&original)?;
            require(
                original.project_id.as_deref() == Some(&project) && owner_id == target,
                "后继会话不能跨工作归属",
            )?;
            read_set = read_set.merge([version(&row), owner_guard])?;
        } else if let Some(previous) = branch.as_ref().or(latest.as_ref()) {
            let keys = [
                ResourceKey::new("session_input_state", &previous.id),
                ResourceKey::new("session_live_workers", &previous.id),
                ResourceKey::new("discussion_live_turn", &previous.id),
            ];
            let rows = self.store.read_resources(&keys)?;
            require(
                rows.len() == keys.len() && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
                "原会话静止谓词读取不完整",
            )?;
            require(
                rows[0].value.as_ref().is_some_and(|s| {
                    if branch.is_some() {
                        s["permission_switch_pending"] == false
                            && rows[2].value == Some(json!(false))
                    } else {
                        s["active"] == false
                    }
                }) && rows[1]
                    .value
                    .as_ref()
                    .and_then(Value::as_array)
                    .is_some_and(Vec::is_empty),
                "请先结束或核对原会话的活动工作",
            )?;
            read_set = read_set.merge(rows.iter().map(version))?;
            if snapshot_id.is_none() {
                let through = continuation
                    .as_ref()
                    .and_then(|source| source.turn_id.as_deref());
                let facts = self.store.conversation_input_facts(previous, through)?;
                read_set = read_set.merge(facts.read_set.versions().iter().cloned())?;
                let sid = stable_object_id(
                    self.store.as_ref(),
                    &c.command_id,
                    "session-handoff-snapshot",
                )?;
                let (project_id, feature_id, task_id) = material_parts(&material);
                let snapshot = Snapshot {
                    id: sid.clone(),
                    project_id,
                    feature_id,
                    task_id,
                    kind: if branch.is_some() {
                        "discussion_continuation"
                    } else {
                        "conversation_handoff"
                    }
                    .into(),
                    content: serde_json::to_string(&facts.content)
                        .map_err(|e| err("serialization", e.to_string()))?,
                    documents: facts.documents.clone(),
                    inherited_from: previous.snapshot_id.clone(),
                    capture: SnapshotCapture {
                        step_execution_ids: vec![],
                        sessions: vec![previous.id.clone()],
                        captured_at: Some(now()),
                        reason: if branch.is_some() {
                            "Explicit continuation from a discussion"
                        } else {
                            "Explicit successor after runtime/configuration change"
                        }
                        .into(),
                    },
                    runtime_bindings: previous.runtime_binding.iter().cloned().collect(),
                    execution_instances: facts.execution_ids.clone(),
                };
                let body = serde_json::to_string_pretty(&snapshot)
                    .map_err(|e| err("serialization", e.to_string()))?;
                let seal = ArtifactSeal::prepare(
                    stable_object_id(
                        self.store.as_ref(),
                        &c.command_id,
                        "session-handoff-artifact",
                    )?,
                    &material,
                    "snapshots",
                    &body,
                    Origin::External,
                )?;
                if branch.is_none()
                    && let Some(projection) =
                        self.captured_snapshot_experience(&facts, previous, &snapshot, &seal, None)?
                {
                    read_set = read_set.merge([ResourceVersion {
                        resource: projection.resource.clone(),
                        revision: 0,
                    }])?;
                    changes.push(projection);
                }
                let key = ResourceKey::new("snapshot", &sid);
                read_set = read_set.merge([ResourceVersion {
                    resource: key.clone(),
                    revision: 0,
                }])?;
                changes.push(ResourceChange {
                    resource: key,
                    value: self.store.encode_runtime_value(&json!(snapshot))?,
                });
                snapshot_id = Some(sid);
                handoff = Some((seal, body));
            }
            predecessor = Some(previous.id.clone());
        }
        if let Some(snapshot) = snapshot_id.as_deref().filter(|_| handoff.is_none()) {
            read_set = read_set.merge(
                self.session_snapshot_scope(snapshot, &project, target)?
                    .versions()
                    .iter()
                    .cloned(),
            )?;
        }
        let sid = stable_object_id(self.store.as_ref(), &c.command_id, "session")?;
        let session = Session {
            id: sid.clone(),
            owner_id: partition_owner.into(),
            agent: agent.clone(),
            harness: binding.installation.id.clone(),
            sandbox: format!("docker-image:{}", binding.installation.id),
            snapshot_id,
            instance: None,
            origin: Origin::External,
            project_id: Some(project.clone()),
            feature_id: feature,
            task_id: task,
            scope: Some(session_scope),
            lifecycle: Some(SessionLifecycle::Prepared),
            runtime_binding: Some(binding.clone()),
            external_session: None,
            predecessor,
            revision: 1,
        };
        let session_key = ResourceKey::new("session", &sid);
        changes.push(ResourceChange {
            resource: session_key.clone(),
            value: self.store.encode_runtime_value(&json!(session))?,
        });
        let record_scope = if session_scope == SessionScope::Global {
            Scope::global()
        } else {
            Scope::project(&project)
        };
        let mut link = record(
            "agent_session",
            sid.clone(),
            record_scope.clone(),
            instance.name.clone(),
            1,
            None,
            json!({"agent_instance_id":configuration.agent_instance_id,"agent_revision_id":agent,"composition_hash":source.content_hash,"model_ref":composition.model_ref,"harness_ref":composition.harness_ref,"binding":binding,"step_execution_id":null,"work_owner":work_owner}),
            true,
        );
        link.id = format!("agent-session:{sid}");
        let link_key = ResourceKey::new("governance", &link.id);
        changes.push(ResourceChange {
            resource: link_key.clone(),
            value: self.store.encode_runtime_record(&link)?,
        });
        changes.push(provenance.clone());
        read_set = read_set.merge([
            ResourceVersion {
                resource: session_key,
                revision: 0,
            },
            ResourceVersion {
                resource: link_key,
                revision: 0,
            },
            ResourceVersion {
                resource: provenance.resource,
                revision: 0,
            },
        ])?;
        let plan = PreparedChangeSet {
            command_id: c.command_id.clone(),
            fingerprint: fingerprint.into(),
            read_set,
            changes,
            result: json!({"id":sid,"reused":false,"revision":1}),
            time: now() as i64,
        };
        if let Some((seal, body)) = handoff {
            self.seal_command_changes(plan, seal, &body, record_scope)
                .map(Some)
        } else {
            self.store.commit_changes(&plan).map(|r| Some(r.result))
        }
    }

    fn discussion_source(
        &self,
        sid: &str,
        expected_revision: Option<u64>,
        turn: Option<&str>,
        turn_revision: Option<u64>,
    ) -> Result<(Session, ReadSet)> {
        require(
            turn.is_some() == turn_revision.is_some(),
            "续聊或删除需要原轮次版本",
        )?;
        let mut keys = vec![ResourceKey::new("session", sid), deleted_session(sid)];
        if let Some(tid) = turn {
            keys.extend([ResourceKey::new("turn", tid), deleted_turn(tid)]);
        }
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == keys.len()
                && rows
                    .iter()
                    .zip(&keys)
                    .all(|(row, key)| &row.resource == key),
            "讨论来源读取不完整",
        )?;
        let raw = rows[0]
            .value
            .as_ref()
            .filter(|_| rows[1].value.is_none())
            .ok_or_else(|| err("not_found", "原会话不存在或已删除"))?;
        if expected_revision != Some(rows[0].revision) {
            return Err(err("revision_conflict", "原会话已变化，请刷新后重试"));
        }
        if let Some(tid) = turn {
            let value = rows[2]
                .value
                .as_ref()
                .filter(|v| v["id"] == tid && v["session_id"] == sid && rows[3].value.is_none())
                .ok_or_else(|| err("not_found", "该轮次不属于原会话或已删除"))?;
            if turn_revision != Some(rows[2].revision) {
                return Err(err("revision_conflict", "原轮次已变化，请刷新后重试"));
            }
            require(
                matches!(
                    value["status"].as_str(),
                    Some("completed" | "failed" | "cancelled" | "needs_reconciliation")
                ),
                "请先结束原轮次",
            )?;
        }
        let session = serde_json::from_value(self.store.restore_runtime_value(raw)?)
            .map_err(|e| err("storage", e.to_string()))?;
        Ok((session, ReadSet::new(rows.iter().map(version).collect())?))
    }

    fn delete_discussion(
        &self,
        method: &str,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Value> {
        require(
            matches!(context.actor(), WorkflowActor::Human),
            "删除讨论需要 Human 来源",
        )?;
        context.require_scope(self.store.project_id().as_deref())?;
        if let Some(saved) = self.store.replay_commit(&c.command_id, fingerprint)? {
            return Ok(saved.result);
        }
        let sid = c
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "缺少原会话"))?;
        let tid = (method == "conversation.delete_turn")
            .then(|| text(&c.payload, "turn_id"))
            .transpose()?;
        let (source, mut guards) = self.discussion_source(
            sid,
            c.expected_revision,
            tid.as_deref(),
            c.payload["turn_revision"].as_u64(),
        )?;
        let global = source.scope == Some(SessionScope::Global);
        require(
            source.scope.is_some()
                && source.project_id.as_deref()
                    == Some(
                        self.store
                            .project_id()
                            .as_deref()
                            .unwrap_or(GLOBAL_DISCUSSION_NAMESPACE),
                    )
                && source
                    .runtime_binding
                    .as_ref()
                    .is_some_and(|b| b.work_type == WorkType::Discussion),
            "只有当前范围内的独立讨论可以删除",
        )?;
        let state = self.store.read_resources(&[
            ResourceKey::new("session_input_state", sid),
            ResourceKey::new("session_live_workers", sid),
        ])?;
        require(state.len() == 2, "删除讨论的运行状态读取不完整")?;
        require(
            state[0].value.as_ref().is_some_and(|s| {
                s["active"] == false
                    && s["permission_switch_pending"] == false
                    && s["configured"] == false
                    && s["controller"] == false
            }) && state[1]
                .value
                .as_ref()
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty),
            "请先结束活动轮次或核对未停止的运行，正式会话按原流程保留",
        )?;
        guards = guards.merge(state.iter().map(version))?;
        let key = tid
            .as_deref()
            .map_or_else(|| deleted_session(sid), deleted_turn);
        let scope = if global {
            Scope::global()
        } else {
            Scope::project(source.project_id.as_deref().unwrap())
        };
        let mut marker = record(
            "discussion_deleted",
            tid.as_deref().unwrap_or(sid).into(),
            scope,
            "删除讨论记录".into(),
            1,
            None,
            json!({"session_id":sid,"turn_id":tid}),
            true,
        );
        marker.id = key.id.clone();
        let mut next = source;
        if tid.is_none() {
            next.lifecycle = Some(SessionLifecycle::Closed);
        } else if next.lifecycle != Some(SessionLifecycle::Closed) {
            // A provider thread can still contain the deleted round.
            next.lifecycle = Some(SessionLifecycle::RequiresSuccessor);
        }
        next.revision += 1;
        let provenance = context.prepare_record(method, c);
        let reads = guards.merge([ResourceVersion {
            resource: provenance.resource.clone(),
            revision: 0,
        }])?;
        self.store
            .commit_changes(&PreparedChangeSet {
                command_id: c.command_id.clone(),
                fingerprint: fingerprint.into(),
                read_set: reads,
                changes: vec![
                    ResourceChange {
                        resource: key,
                        value: marker.to_storage(),
                    },
                    ResourceChange {
                        resource: ResourceKey::new("session", sid),
                        value: self.store.encode_runtime_value(&json!(next))?,
                    },
                    provenance,
                ],
                result: json!({"id":sid,"turn_id":tid,"revision":next.revision}),
                time: now() as i64,
            })
            .map(|receipt| receipt.result)
    }

    fn session_owner_source(&self, session: &str) -> Result<ResourceSnapshot> {
        let link = crate::platform_facts::one(
            self.store.as_ref(),
            "governance",
            &format!("agent-session:{session}"),
        )?;
        if link
            .value
            .as_ref()
            .is_some_and(|row| !row["data"]["work_owner"].is_null())
        {
            return Ok(link);
        }
        let original = crate::platform_facts::one(
            self.store.as_ref(),
            "governance",
            &format!("formal-session-source:{session}"),
        )?;
        Ok(if original.value.is_some() {
            original
        } else {
            link
        })
    }

    pub(crate) fn session_work_owner(&self, session: &Session) -> Result<(Id, ResourceVersion)> {
        let link = self.session_owner_source(&session.id)?;
        let mut owner = session.owner_id.clone();
        if let Some(row) = &link.value {
            if let Some(reference) = row["data"]
                .get("work_owner")
                .filter(|value| !value.is_null())
            {
                let reference: WorkReference = serde_json::from_value(reference.clone())
                    .map_err(|error| err("storage", error.to_string()))?;
                match reference {
                    WorkReference::Entity { id, project } => {
                        require(
                            session.project_id.as_deref() == Some(project.as_str()),
                            "会话对象范围跨项目",
                        )?;
                        owner = id;
                    }
                    other => require(other.id() == owner, "会话归属与冻结来源不符")?,
                }
            }
        }
        Ok((owner, version(&link)))
    }

    pub(crate) fn session_snapshot_scope(
        &self,
        id: &str,
        project: &str,
        owner: &str,
    ) -> Result<ReadSet> {
        let key = ResourceKey::new("snapshot", id);
        let row = self
            .store
            .read_resources(std::slice::from_ref(&key))?
            .into_iter()
            .next()
            .ok_or_else(|| err("storage", "继承快照读取不完整"))?;
        let snapshot = row
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "继承快照不存在"))?;
        let mut guards = vec![version(&row)];
        let source = if let Some(project) = snapshot["project_id"].as_str() {
            project.to_owned()
        } else if let Some(feature) = snapshot["feature_id"].as_str() {
            let parent = self
                .store
                .read_resources(&[ResourceKey::new("feature", feature)])?
                .into_iter()
                .next()
                .ok_or_else(|| err("storage", "继承快照归属读取不完整"))?;
            let project = text(
                parent
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "继承快照父对象缺失"))?,
                "project_id",
            )?;
            guards.push(version(&parent));
            project
        } else {
            GLOBAL_DISCUSSION_NAMESPACE.into()
        };
        require(source == project, "会话不能跨项目继承快照")?;
        let entity_scope = owner != project
            && crate::platform_facts::one(
                self.store.as_ref(),
                "governance",
                &domain_entity_key(project, owner),
            )?
            .value
            .is_some();
        let mut has_entity_source = false;
        for session in snapshot["capture"]["sessions"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let session = session
                .as_str()
                .ok_or_else(|| err("storage", "快照会话引用无效"))?;
            let link = self.session_owner_source(session)?;
            if let Some(row) = &link.value
                && row["data"]["work_owner"]["kind"] == "entity"
            {
                require(
                    row["data"]["work_owner"]["project"] == project
                        && row["data"]["work_owner"]["id"] == owner,
                    "会话不能跨对象继承快照",
                )?;
                has_entity_source = true;
            } else {
                require(!entity_scope, "对象会话不能继承其他范围的快照")?;
            }
            guards.push(version(&link));
        }
        require(
            !entity_scope || has_entity_source,
            "对象快照需要确切的会话来源",
        )?;
        ReadSet::default().merge(guards)
    }
}
