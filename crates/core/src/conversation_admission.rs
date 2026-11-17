//! One finite input admission shared by native discussion and formal callers.
use crate::{
    concurrency::*,
    governance::record,
    material_commands::{material_parts, stable_object_id},
    work_owners::WorkOwnerFacts,
    *,
};

pub(crate) struct NativeConversationInput {
    pub session: Session,
    pub original: ResourceSnapshot,
    pub read_set: ReadSet,
    pub changes: Vec<ResourceChange>,
    pub provenance: ResourceChange,
    pub input: String,
    pub client: String,
    pub references: Value,
    pub task_source: Option<ConfigurationRecord>,
    pub code_view: Option<FrozenCode>,
    pub origin: Origin,
    pub capture: Option<(Session, ConversationCaptureFacts)>,
    pub extra_files: Vec<(ArtifactSeal, String)>,
}
impl Engine {
    pub(crate) fn admit_native_conversation_input(
        &self,
        c: &Command,
        fingerprint: &str,
        mut prepared: NativeConversationInput,
    ) -> Result<Value> {
        let session = &prepared.session;
        let sid = &session.id;
        let project = session
            .project_id
            .as_deref()
            .ok_or_else(|| err("invalid_scope", "输入缺少项目范围"))?;
        let formal = prepared.task_source.is_some();
        let creating = prepared.original.revision == 0;
        let (captured_session, facts) = if let Some(capture) = prepared.capture.take() {
            capture
        } else {
            (
                session.clone(),
                self.store.conversation_input_facts(session)?,
            )
        };
        require(
            captured_session.project_id == session.project_id
                && captured_session.owner_id == session.owner_id,
            "输入捕获不能跨项目或工作归属",
        )?;
        let same_source = match (&facts.workflow, &prepared.task_source) {
            (None, None) => true,
            (Some(actual), Some(selected)) => {
                actual.scope == selected.scope
                    && actual.data == selected.data
                    && (actual.id == selected.id || creating)
            }
            _ => false,
        };
        require(same_source, "输入准备不能更改正式任务关联")?;
        prepared.read_set = prepared
            .read_set
            .merge(facts.read_set.versions().iter().cloned())?;
        let (material, source_revision) = if session.scope == Some(SessionScope::Global) {
            require(
                project == GLOBAL_DISCUSSION_NAMESPACE && session.owner_id == "global",
                "全局输入归属不符",
            )?;
            (MaterialOwner::Global, None)
        } else {
            let owner = WorkOwnerFacts::read(self.store.as_ref(), &session.owner_id)?;
            require(owner.project == project, "输入工作归属跨项目")?;
            prepared.read_set = prepared
                .read_set
                .merge(owner.read_set.versions().iter().cloned())?;
            (owner.material, Some(owner.owner.revision))
        };
        let tid = stable_object_id(self.store.as_ref(), &c.command_id, "conversation-turn")?;
        let task = prepared
            .task_source
            .as_ref()
            .map(|source| {
                let task: Value = serde_json::from_str(&prepared.input)
                    .map_err(|e| err("task_contract_mismatch", e.to_string()))?;
                let record = ConfiguredAgentTask {
                    renderer_version: task["renderer_version"]
                        .as_str()
                        .ok_or_else(|| err("task_contract_mismatch", "正式输入缺少 renderer"))?
                        .into(),
                    turn_id: tid.clone(),
                    agent_revision_id: session.agent.clone(),
                    source: source.data.clone(),
                    digest: configuration_hash(&task),
                    task,
                    rendered_input: prepared.input.clone(),
                };
                record.validate()?;
                Ok::<_, Error>(record)
            })
            .transpose()?;
        let snapshot_id = stable_object_id(
            self.store.as_ref(),
            &c.command_id,
            "conversation-input-snapshot",
        )?;
        let (project_id, feature_id, task_id) = material_parts(&material);
        let snapshot = Snapshot {
            id: snapshot_id.clone(),
            project_id,
            feature_id,
            task_id,
            kind: "conversation_input".into(),
            content: serde_json::to_string(&facts.content)
                .map_err(|e| err("serialization", e.to_string()))?,
            documents: if formal {
                vec![]
            } else {
                facts.documents.clone()
            },
            inherited_from: session.snapshot_id.clone(),
            capture: SnapshotCapture {
                step_execution_ids: prepared
                    .task_source
                    .iter()
                    .filter_map(|s| s.data["execution_id"].as_str().map(str::to_owned))
                    .collect(),
                sessions: vec![captured_session.id.clone()],
                captured_at: Some(now()),
                reason: "Frozen input before dispatch".into(),
            },
            runtime_bindings: session.runtime_binding.iter().cloned().collect(),
            execution_instances: facts.execution_ids.clone(),
        };
        let snapshot_body = serde_json::to_string_pretty(&snapshot)
            .map_err(|e| err("serialization", e.to_string()))?;
        let input_seal = ArtifactSeal::prepare(
            stable_object_id(
                self.store.as_ref(),
                &c.command_id,
                "conversation-input-artifact",
            )?,
            &material,
            "conversation",
            &prepared.input,
            prepared.origin,
        )?;
        let snapshot_seal = ArtifactSeal::prepare(
            stable_object_id(
                self.store.as_ref(),
                &c.command_id,
                "conversation-context-artifact",
            )?,
            &material,
            "snapshots",
            &snapshot_body,
            Origin::External,
        )?;
        let turn = ConversationTurn {
            id: tid.clone(),
            project_id: project.into(),
            session_id: sid.clone(),
            client_message_id: prepared.client.clone(),
            input: prepared.input.clone(),
            input_digest: input_seal.artifact.sha256.clone(),
            input_snapshot: Some(snapshot_id.clone()),
            code_view: prepared.code_view,
            source_revision,
            status: TurnStatus::Queued,
            external_message: None,
            external_turn: None,
            consumed: false,
            cancel_requested: false,
            started: None,
            finished: None,
            revision: 1,
        };
        let mut turn_value = self.store.encode_runtime_value(&json!(turn))?;
        if let Some(task) = &task {
            let (data, encoded_input) = self.store.encode_execution_task(task)?;
            turn_value["input"] = json!(encoded_input);
            let mut task_record = record(
                "configured_execution_task",
                tid.clone(),
                prepared.task_source.as_ref().unwrap().scope.clone(),
                "实际步骤任务".into(),
                1,
                None,
                data,
                true,
            );
            task_record.id = format!("configured-task:{tid}");
            let key = ResourceKey::new("governance", &task_record.id);
            prepared.read_set = prepared.read_set.merge([ResourceVersion {
                resource: key.clone(),
                revision: 0,
            }])?;
            prepared.changes.push(ResourceChange {
                resource: key,
                value: task_record.to_storage(),
            });
        }
        // The full user ledger body is the authenticated record of what was
        // actually accepted. The task/Turn rows retain compact source markers.
        let event_id = stable_object_id(
            self.store.as_ref(),
            &c.command_id,
            "conversation-input-event",
        )?;
        let event = ConversationEvent {
            id: event_id.clone(),
            session_id: sid.clone(),
            turn_id: tid.clone(),
            sequence: 0,
            deduplication_key: format!("input:{}", prepared.client),
            kind: ConversationEventKind::User,
            content: prepared.input.clone(),
            artifact_id: Some(input_seal.artifact.id.clone()),
            time: now(),
            mode: MessageMode::Full,
            provider_item: None,
        };
        let turn_key = ResourceKey::new("turn", &tid);
        let snapshot_key = ResourceKey::new("snapshot", &snapshot_id);
        let event_key = ResourceKey::new("conversation_event", event_id);
        let session_next = if creating {
            require(
                prepared.original.value.is_none()
                    && prepared.original.resource == ResourceKey::new("session", sid)
                    && session.revision == 1,
                "后继输入需要实际不存在的 Session 见证",
            )?;
            self.store.encode_runtime_value(&json!(session))?
        } else {
            let mut next = prepared
                .original
                .value
                .clone()
                .ok_or_else(|| err("not_found", "输入原会话缺失"))?;
            next["revision"] = json!(prepared.original.revision + 1);
            next
        };
        prepared.changes.extend([
            ResourceChange {
                resource: prepared.original.resource.clone(),
                value: session_next,
            },
            ResourceChange {
                resource: turn_key.clone(),
                value: turn_value,
            },
            ResourceChange {
                resource: snapshot_key.clone(),
                value: self.store.encode_runtime_value(&json!(snapshot))?,
            },
            ResourceChange {
                resource: event_key.clone(),
                value: json!(event),
            },
            prepared.provenance.clone(),
        ]);
        prepared.read_set = prepared.read_set.merge([
            ResourceVersion {
                resource: turn_key,
                revision: 0,
            },
            ResourceVersion {
                resource: snapshot_key,
                revision: 0,
            },
            ResourceVersion {
                resource: event_key,
                revision: 0,
            },
            ResourceVersion {
                resource: prepared.provenance.resource,
                revision: 0,
            },
        ])?;
        let scope = if session.scope == Some(SessionScope::Global) {
            Scope::global()
        } else {
            Scope::project(project)
        };
        if c.payload.get("experience_refs").is_some() {
            let mut context = record(
                "conversation_context",
                tid.clone(),
                scope.clone(),
                "讨论引用的 Agent 经验".into(),
                1,
                None,
                json!({"session_id":sid,"experience_refs":prepared.references}),
                true,
            );
            context.id = format!("conversation-context:{tid}");
            let key = ResourceKey::new("governance", &context.id);
            prepared.read_set = prepared.read_set.merge([ResourceVersion {
                resource: key.clone(),
                revision: 0,
            }])?;
            prepared.changes.push(ResourceChange {
                resource: key,
                value: self.store.encode_runtime_record(&context)?,
            });
        }
        if let Some(projection) = self.captured_snapshot_experience(
            &facts,
            session,
            &snapshot,
            &snapshot_seal,
            Some(&tid),
        )? {
            prepared.read_set = prepared.read_set.merge([ResourceVersion {
                resource: projection.resource.clone(),
                revision: 0,
            }])?;
            prepared.changes.push(projection);
        }
        let mut result = json!({"id":tid,"operation_id":tid,"accepted":true});
        if creating {
            result["effective_session_id"] = json!(sid);
            result["predecessor"] = json!(captured_session.id);
        }
        let mut files = prepared.extra_files;
        files.push((input_seal, prepared.input));
        files.push((snapshot_seal, snapshot_body));
        let files = files
            .iter()
            .map(|(seal, body)| (seal.clone(), body.as_str()))
            .collect::<Vec<_>>();
        self.seal_command_batch_changes(
            PreparedChangeSet {
                command_id: c.command_id.clone(),
                fingerprint: fingerprint.into(),
                read_set: prepared.read_set,
                changes: prepared.changes,
                result,
                time: now() as i64,
            },
            &files,
            scope,
        )
    }
}
