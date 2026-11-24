//! Formal input uses its actual immutable workflow grant or an explicit Human
//! supplement to a waiting step. No coordinator is recorded as a Human.
use crate::{
    concurrency::*,
    conversation_admission::NativeConversationInput,
    governance::record,
    material_commands::stable_object_id,
    workflow_resources::{WorkflowResources, live_workflow_chain, version},
    *,
};

impl Engine {
    pub(crate) fn original_input_intent(
        &self,
        c: &Command,
    ) -> Result<Option<(ConfigurationRecord, ResourceVersion)>> {
        self.original_runtime_intent("conversation.send", c)
    }
    pub(crate) fn original_runtime_intent(
        &self,
        method: &str,
        c: &Command,
    ) -> Result<Option<(ConfigurationRecord, ResourceVersion)>> {
        let key = ResourceKey::new("workflow_command_intent", &c.command_id);
        let row = self
            .store
            .read_resources(std::slice::from_ref(&key))?
            .into_iter()
            .next()
            .ok_or_else(|| err("storage", "原输入意图索引读取不完整"))?;
        let Some(raw) = row.value else {
            return Ok(None);
        };
        let stored = ConfigurationRecord::from_storage(raw)?;
        let facts = self
            .store
            .workflow_intent_facts(&stored.id, false)?
            .ok_or_else(|| err("not_found", "原输入意图缺失"))?;
        require(
            facts.row.revision == row.revision
                && facts.record.data["method"] == method
                && facts.record.data["command"] == json!(c)
                && facts.record.data["status"] == "prepared",
            "调用不能替换原工作流意图或原命令",
        )?;
        Ok(Some((
            facts.record,
            ResourceVersion {
                resource: ResourceKey::new("governance", stored.id),
                revision: row.revision,
            },
        )))
    }
    pub(crate) fn workflow_command_origin(
        &self,
        method: &str,
        c: &Command,
        resources: &WorkflowResources,
        intent: &ConfigurationRecord,
        read_set: &mut ReadSet,
    ) -> Result<ResourceChange> {
        let key = ResourceKey::new(
            "workflow_dispatch_source",
            json!([resources.execution.id, resources.execution.plan_revision_id]).to_string(),
        );
        let dispatch = self
            .store
            .read_resources(std::slice::from_ref(&key))?
            .into_iter()
            .next()
            .ok_or_else(|| err("storage", "输入原派发读取不完整"))?;
        let dispatch_record = ConfigurationRecord::from_storage(
            dispatch
                .value
                .clone()
                .ok_or_else(|| err("permission_denied", "输入缺少真实派发依据"))?,
        )?;
        let decision_key =
            ResourceKey::new("governance", text(&dispatch_record.data, "decision_id")?);
        let decision = self
            .store
            .read_resources(std::slice::from_ref(&decision_key))?
            .into_iter()
            .next()
            .ok_or_else(|| err("storage", "原派发决定读取不完整"))?;
        let row = ConfigurationRecord::from_storage(
            decision
                .value
                .clone()
                .ok_or_else(|| err("permission_denied", "原派发决定缺失"))?,
        )?;
        let decided: WorkflowDecision = row.decode()?;
        require(
            dispatch_record.r#type == "workflow_dispatch"
                && dispatch_record.immutable
                && row.r#type == "workflow_decision"
                && row.immutable
                && dispatch_record.scope == resources.record.scope
                && row.scope == resources.record.scope
                && decided.instance_id == resources.record.id
                && decided.execution_id.as_deref() == Some(resources.execution.id.as_str())
                && decided.plan_revision_id == resources.execution.plan_revision_id
                && decided.operation == "dispatch"
                && decided.root_instance_id == resources.instance.root_instance_id,
            "输入原派发决定不属于当前执行",
        )?;
        *read_set = read_set.merge([version(&dispatch), version(&decision)])?;
        let context =
            InvocationContext::workflow(decided.actor, Some(resources.instance.project_id.clone()));
        let decision_id = row.id.clone();
        let mut provenance = context.prepare_record(method, c);
        let mut row = ConfigurationRecord::from_storage(provenance.value)?;
        row.data["origin"] = json!({"kind":"runtime_coordinator","workflow_intent_id":intent.id,"dispatch_id":dispatch_record.id,"decision_id":decision_id});
        row.content_hash = content_digest(&row.refs, &row.data);
        provenance.value = row.to_storage();
        Ok(provenance)
    }
    pub(crate) fn native_formal_conversation_submission(
        &self,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
        session: Session,
        rows: &[ResourceSnapshot],
    ) -> Result<Value> {
        let sid = &session.id;
        let project = session
            .project_id
            .as_deref()
            .ok_or_else(|| err("invalid_scope", "正式输入缺少项目"))?;
        context.require_project(project)?;
        if let Some(previous) = &rows[2].value {
            let turn = text(previous, "id")?;
            let supplied = text(&c.payload, "input")?;
            let input = if rows[1]
                .value
                .as_ref()
                .is_some_and(|v| v["configured"] == true)
            {
                let task = self
                    .store
                    .execution_task_detail(&turn)?
                    .ok_or_else(|| err("not_found", "已接纳输入的原冻结任务缺失"))?;
                let full = text(&task["task"], "rendered_input")?;
                require(
                    full == supplied || task["task"]["task"]["supplemental_input"] == supplied,
                    "客户端消息 ID 被不同正式输入复用",
                )?;
                full
            } else {
                let value = self.store.restore_runtime_value(previous)?;
                let full = text(&value, "input")?;
                require(full == supplied, "客户端消息 ID 被不同编排输入复用")?;
                full
            };
            require(
                !input.is_empty() && c.payload.get("experience_refs").is_none(),
                "正式输入重放不能附加经验引用",
            )?;
            let provenance = context.prepare_record("conversation.send", c);
            return self
                .store
                .commit_changes(&PreparedChangeSet {
                    command_id: c.command_id.clone(),
                    fingerprint: fingerprint.into(),
                    read_set: ReadSet::new(vec![
                        version(&rows[2]),
                        ResourceVersion {
                            resource: provenance.resource.clone(),
                            revision: 0,
                        },
                    ])?,
                    changes: vec![provenance],
                    result: json!({"id":turn,"operation_id":turn,"replayed":true}),
                    time: now() as i64,
                })
                .map(|r| r.result);
        }
        require(
            c.expected_revision == Some(session.revision),
            "正式输入的原会话版本已变化",
        )?;
        require(
            !matches!(
                session.lifecycle,
                Some(
                    SessionLifecycle::RequiresSuccessor
                        | SessionLifecycle::Closed
                        | SessionLifecycle::NeedsReconciliation
                )
            ),
            "原会话已关闭或需要后继",
        )?;
        let state = rows[1]
            .value
            .as_ref()
            .ok_or_else(|| err("storage", "正式输入准入事实缺失"))?;
        require(
            state["active"] == false && state["permission_switch_pending"] == false,
            "正式会话正忙或正在切换",
        )?;
        require(
            ![
                "task",
                "frozen_task",
                "task_instructions",
                "task_source",
                "scenario",
            ]
            .iter()
            .any(|key| c.payload.get(key).is_some())
                && c.payload.get("experience_refs").is_none(),
            "正式输入不能替换冻结任务或附加自由讨论经验",
        )?;
        let client = text(&c.payload, "client_message_id")?;
        require(client.len() <= 160, "消息身份超出限制")?;
        let keys = [
            ResourceKey::new("configured_session_source", sid),
            ResourceKey::new("controller_session_source", sid),
            ResourceKey::new("session_live_workers", sid),
        ];
        let selected = self.store.read_resources(&keys)?;
        require(
            selected.len() == 3 && selected.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "正式输入来源读取不完整",
        )?;
        require(
            selected[2]
                .value
                .as_ref()
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty),
            "原会话仍有未撤销 worker，需要先核对",
        )?;
        let mut read_set = ReadSet::new(rows.iter().map(version).collect())?
            .merge(selected.iter().map(version))?;
        let intent = self.original_input_intent(c)?;
        let mut changes = Vec::new();
        let input;
        let provenance;
        let source;
        let code_view;
        let workspace;
        let origin;
        if let Some(raw) = &selected[0].value {
            require(
                selected[1].value.is_none() && state["configured"] == true,
                "正式会话关联存在歧义",
            )?;
            let binding = ConfigurationRecord::from_storage(raw.clone())?;
            let resources = WorkflowResources::read(
                self.store.as_ref(),
                &text(&binding.data, "instance_id")?,
                &text(&binding.data, "execution_id")?,
            )?;
            let (owner_id, owner_guard) = self.session_work_owner(&session)?;
            read_set = read_set.merge([owner_guard])?;
            require(
                resources.record.scope == binding.scope
                    && resources.instance.project_id == project
                    && resources.instance.owner_id == owner_id
                    && resources.execution.session_id.as_deref() == Some(sid.as_str())
                    && resources.execution.agent_revision_id.as_deref()
                        == Some(session.agent.as_str()),
                "原任务与实际会话归属不符",
            )?;
            let status = if intent.is_some() {
                StepExecutionStatus::Running
            } else {
                StepExecutionStatus::WaitingInput
            };
            let (authority, guards) = live_workflow_chain(
                self.store.as_ref(),
                &resources.record.id,
                &resources.execution.id,
                status,
            )?;
            read_set = read_set
                .merge(resources.reads.versions().iter().cloned())?
                .merge(guards.versions().iter().cloned())?;
            let definition = resources
                .instance
                .plan
                .document
                .steps
                .iter()
                .find(|s| s.key == resources.execution.step)
                .ok_or_else(|| err("storage", "原步骤定义缺失"))?;
            let mut execution = resources.execution.clone();
            if let Some((intent, guard)) = &intent {
                require(
                    intent.data["configured"] == true
                        && intent.definition_id == execution.id
                        && intent.data["execution_authority"] == json!(authority),
                    "原输入意图改变了步骤资格",
                )?;
                input = text(&c.payload, "input")?;
                let supplied: Value = serde_json::from_str(&input)
                    .map_err(|e| err("task_contract_mismatch", e.to_string()))?;
                let canonical: Value = serde_json::from_str(&render_source_task(
                    &resources.instance,
                    definition,
                    &execution,
                    supplied["supplemental_input"].as_str(),
                )?)
                .map_err(|e| err("serialization", e.to_string()))?;
                require(
                    supplied == canonical,
                    "原输入改变了冻结任务、来源或输出合同",
                )?;
                read_set = read_set.merge([guard.clone()])?;
                provenance = self.workflow_command_origin(
                    "conversation.send",
                    c,
                    &resources,
                    intent,
                    &mut read_set,
                )?;
                origin = Origin::External;
            } else {
                require(
                    matches!(context.actor(), WorkflowActor::Human),
                    "步骤补充输入需要实际 Human 来源",
                )?;
                let supplemental = text(&c.payload, "input")?;
                require(
                    supplemental.len() <= 65536 && execution.sequence < definition.budget.max_turns,
                    "补充输入或执行轮次超出预算",
                )?;
                let (wait_reads, wait_changes) = self.resume_native_workflow_wait(&execution)?;
                read_set = read_set.merge(wait_reads.versions().iter().cloned())?;
                changes.extend(wait_changes);
                execution.sequence += 1;
                execution.status = StepExecutionStatus::Running;
                execution.budget_wait_started = None;
                input = render_source_task(
                    &resources.instance,
                    definition,
                    &execution,
                    Some(&supplemental),
                )?;
                provenance = context.prepare_record("conversation.send", c);
                origin = Origin::Human;
            }
            execution.turn_id = Some(stable_object_id(
                self.store.as_ref(),
                &c.command_id,
                "conversation-turn",
            )?);
            changes.push(resources.changed_step(self.store.as_ref(), &execution)?);
            let (view, resource, guards) =
                self.formal_workspace(&resources.instance, &execution, definition)?;
            read_set = read_set.merge(guards.versions().iter().cloned())?;
            code_view = view;
            workspace = resource;
            source = Some(binding);
        } else {
            let raw = selected[1]
                .value
                .as_ref()
                .ok_or_else(|| err("permission_denied", "正式输入缺少会话来源"))?;
            let binding = ConfigurationRecord::from_storage(raw.clone())?;
            let (intent, guard) = intent
                .ok_or_else(|| err("permission_denied", "请通过当前项目经理入口发送编排输入"))?;
            let controller_key =
                ResourceKey::new("governance", text(&binding.data, "controller_id")?);
            let controller = self
                .store
                .read_resources(std::slice::from_ref(&controller_key))?
                .into_iter()
                .next()
                .ok_or_else(|| err("storage", "原控制器读取不完整"))?;
            let row = ConfigurationRecord::from_storage(
                controller
                    .value
                    .clone()
                    .ok_or_else(|| err("not_found", "原控制器缺失"))?,
            )?;
            let actual: WorkflowController = row.decode()?;
            require(
                row.r#type == "workflow_controller"
                    && actual.status == "active"
                    && actual.session_id.as_deref() == Some(sid.as_str())
                    && actual.project_id == project
                    && actual.epoch == binding.data["epoch"].as_u64().unwrap_or(0)
                    && intent.data["controller"] == true
                    && intent.definition_id == row.id
                    && c.payload["controller_epoch"] == actual.epoch,
                "原控制器输入授权已失效",
            )?;
            read_set = read_set.merge([guard, version(&controller)])?;
            input = text(&c.payload, "input")?;
            let mut audit = record(
                "command_context",
                c.command_id.clone(),
                Scope::project(project),
                "conversation.send".into(),
                1,
                None,
                json!({"origin":{"kind":"runtime_coordinator","controller_id":row.id,"epoch":actual.epoch,"workflow_intent_id":intent.id},"target":sid,"request_digest":fingerprint}),
                true,
            );
            audit.id = format!("command-context:{}", configuration_hash(&c.command_id));
            provenance = ResourceChange {
                resource: ResourceKey::new("governance", &audit.id),
                value: audit.to_storage(),
            };
            source = None;
            code_view = None;
            workspace = None;
            origin = Origin::External;
            let wakeup = self
                .store
                .read_resources(&[ResourceKey::new("orchestration_wakeup", &client)])?
                .remove(0);
            let mut value = wakeup
                .value
                .clone()
                .ok_or_else(|| err("not_found", "原经理唤醒缺失"))?;
            require(
                value["controller_id"] == row.id && value["status"] == "prepared",
                "经理唤醒已变化或不属于原控制器",
            )?;
            read_set = read_set.merge([version(&wakeup)])?;
            value["status"] = json!("dispatched");
            value["turn_id"] = json!(stable_object_id(
                self.store.as_ref(),
                &c.command_id,
                "conversation-turn"
            )?);
            changes.push(ResourceChange {
                resource: wakeup.resource,
                value,
            });
        }
        require(
            input.len() <= TASK_PAYLOAD_LIMIT,
            "正式输入超过任务正文预算",
        )?;
        self.admit_native_conversation_input(
            c,
            fingerprint,
            NativeConversationInput {
                session,
                original: rows[0].clone(),
                read_set,
                changes,
                provenance,
                input,
                client,
                references: json!([]),
                task_source: source,
                code_view,
                workspace,
                origin,
                capture: None,
                extra_files: vec![],
            },
        )
    }

    pub(crate) fn formal_workspace(
        &self,
        instance: &ConfiguredWorkflowInstance,
        execution: &StepExecution,
        step: &StepDefinition,
    ) -> Result<(Option<FrozenCode>, Option<FrozenResourceHandle>, ReadSet)> {
        let (resource, reads) = self.formal_input_resource(instance, execution, step)?;
        if resource.is_some() {
            return Ok((None, resource, reads));
        }
        let (git, reads) = self.formal_input_code_view(instance, execution, step)?;
        Ok((git, None, reads))
    }

    /// Only an accepted output of this frozen execution plan can supply an artifact workspace.
    fn formal_input_resource(
        &self,
        instance: &ConfiguredWorkflowInstance,
        execution: &StepExecution,
        step: &StepDefinition,
    ) -> Result<(Option<FrozenResourceHandle>, ReadSet)> {
        let Some(name) = &step.access.workspace_input else {
            return Ok((None, ReadSet::default()));
        };
        let Some(_) = execution
            .inputs
            .get(name)
            .filter(|grant| grant["kind"] == "artifact")
        else {
            return Ok((None, ReadSet::default()));
        };
        require(
            step.access.writable_paths.is_empty(),
            "封存资源不可申请写租约",
        )?;
        let Some(InputBinding::Source { source }) = step.inputs.get(name) else {
            return Err(err(
                "invalid_configuration",
                "封存资源需要实际步骤输出作为来源",
            ));
        };
        let (previous, output_id) = instance
            .steps
            .iter()
            .find(|previous| {
                previous.step == source.step && previous.status == StepExecutionStatus::Completed
            })
            .and_then(|previous| previous.outputs.get(&source.port).map(|id| (previous, id)))
            .ok_or_else(|| {
                err(
                    "input_unavailable",
                    "工作区资源需要当前流程中已接纳的原输出",
                )
            })?;
        let output = crate::platform_facts::one(self.store.as_ref(), "governance", output_id)?;
        let raw = output
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "原输出不存在"))?;
        require(
            raw["type"] == "workflow_output"
                && raw["data"]["execution_id"] == previous.id
                && raw["data"]["port"] == source.port,
            "工作区资源必须使用原步骤产物",
        )?;
        let id = text(&raw["data"], "artifact_id")?;
        let artifact = crate::platform_facts::one(self.store.as_ref(), "artifact", &id)?;
        let digest = text(
            artifact
                .value
                .as_ref()
                .ok_or_else(|| err("not_found", "封存资源不存在"))?,
            "sha256",
        )?;
        Ok((
            Some(FrozenResourceHandle {
                project_id: instance.project_id.clone(),
                owner_id: instance.owner_id.clone(),
                version: digest,
                source: FrozenResourceSource::Artifact { id },
            }),
            ReadSet::new(vec![version(&output), version(&artifact)])?,
        ))
    }

    fn formal_input_code_view(
        &self,
        instance: &ConfiguredWorkflowInstance,
        execution: &StepExecution,
        step: &StepDefinition,
    ) -> Result<(Option<FrozenCode>, ReadSet)> {
        let Some(name) = &step.access.workspace_input else {
            return Ok((None, ReadSet::default()));
        };
        let grant = execution
            .inputs
            .get(name)
            .ok_or_else(|| err("input_unavailable", "工作空间输入缺失"))?;
        let id = text(grant, "workspace_id")?;
        let revision = text(grant, "revision")?;
        require(
            [40, 64].contains(&revision.len()) && revision.bytes().all(|b| b.is_ascii_hexdigit()),
            "正式工作空间需要精确代码提交",
        )?;
        let keys = [
            ResourceKey::new("repository_worktree", id),
            ResourceKey::new(
                "repository_worktree_owner",
                json!([instance.project_id, instance.project_id]).to_string(),
            ),
            ResourceKey::new("project", &instance.project_id),
        ];
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == 3 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "正式代码视图事实缺失",
        )?;
        let workspace: RepositoryWorktree = serde_json::from_value(
            rows[0]
                .value
                .clone()
                .ok_or_else(|| err("not_found", "工作副本缺失"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        let target: RepositoryWorktree = serde_json::from_value(
            rows[1]
                .value
                .clone()
                .ok_or_else(|| err("configuration_required", "项目主干工作空间缺失"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        let project: Project = serde_json::from_value(
            rows[2]
                .value
                .clone()
                .ok_or_else(|| err("not_found", "项目缺失"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        let mut guards = ReadSet::new(rows.iter().map(version).collect())?;
        let allowed = if workspace.owner_id == instance.owner_id
            || workspace.owner_id == instance.project_id
        {
            true
        } else {
            let row = self
                .store
                .read_resources(&[ResourceKey::new("task", &workspace.owner_id)])?
                .remove(0);
            let allowed = row
                .value
                .as_ref()
                .is_some_and(|t| t["feature_id"] == instance.owner_id);
            guards = guards.merge([version(&row)])?;
            allowed
        };
        require(
            allowed
                && workspace.project_id == instance.project_id
                && target.project_id == instance.project_id
                && (step.access.writable_paths.is_empty()
                    || matches!(workspace.kind, WorktreeKind::Feature | WorktreeKind::Task)),
            "正式代码工作空间不属于实际执行范围",
        )?;
        Ok((
            Some(FrozenCode {
                revision,
                context: GitContext {
                    project: instance.project_id.clone(),
                    workspace: project
                        .workspace_definition
                        .ok_or_else(|| err("configuration_required", "项目仓库未登记"))?,
                    source: CodeLocation {
                        owner: workspace.owner_id,
                        branch: workspace.branch,
                        worktree: workspace.path,
                    },
                    target: CodeLocation {
                        owner: target.owner_id,
                        branch: target.branch,
                        worktree: target.path,
                    },
                    origin: Origin::External,
                },
            }),
            guards,
        ))
    }
}
