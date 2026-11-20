//! Permission adoption freezes its original request before stopping old writers.
use crate::{
    concurrency::*,
    conversation_admission::NativeConversationInput,
    governance::{record, update_record},
    material_commands::{material_parts, stable_object_id},
    work_owners::WorkOwnerFacts,
    workflow_resources::{WorkflowResources, live_workflow_chain, version},
    *,
};

pub fn permission_switch_id(command: &str) -> String {
    format!("permission-switch:{}", configuration_hash(&json!(command)))
}
fn compatible_binding(original: &RuntimeBinding, next: &RuntimeBinding) -> bool {
    let mut expected = original.clone();
    expected.permissions = next.permissions.clone();
    if let Some(permissions) = &next.permissions {
        expected.writable_paths = permissions.scope.writable_paths.clone();
        expected.read_only_paths = permissions.scope.read_only_paths.clone();
        expected.tool_policy = if expected.writable_paths.is_empty() {
            ToolPolicy::ReadOnly
        } else if expected.writable_paths == ["."] {
            ToolPolicy::WorkspaceWrite
        } else {
            ToolPolicy::ScopedWrite
        };
    }
    expected.configuration_digest = next.configuration_digest.clone();
    if let (Some(old), Some(next)) = (&mut expected.agent_configuration, &next.agent_configuration)
    {
        old.integration_lock = next.integration_lock.clone();
        if next.context_protocol.as_deref() == Some(AGENT_CONTEXT_PROTOCOL) {
            old.context_protocol = next.context_protocol.clone();
        }
    }
    matches!(
        original.work_type,
        WorkType::Discussion | WorkType::Workflow
    ) && expected == *next
}
struct PermissionOwner {
    project: Id,
    material: MaterialOwner,
    read_set: ReadSet,
}
impl Engine {
    fn permission_switch_owner(&self, session: &Session) -> Result<PermissionOwner> {
        if session.scope == Some(SessionScope::Global) {
            require(
                session.owner_id == "global"
                    && session.project_id.as_deref() == Some(GLOBAL_DISCUSSION_NAMESPACE)
                    && self.store.project_id().is_none(),
                "全局权限后继作用域不符",
            )?;
            return Ok(PermissionOwner {
                project: GLOBAL_DISCUSSION_NAMESPACE.into(),
                material: MaterialOwner::Global,
                read_set: ReadSet::default(),
            });
        }
        let actual = WorkOwnerFacts::read(self.store.as_ref(), &session.owner_id)?;
        Ok(PermissionOwner {
            project: actual.project,
            material: actual.material,
            read_set: actual.read_set,
        })
    }

    pub fn send_with_permission_binding(
        &self,
        c: Command,
        binding: RuntimeBinding,
    ) -> Result<Value> {
        let catalog = self.store.catalog_revision()?;
        self.send_with_permission_binding_at(c, binding, catalog)
    }
    pub fn send_with_permission_binding_at(
        &self,
        c: Command,
        binding: RuntimeBinding,
        catalog_revision: u64,
    ) -> Result<Value> {
        require(!c.command_id.is_empty(), "command_id 不能为空")?;
        let fingerprint = command_fingerprint("conversation.send", &c);
        if let Some(result) = self.replay_sealed_command(&c.command_id, &fingerprint)? {
            return Ok(result);
        }
        require(
            ![
                "task",
                "frozen_task",
                "task_instructions",
                "task_source",
                "scenario",
            ]
            .iter()
            .any(|key| c.payload.get(key).is_some()),
            "权限后继不能替换正式任务来源",
        )?;
        require(
            text(&c.payload, "input")?.len() <= 65536
                && text(&c.payload, "client_message_id")?.len() <= 160,
            "原输入或客户端身份超出限制",
        )?;
        let references = c
            .payload
            .get("experience_refs")
            .cloned()
            .unwrap_or(json!([]));
        let refs = references
            .as_array()
            .ok_or_else(|| err("invalid_params", "经验引用需要列表"))?;
        require(refs.len() <= 20, "经验引用超出限制")?;
        for reference in refs {
            text(reference, "source_project_id")?;
            text(reference, "snapshot_id")?;
        }
        let sid = c
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "需要 Session"))?;
        let intent_key = ResourceKey::new("governance", permission_switch_id(&c.command_id));
        let mut intent_row = self
            .store
            .read_resources(std::slice::from_ref(&intent_key))?
            .remove(0);
        if intent_row.value.is_none() {
            let keys = [
                ResourceKey::new("session", sid),
                ResourceKey::new("session_input_state", sid),
                ResourceKey::new("configured_session_source", sid),
                ResourceKey::new("controller_session_source", sid),
                ResourceKey::new("session_live_workers", sid),
            ];
            let rows = self.store.read_resources(&keys)?;
            require(
                rows.len() == 5 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
                "权限切换原事实不完整",
            )?;
            let session: Session = serde_json::from_value(
                self.store.restore_runtime_value(
                    rows[0]
                        .value
                        .as_ref()
                        .ok_or_else(|| err("not_found", "原 Session 不存在"))?,
                )?,
            )
            .map_err(|e| err("storage", e.to_string()))?;
            require(
                c.expected_revision == Some(session.revision),
                "Session 原版本已改变",
            )?;
            let state = rows[1]
                .value
                .as_ref()
                .ok_or_else(|| err("storage", "原会话准入状态缺失"))?;
            if state["active"] == true || state["permission_switch_pending"] == true {
                return Err(err(
                    "permission_switch_pending",
                    "原会话有活动输入或既有切换",
                ));
            }
            require(
                state["active"] == false
                    && state["permission_switch_pending"] == false
                    && !matches!(
                        session.lifecycle,
                        Some(
                            SessionLifecycle::Closed
                                | SessionLifecycle::NeedsReconciliation
                                | SessionLifecycle::RequiresSuccessor
                        )
                    ),
                "原会话有活动工作、待核对状态或既有切换",
            )?;
            require(
                rows[3].value.is_none(),
                "编排权限切换须通过版本化的经理后继入口",
            )?;
            require(
                session.scope != Some(SessionScope::Task) || rows[2].value.is_some(),
                "独立 Task 会话需要显式新建 Session 以切换配置",
            )?;
            require(
                session
                    .runtime_binding
                    .as_ref()
                    .is_some_and(|old| compatible_binding(old, &binding)),
                "Agent 身份或执行环境已变化，需要显式采用对应运行配置",
            )?;
            let owner = self.permission_switch_owner(&session)?;
            let mut basis = ReadSet::new(vec![
                version(&rows[0]),
                version(&rows[2]),
                version(&rows[3]),
            ])?
            .merge(owner.read_set.versions().iter().cloned())?;
            require(
                session.project_id.as_deref() == Some(owner.project.as_str()),
                "权限切换跨项目",
            )?;
            if let Some(source) = &rows[2].value {
                require(
                    c.payload.get("experience_refs").is_none(),
                    "正式后继不能附加自由讨论经验",
                )?;
                let source = ConfigurationRecord::from_storage(source.clone())?;
                let resources = WorkflowResources::read(
                    self.store.as_ref(),
                    &text(&source.data, "instance_id")?,
                    &text(&source.data, "execution_id")?,
                )?;
                let (_, authority) = live_workflow_chain(
                    self.store.as_ref(),
                    &resources.record.id,
                    &resources.execution.id,
                    StepExecutionStatus::WaitingInput,
                )?;
                require(
                    resources.execution.session_id.as_deref() == Some(sid),
                    "权限切换的原步骤会话已失效",
                )?;
                basis = basis
                    .merge(resources.reads.versions().iter().cloned())?
                    .merge(authority.versions().iter().cloned())?;
            }
            let catalog = self.store.configuration_snapshot()?;
            require(
                catalog.revision == catalog_revision,
                "权限准备期间配置已变化，请保留原请求",
            )?;
            validate_frozen_permissions(
                &catalog.state,
                binding
                    .permissions
                    .as_ref()
                    .ok_or_else(|| err("permission_unconfigured", "新权限缺少确切绑定"))?,
            )?;
            let mut intent = record(
                "permission_switch",
                sid.into(),
                if owner.material == MaterialOwner::Global {
                    Scope::global()
                } else {
                    Scope::project(&owner.project)
                },
                "原请求的权限后继".into(),
                1,
                None,
                json!({"status":"prepared","command":c,"fingerprint":fingerprint,"binding":binding,"basis":basis,"session_state":state,"origin":{"kind":"human","project_id":owner.project}}),
                false,
            );
            intent.id = intent_key.id.clone();
            let reads = basis.merge([
                version(&rows[1]),
                version(&rows[4]),
                version(&intent_row),
                ResourceVersion {
                    resource: ResourceKey::new("catalog_clock", "configuration"),
                    revision: catalog_revision,
                },
            ])?;
            self.store.commit_changes(&PreparedChangeSet {
                command_id: format!("{}:permission-freeze", c.command_id),
                fingerprint: configuration_hash(&("permission-freeze/v1", &c)),
                read_set: reads,
                changes: vec![ResourceChange {
                    resource: intent_key.clone(),
                    value: self.store.encode_runtime_record(&intent)?,
                }],
                result: json!({"id":intent.id}),
                time: now() as i64,
            })?;
            intent_row = self
                .store
                .read_resources(std::slice::from_ref(&intent_key))?
                .remove(0);
        }
        let intent = ConfigurationRecord::from_storage(
            self.store.restore_runtime_record(
                intent_row
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "原权限切换意图缺失"))?,
            )?,
        )?;
        require(
            intent.r#type == "permission_switch"
                && intent.data["command"] == json!(c)
                && intent.data["fingerprint"] == fingerprint
                && intent.data["binding"] == json!(binding)
                && matches!(
                    intent.data["status"].as_str(),
                    Some("prepared" | "needs_reconciliation")
                ),
            "权限切换不能替换原请求、绑定或阶段",
        )?;
        let basis: ReadSet = serde_json::from_value(intent.data["basis"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let original = self
            .store
            .read_resources(&[ResourceKey::new("session", sid)])?
            .remove(0);
        require(
            basis.require(&original.resource)? == original.revision,
            "切换原会话已变化",
        )?;
        let previous: Session = serde_json::from_value(
            self.store.restore_runtime_value(
                original
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "原会话缺失"))?,
            )?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        let live = self.store.read_resources(&[
            ResourceKey::new("session_live_workers", sid),
            ResourceKey::new("session_latest_turn", sid),
        ])?;
        let workers = live[0]
            .value
            .as_ref()
            .and_then(Value::as_array)
            .ok_or_else(|| err("storage", "原 worker 集合缺失"))?;
        if !workers.is_empty() {
            let turn = live[1]
                .value
                .as_ref()
                .ok_or_else(|| err("needs_reconciliation", "原 worker 缺少实际轮次"))?;
            let tid = text(turn, "id")?;
            if let Err(error) = self.stop_conversation_workers(&previous, &tid, &basis) {
                let mut data = intent.data.clone();
                data["status"] = json!("needs_reconciliation");
                data["reason"] = json!(error.message);
                let mut failure = intent.clone();
                update_record(&mut failure, data);
                let _ = self.store.commit_changes(&PreparedChangeSet {
                    command_id: format!("{}:stop-failed:{}", c.command_id, intent_row.revision),
                    fingerprint: configuration_hash(&(
                        "permission-stop-failure/v1",
                        &intent.id,
                        intent_row.revision,
                        &error,
                    )),
                    read_set: ReadSet::new(vec![version(&intent_row)])?,
                    changes: vec![ResourceChange {
                        resource: intent_key.clone(),
                        value: self.store.encode_runtime_record(&failure)?,
                    }],
                    result: Value::Null,
                    time: now() as i64,
                });
                return Err(error);
            }
        }
        let ready = self.store.read_resources(&[
            ResourceKey::new("session_input_state", sid),
            ResourceKey::new("session_live_workers", sid),
        ])?;
        require(
            ready[0].value.as_ref().is_some_and(|s| {
                s["active"] == false
                    && s["permission_switch_pending"] == true
                    && s["configured"] == intent.data["session_state"]["configured"]
                    && s["controller"] == false
            }) && ready[1]
                .value
                .as_ref()
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty),
            "原会话尚未静止或权限切换资格已改变",
        )?;
        let facts = self.store.conversation_input_facts(&previous, None)?;
        let owner = self.permission_switch_owner(&previous)?;
        let mut reads = basis
            .merge(facts.read_set.versions().iter().cloned())?
            .merge(ready.iter().map(version))?
            .merge([version(&intent_row)])?;
        let catalog = self.store.configuration_snapshot()?;
        let configuration = binding
            .agent_configuration
            .as_ref()
            .ok_or_else(|| err("configuration_required", "权限后继缺少原 Agent 实例"))?;
        let instance = catalog
            .state
            .configuration(&configuration.agent_instance_id)?;
        require(
            instance.r#type == "agent_instance"
                && instance.data["agent_revision_id"] == configuration.agent_revision_id
                && !configuration_retired(&catalog.state, instance)
                && catalog.is_head(instance),
            "权限后继准备期间 Agent 实例已撤销或换绑",
        )?;
        reads = reads.merge([ResourceVersion {
            resource: ResourceKey::new("catalog_clock", "configuration"),
            revision: catalog.revision,
        }])?;
        let snapshot_id = stable_object_id(
            self.store.as_ref(),
            &c.command_id,
            "permission-handoff-snapshot",
        )?;
        let (project_id, feature_id, task_id) = material_parts(&owner.material);
        let snapshot = Snapshot {
            id: snapshot_id.clone(),
            project_id,
            feature_id,
            task_id,
            kind: "permission_handoff".into(),
            content: serde_json::to_string(&facts.content)
                .map_err(|e| err("serialization", e.to_string()))?,
            documents: facts.documents.clone(),
            inherited_from: previous.snapshot_id.clone(),
            capture: SnapshotCapture {
                step_execution_ids: vec![],
                sessions: vec![sid.into()],
                captured_at: Some(now()),
                reason: "Explicit permission successor".into(),
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
                "permission-handoff-artifact",
            )?,
            &owner.material,
            "snapshots",
            &body,
            Origin::External,
        )?;
        let next_id = stable_object_id(
            self.store.as_ref(),
            &c.command_id,
            "permission-successor-session",
        )?;
        let mut next = previous.clone();
        next.id = next_id.clone();
        next.runtime_binding = Some(binding.clone());
        next.snapshot_id = Some(snapshot_id.clone());
        next.external_session = None;
        next.predecessor = Some(sid.into());
        next.lifecycle = Some(SessionLifecycle::Prepared);
        next.revision = 1;
        let absence = self
            .store
            .read_resources(&[ResourceKey::new("session", &next_id)])?
            .remove(0);
        require(
            absence.value.is_none() && absence.revision == 0,
            "权限后继身份已被占用",
        )?;
        reads = reads.merge([version(&absence)])?;
        let mut old = original.value.clone().unwrap();
        old["lifecycle"] = json!(SessionLifecycle::RequiresSuccessor);
        old["revision"] = json!(original.revision + 1);
        let snapshot_key = ResourceKey::new("snapshot", &snapshot_id);
        let mut changes = vec![
            ResourceChange {
                resource: original.resource.clone(),
                value: old,
            },
            ResourceChange {
                resource: snapshot_key.clone(),
                value: self.store.encode_runtime_value(&json!(snapshot))?,
            },
        ];
        reads = reads.merge([ResourceVersion {
            resource: snapshot_key,
            revision: 0,
        }])?;
        if let Some(projection) =
            self.captured_snapshot_experience(&facts, &previous, &snapshot, &seal, None)?
        {
            reads = reads.merge([ResourceVersion {
                resource: projection.resource.clone(),
                revision: 0,
            }])?;
            changes.push(projection);
        }
        if let Some(link) = &facts.link {
            let link = ConfigurationRecord::from_storage(
                self.store.restore_runtime_record(&link.to_storage())?,
            )?;
            let mut data = link.data.clone();
            data["binding"] = json!(binding);
            let mut link = record(
                "agent_session",
                next_id.clone(),
                link.scope,
                link.name,
                1,
                None,
                data,
                true,
            );
            link.id = format!("agent-session:{next_id}");
            let key = ResourceKey::new("governance", &link.id);
            reads = reads.merge([ResourceVersion {
                resource: key.clone(),
                revision: 0,
            }])?;
            changes.push(ResourceChange {
                resource: key,
                value: self.store.encode_runtime_record(&link)?,
            });
        }
        let (input, task_source, code_view) = if let Some(source) = &facts.workflow {
            require(
                c.payload.get("experience_refs").is_none(),
                "正式后继输入不能附加自由讨论经验",
            )?;
            let resources = WorkflowResources::read(
                self.store.as_ref(),
                &text(&source.data, "instance_id")?,
                &text(&source.data, "execution_id")?,
            )?;
            reads = reads.merge(resources.reads.versions().iter().cloned())?;
            let step = resources
                .instance
                .plan
                .document
                .steps
                .iter()
                .find(|s| s.key == resources.execution.step)
                .ok_or_else(|| err("storage", "原步骤定义缺失"))?;
            let mut execution = resources.execution.clone();
            require(
                execution.sequence < step.budget.max_turns,
                "原步骤已达到输入轮次上限",
            )?;
            let (wait_reads, wait_changes) = self.resume_native_workflow_wait(&execution)?;
            reads = reads.merge(wait_reads.versions().iter().cloned())?;
            changes.extend(wait_changes);
            execution.sequence += 1;
            execution.budget_wait_started = None;
            execution.status = StepExecutionStatus::Running;
            execution.session_id = Some(next_id.clone());
            execution.turn_id = Some(stable_object_id(
                self.store.as_ref(),
                &c.command_id,
                "conversation-turn",
            )?);
            execution.runtime_context["binding"] =
                crate::workflow_runtime_facts::public_binding(&next_id, &binding);
            let input = render_source_task(
                &resources.instance,
                step,
                &execution,
                Some(&text(&c.payload, "input")?),
            )?;
            let (view, guards) =
                self.formal_input_code_view(&resources.instance, &execution, step)?;
            reads = reads.merge(guards.versions().iter().cloned())?;
            changes.push(resources.changed_step(self.store.as_ref(), &execution)?);
            let mut association = record(
                "configured_session",
                next_id.clone(),
                source.scope.clone(),
                source.name.clone(),
                1,
                None,
                source.data.clone(),
                true,
            );
            association.id = format!("formal-session-source:{next_id}");
            let key = ResourceKey::new("governance", &association.id);
            reads = reads.merge([ResourceVersion {
                resource: key.clone(),
                revision: 0,
            }])?;
            changes.push(ResourceChange {
                resource: key,
                value: self.store.encode_runtime_record(&association)?,
            });
            (input, Some(association), view)
        } else {
            let (view, guards) = self.native_discussion_code_view(&previous)?;
            reads = reads.merge(guards.versions().iter().cloned())?;
            (text(&c.payload, "input")?, None, view)
        };
        let references = c
            .payload
            .get("experience_refs")
            .cloned()
            .unwrap_or(json!([]));
        let refs = references
            .as_array()
            .ok_or_else(|| err("invalid_params", "经验引用需要列表"))?;
        require(
            refs.len() <= 20 && text(&c.payload, "input")?.len() <= 65536,
            "原输入或经验引用超出限制",
        )?;
        for reference in refs {
            text(reference, "source_project_id")?;
            text(reference, "snapshot_id")?;
        }
        let mut adopted = intent.clone();
        let mut data = adopted.data.clone();
        data["status"] = json!("adopted");
        data["effective_session_id"] = json!(next_id);
        update_record(&mut adopted, data);
        changes.push(ResourceChange {
            resource: intent_key,
            value: self.store.encode_runtime_record(&adopted)?,
        });
        let context = InvocationContext::human(Some(owner.project));
        let provenance = context.prepare_record("conversation.send", &c);
        self.admit_native_conversation_input(
            &c,
            &fingerprint,
            NativeConversationInput {
                session: next,
                original: absence,
                read_set: reads,
                changes,
                provenance,
                input,
                client: text(&c.payload, "client_message_id")?,
                references,
                task_source,
                code_view,
                origin: Origin::Human,
                capture: Some((previous, facts)),
                extra_files: vec![(seal, body)],
            },
        )
    }
}

impl Engine {
    pub fn replay_successor_input(&self, c: &Command, next: &str) -> Result<Value> {
        use crate::concurrency::*;
        let fingerprint = command_fingerprint("conversation.send", c);
        if let Some(receipt) = self.store.replay_commit(&c.command_id, &fingerprint)? {
            return Ok(receipt.result);
        }
        let previous = c
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "缺少原会话"))?;
        let client = text(&c.payload, "client_message_id")?;
        let keys = [
            ResourceKey::new(
                "permission_transition",
                json!([previous, client]).to_string(),
            ),
            ResourceKey::new("conversation_client", json!([next, client]).to_string()),
            ResourceKey::new("session", next),
        ];
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == 3 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "权限后继重放事实不完整",
        )?;
        require(
            rows[0]
                .value
                .as_ref()
                .is_some_and(|r| r["data"]["effective_session_id"] == next),
            "输入重放不属于原权限后继",
        )?;
        let original = rows[1]
            .value
            .as_ref()
            .ok_or_else(|| err("needs_reconciliation", "后继输入记录缺失"))?;
        let turn = text(original, "id")?;
        let supplied = text(&c.payload, "input")?;
        let task = self.store.execution_task_detail(&turn)?;
        if let Some(task) = task {
            require(
                task["task"]["rendered_input"] == supplied
                    || task["task"]["task"]["supplemental_input"] == supplied,
                "客户端消息 ID 被不同正式输入复用",
            )?;
        } else {
            let value = self.store.restore_runtime_value(original)?;
            require(value["input"] == supplied, "客户端消息 ID 被不同输入复用")?;
        }
        let references = self
            .store
            .restore_runtime_value(&original["experience_refs"])?;
        require(
            references
                == c.payload
                    .get("experience_refs")
                    .cloned()
                    .unwrap_or(json!([])),
            "客户端消息 ID 被不同经验引用复用",
        )?;
        let session = rows[2]
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "后继会话缺失"))?;
        let project = session["project_id"]
            .as_str()
            .ok_or_else(|| err("invalid_scope", "后继会话缺少项目"))?;
        require(
            self.store.project_id().is_none_or(|id| id == project),
            "后继输入不能跨项目",
        )?;
        let context =
            InvocationContext::human((session["scope"] != "global").then(|| project.to_owned()));
        let provenance = context.prepare_record("conversation.send", c);
        let read_set = ReadSet::new(
            rows.iter()
                .map(|r| ResourceVersion {
                    resource: r.resource.clone(),
                    revision: r.revision,
                })
                .collect(),
        )?
        .merge([ResourceVersion {
            resource: provenance.resource.clone(),
            revision: 0,
        }])?;
        self.store.commit_changes(&PreparedChangeSet{command_id:c.command_id.clone(),fingerprint,read_set,changes:vec![provenance],result:json!({"id":turn,"operation_id":turn,"replayed":true,"effective_session_id":next}),time:now() as i64}).map(|r|r.result)
    }
}
