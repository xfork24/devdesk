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

impl Engine {
    pub(crate) fn native_session_command(
        &self,
        method: &str,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Option<Value>> {
        if method != "session.open" {
            return Ok(None);
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
        let (project, feature, task, session_scope, material) = if target == "global" {
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
            )
        } else {
            let owner = WorkOwnerFacts::read(self.store.as_ref(), target)?;
            owner.require_revision(c.expected_revision)?;
            context.require_project(&owner.project)?;
            read_set = read_set.merge(owner.read_set.versions().iter().cloned())?;
            let (_, feature, task) = material_parts(&owner.material);
            let scope = match owner.material {
                MaterialOwner::Project { .. } => SessionScope::Project,
                MaterialOwner::Feature { .. } => SessionScope::Feature,
                MaterialOwner::Task { .. } => SessionScope::Task,
                MaterialOwner::Global => return Err(err("invalid_scope", "无效会话工作范围")),
            };
            (owner.project, feature, task, scope, owner.material)
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
        let family = ResourceKey::new(
            "session_open_candidate",
            json!([
                target,
                source.definition_id,
                configuration.agent_instance_id
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
        let latest = candidate
            .value
            .as_ref()
            .map(|raw| {
                self.store.restore_runtime_value(raw).and_then(|value| {
                    serde_json::from_value::<Session>(value)
                        .map_err(|e| err("storage", e.to_string()))
                })
            })
            .transpose()?;
        let mut snapshot_id = opt(&c.payload, "snapshot_id");
        let mut predecessor = opt(&c.payload, "predecessor");
        let provenance = context.prepare_record(method, c);
        if let Some(latest) = &latest {
            require(
                latest.project_id.as_deref() == Some(project.as_str())
                    && latest.scope == Some(session_scope)
                    && latest.owner_id == target,
                "会话候选跨范围",
            )?;
            if c.payload["force_new"] != true
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
        if let Some(previous) = &predecessor {
            require(snapshot_id.is_some(), "显式后继会话需要继承快照")?;
            let key = ResourceKey::new("session", previous);
            let row = self
                .store
                .read_resources(std::slice::from_ref(&key))?
                .into_iter()
                .next()
                .ok_or_else(|| err("storage", "原会话读取不完整"))?;
            require(
                row.value
                    .as_ref()
                    .is_some_and(|s| s["project_id"] == project),
                "后继会话不能跨项目",
            )?;
            read_set = read_set.merge([version(&row)])?;
        } else if let Some(previous) = &latest {
            let keys = [
                ResourceKey::new("session_input_state", &previous.id),
                ResourceKey::new("session_live_workers", &previous.id),
            ];
            let rows = self.store.read_resources(&keys)?;
            require(
                rows.len() == 2 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
                "原会话静止谓词读取不完整",
            )?;
            require(
                rows[0].value.as_ref().is_some_and(|s| s["active"] == false)
                    && rows[1]
                        .value
                        .as_ref()
                        .and_then(Value::as_array)
                        .is_some_and(Vec::is_empty),
                "请先结束或核对原会话的活动工作",
            )?;
            read_set = read_set.merge(rows.iter().map(version))?;
            if snapshot_id.is_none() {
                let facts = self.store.conversation_input_facts(previous)?;
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
                    kind: "conversation_handoff".into(),
                    content: serde_json::to_string(&facts.content)
                        .map_err(|e| err("serialization", e.to_string()))?,
                    documents: facts.documents.clone(),
                    inherited_from: previous.snapshot_id.clone(),
                    capture: SnapshotCapture {
                        step_execution_ids: vec![],
                        sessions: vec![previous.id.clone()],
                        captured_at: Some(now()),
                        reason: "Explicit successor after runtime/configuration change".into(),
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
                if let Some(projection) =
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
                self.session_snapshot_scope(snapshot, &project)?
                    .versions()
                    .iter()
                    .cloned(),
            )?;
        }
        let sid = stable_object_id(self.store.as_ref(), &c.command_id, "session")?;
        let session = Session {
            id: sid.clone(),
            owner_id: target.into(),
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
            json!({"agent_instance_id":configuration.agent_instance_id,"agent_revision_id":agent,"composition_hash":source.content_hash,"model_ref":composition.model_ref,"harness_ref":composition.harness_ref,"binding":binding,"step_execution_id":null}),
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

    pub(crate) fn session_snapshot_scope(&self, id: &str, project: &str) -> Result<ReadSet> {
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
        ReadSet::new(guards)
    }
}
