//! A formal Session freezes its original native grant before runtime setup.
//! Configuration publication during that setup cannot silently select a new
//! Agent, permission pair, integration closure, Actor, or parent execution.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    material_commands::{material_parts, stable_object_id},
    work_owners::WorkOwnerFacts,
    workflow_resources::{WorkflowResources, live_workflow_chain, version},
    *,
};

pub(crate) struct PreparedFormalSession {
    namespace: StoreNamespace,
    root: String,
    grant: ResourceSnapshot,
    pub request: ConfiguredAgentRequest,
    pub agent_instance: Id,
    command: Command,
    basis: ReadSet,
    source: Value,
}
impl Engine {
    pub(crate) fn prepare_formal_session(
        &self,
        c: &Command,
    ) -> Result<Option<PreparedFormalSession>> {
        if !c.payload["configured_execution_id"].is_string()
            && !c.payload["controller_id"].is_string()
        {
            return Ok(None);
        }
        let grant_key = ResourceKey::new(
            "governance",
            format!(
                "formal-session-grant:{}",
                configuration_hash(&(self.store.namespace(), self.store.root(), &c.command_id))
            ),
        );
        let original = self
            .store
            .read_resources(std::slice::from_ref(&grant_key))?
            .remove(0);
        if original.value.is_some() {
            return self.restore_formal_session(c, original).map(Some);
        }
        let (intent, intent_guard) = self
            .original_runtime_intent("session.open", c)?
            .ok_or_else(|| err("permission_denied", "正式会话需要确切持久派发意图"))?;
        let catalog = self.store.configuration_snapshot()?;
        let owner = WorkOwnerFacts::read(
            self.store.as_ref(),
            c.target
                .as_deref()
                .ok_or_else(|| err("invalid_params", "正式会话缺少工作归属"))?,
        )?;
        owner.require_revision(c.expected_revision)?;
        let mut basis = owner.read_set.merge([intent_guard])?;
        let agent = text(&c.payload, "agent")?;
        let agent_record = catalog.state.configuration(&agent)?;
        require(
            agent_record.r#type == "agent" && agent_record.immutable,
            "正式会话需要确切 Agent 源",
        )?;
        let request;
        let source;
        let scope = Scope::project(&owner.project);
        if let Some(id) = c.payload["configured_execution_id"].as_str() {
            require(
                c.payload.get("controller_id").is_none()
                    && intent.definition_id == id
                    && intent.data["configured"] == true,
                "正式会话存在不匹配的派发来源",
            )?;
            let authority: crate::tool_calls::WorkflowToolAuthority =
                serde_json::from_value(intent.data["execution_authority"].clone())
                    .map_err(|e| err("storage", e.to_string()))?;
            let instance = authority
                .instances
                .first()
                .ok_or_else(|| err("storage", "步骤意图缺少原实例"))?;
            let resources = WorkflowResources::read(self.store.as_ref(), instance, id)?;
            let (actual, guards) = live_workflow_chain(
                self.store.as_ref(),
                instance,
                id,
                StepExecutionStatus::Running,
            )?;
            require(
                actual == authority
                    && resources.instance.owner_id == owner.id
                    && resources.instance.project_id == owner.project
                    && resources.execution.agent_revision_id.as_deref() == Some(agent.as_str()),
                "正式会话不能替换原步骤资格或 Agent",
            )?;
            let step = resources
                .instance
                .plan
                .document
                .steps
                .iter()
                .find(|s| s.key == resources.execution.step)
                .ok_or_else(|| err("storage", "正式步骤定义缺失"))?;
            let (_, _, workspace) =
                self.formal_workspace(&resources.instance, &resources.execution, step)?;
            basis = basis
                .merge(resources.reads.versions().iter().cloned())?
                .merge(guards.versions().iter().cloned())?
                .merge(workspace.versions().iter().cloned())?;
            request = ConfiguredAgentRequest {
                integration_lock: resources
                    .instance
                    .plan
                    .integration_locks
                    .get(&agent)
                    .cloned()
                    .ok_or_else(|| err("configuration_required", "Agent 接入未冻结"))?,
                access: step.access.clone(),
                permissions: resources
                    .instance
                    .plan
                    .permission_locks
                    .get(&format!(
                        "{}:{}",
                        resources.instance.plan.workflow_revision_id, step.key
                    ))
                    .cloned()
                    .ok_or_else(|| err("permission_unconfigured", "步骤权限未冻结"))?,
            };
            let dispatch_key = ResourceKey::new(
                "workflow_dispatch_source",
                json!([id, resources.execution.plan_revision_id]).to_string(),
            );
            let dispatch = self
                .store
                .read_resources(std::slice::from_ref(&dispatch_key))?
                .remove(0);
            let dispatch_row = ConfigurationRecord::from_storage(
                dispatch
                    .value
                    .clone()
                    .ok_or_else(|| err("permission_denied", "正式会话缺少原步骤派发"))?,
            )?;
            let decision = self
                .store
                .read_resources(&[ResourceKey::new(
                    "governance",
                    text(&dispatch_row.data, "decision_id")?,
                )])?
                .remove(0);
            let decision_row = ConfigurationRecord::from_storage(
                decision
                    .value
                    .clone()
                    .ok_or_else(|| err("permission_denied", "正式会话缺少原派发决定"))?,
            )?;
            let decided: WorkflowDecision = decision_row.decode()?;
            require(
                dispatch_row.r#type == "workflow_dispatch"
                    && dispatch_row.immutable
                    && decision_row.r#type == "workflow_decision"
                    && decision_row.immutable
                    && dispatch_row.scope == resources.record.scope
                    && decision_row.scope == resources.record.scope
                    && decided.instance_id == *instance
                    && decided.root_instance_id == resources.instance.root_instance_id
                    && decided.execution_id.as_deref() == Some(id)
                    && decided.plan_revision_id == resources.execution.plan_revision_id
                    && decided.operation == "dispatch",
                "正式会话派发决定不属于原步骤",
            )?;
            basis = basis.merge([version(&dispatch), version(&decision)])?;
            source = json!({"kind":"workflow","intent_id":intent.id,"instance_id":instance,"execution_id":id,"plan_revision_id":resources.execution.plan_revision_id,"workflow_revision_id":resources.instance.workflow_ref,"step":resources.execution.step,"scope":resources.record.scope,"execution_authority":actual,
                "origin":{"kind":"runtime_coordinator","workflow_intent_id":intent.id,"dispatch_id":dispatch_row.id,"decision_id":decision_row.id,"actor":decided.actor}});
        } else {
            let id = text(&c.payload, "controller_id")?;
            let row = self
                .store
                .read_resources(&[ResourceKey::new("governance", &id)])?
                .remove(0);
            let record = ConfigurationRecord::from_storage(
                row.value
                    .clone()
                    .ok_or_else(|| err("not_found", "原控制器缺失"))?,
            )?;
            let actual: WorkflowController = record.decode()?;
            require(
                record.r#type == "workflow_controller"
                    && record.scope == scope
                    && actual.owner_id == owner.id
                    && actual.project_id == owner.project
                    && actual.status == "active"
                    && c.payload["controller_epoch"] == actual.epoch
                    && actual.agent_revision_id == agent
                    && intent.data["controller"] == true
                    && intent.definition_id == id,
                "控制器会话的原授权已失效",
            )?;
            let composition = catalog.state.resolve_agent(&agent)?;
            let role = crate::configuration::admitted_controller_role(&catalog.state, &actual)?;
            require(
                composition.role == role.definition_id,
                "编排 Agent 角色不匹配",
            )?;
            let permissions = resolve_permission_lock(
                &catalog.state,
                &composition.harness_ref,
                &permission_selection_with_profile(&catalog.state, &actual.harness_permission_ref)?,
            )?;
            let integration_lock =
                freeze_integrations(&catalog.state, &composition, Some(&owner.project))?;
            require(
                !integration_lock.platform_tools.is_empty(),
                "编排 Agent 缺少原生控制桥",
            )?;
            request = ConfiguredAgentRequest {
                permissions,
                integration_lock,
                access: StepAccess::default(),
            };
            basis = basis.merge([version(&row)])?;
            source = json!({"kind":"controller","intent_id":intent.id,"controller_id":id,"epoch":actual.epoch,"origin":{"kind":"runtime_coordinator","workflow_intent_id":intent.id,"controller_id":id,"epoch":actual.epoch}});
        }
        validate_frozen_permission_lock(&catalog.state, &request.permissions)?;
        let selected = opt(&c.payload, "agent_instance_id")
            .and_then(|id| catalog.record(&id))
            .or_else(|| {
                catalog.state.governance.iter().find(|r| {
                    r.r#type == "agent_instance"
                        && r.data["agent_revision_id"] == agent
                        && r.scope.visible_from(&scope)
                        && !configuration_retired(&catalog.state, r)
                        && catalog.is_head(r)
                })
            });
        let mut changes = Vec::new();
        let mut created = None;
        let instance = if let Some(selected) = selected {
            require(
                selected.r#type == "agent_instance"
                    && selected.scope.visible_from(&scope)
                    && selected.data["agent_revision_id"] == agent
                    && !configuration_retired(&catalog.state, selected)
                    && catalog.is_head(selected),
                "Agent 实例与原派发不符或已撤销",
            )?;
            selected.id.clone()
        } else {
            require(
                c.payload.get("agent_instance_id").is_none(),
                "指定的 Agent 实例不存在",
            )?;
            let mut row = record(
                "agent_instance",
                format!("agent-instance.{}:agent-instance", c.command_id),
                scope.clone(),
                agent_record.name.clone(),
                1,
                None,
                json!(AgentInstance {
                    agent_revision_id: agent.clone(),
                    scope: scope.clone(),
                    source_instance_id: None
                }),
                true,
            );
            row.id = format!("{}:1", row.definition_id);
            let key = ResourceKey::new("governance", &row.id);
            created = Some(ResourceVersion {
                resource: key.clone(),
                revision: 0,
            });
            changes.push(ResourceChange {
                resource: key,
                value: self.store.encode_runtime_record(&row)?,
            });
            row.id
        };
        let freeze_reads = basis.merge(created.iter().cloned())?;
        // Configuration preparation does not pin mutable progress, budget or
        // pause epochs. The task/source basis stays original; admission takes
        // a fresh live lease for this exact immutable request after setup.
        basis = ReadSet::new(
            basis
                .versions()
                .iter()
                .filter(|v| {
                    !matches!(
                        v.resource.kind.as_str(),
                        "workflow_step" | "workflow_header" | "workflow_run_authority"
                    ) && !(source["kind"] == "controller"
                        && v.resource
                            == ResourceKey::new(
                                "governance",
                                source["controller_id"].as_str().unwrap_or(""),
                            ))
                })
                .cloned()
                .collect(),
        )?;
        if source["kind"] == "controller" {
            let authority = self
                .store
                .read_resources(&[ResourceKey::new(
                    "controller_authority",
                    text(&source, "controller_id")?,
                )])?
                .remove(0);
            let raw = authority
                .value
                .as_ref()
                .ok_or_else(|| err("not_found", "原经理资格缺失"))?;
            require(
                raw["data"]["epoch"] == source["epoch"] && raw["data"]["status"] == "active",
                "原经理资格已变化",
            )?;
            basis = basis.merge([version(&authority)])?;
        }

        let selected_revision = if created.is_some() {
            1
        } else {
            catalog.state.configuration(&instance)?.revision
        };
        basis = basis.merge([ResourceVersion {
            resource: ResourceKey::new("governance", &instance),
            revision: selected_revision,
        }])?;
        // This is the atomic configuration freeze. Subsequent runtime setup
        // consumes this exact durable choice, including its original authority.
        let mut grant = record(
            "formal_session_preparation",
            c.command_id.clone(),
            scope,
            "正式会话的原配置冻结".into(),
            1,
            None,
            json!({"status":"prepared","command":c,"basis":basis,"request":{"integration_lock":request.integration_lock,"access":request.access,"permissions":request.permissions},"agent_instance_id":instance,"source":source,"catalog_fingerprint":catalog.fingerprint}),
            false,
        );
        grant.id = grant_key.id.clone();
        let read_set = freeze_reads
            .merge(
                basis
                    .versions()
                    .iter()
                    .filter(|v| v.resource.kind == "controller_authority")
                    .cloned(),
            )?
            .merge([
                version(&original),
                ResourceVersion {
                    resource: ResourceKey::new("catalog_clock", "configuration"),
                    revision: catalog.revision,
                },
            ])?;
        changes.push(ResourceChange {
            resource: grant_key.clone(),
            value: self.store.encode_runtime_record(&grant)?,
        });
        self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("{}:formal-session-freeze", c.command_id),
            fingerprint: configuration_hash(&("formal-session-freeze/v1", c)),
            read_set,
            changes,
            result: json!({"id":grant.id}),
            time: now() as i64,
        })?;
        let saved = self
            .store
            .read_resources(std::slice::from_ref(&grant_key))?
            .remove(0);
        self.restore_formal_session(c, saved).map(Some)
    }
    fn restore_formal_session(
        &self,
        c: &Command,
        grant: ResourceSnapshot,
    ) -> Result<PreparedFormalSession> {
        let row = ConfigurationRecord::from_storage(
            self.store.restore_runtime_record(
                grant
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "原正式会话冻结缺失"))?,
            )?,
        )?;
        require(
            row.r#type == "formal_session_preparation"
                && row.data["command"] == json!(c)
                && row.data["status"] == "prepared",
            "原正式会话冻结已结束或命令被复用",
        )?;
        let request = ConfiguredAgentRequest {
            integration_lock: serde_json::from_value(
                row.data["request"]["integration_lock"].clone(),
            )
            .map_err(|e| err("storage", e.to_string()))?,
            access: serde_json::from_value(row.data["request"]["access"].clone())
                .map_err(|e| err("storage", e.to_string()))?,
            permissions: serde_json::from_value(row.data["request"]["permissions"].clone())
                .map_err(|e| err("storage", e.to_string()))?,
        };
        Ok(PreparedFormalSession {
            namespace: self.store.namespace(),
            root: self.store.root(),
            grant,
            request,
            agent_instance: text(&row.data, "agent_instance_id")?,
            command: c.clone(),
            basis: serde_json::from_value(row.data["basis"].clone())
                .map_err(|e| err("storage", e.to_string()))?,
            source: row.data["source"].clone(),
        })
    }
    pub(crate) fn admit_formal_session(
        &self,
        c: &Command,
        binding: &RuntimeBinding,
        prepared: &PreparedFormalSession,
    ) -> Result<Value> {
        require(
            prepared.namespace == self.store.namespace() && prepared.root == self.store.root(),
            "正式会话准备不能跨数据集",
        )?;
        for (key, value) in prepared
            .command
            .payload
            .as_object()
            .ok_or_else(|| err("invalid_params", "正式会话参数必须为对象"))?
        {
            require(
                c.payload.get(key) == Some(value),
                "正式会话不能改变原派发字段",
            )?;
        }
        require(
            c.command_id == prepared.command.command_id
                && c.target == prepared.command.target
                && c.expected_revision == prepared.command.expected_revision,
            "正式会话不能改变原命令身份或版本",
        )?;
        let fingerprint = command_fingerprint("session.open", &prepared.command);
        if let Some(receipt) = self.store.replay_commit(&c.command_id, &fingerprint)? {
            return Ok(receipt.result);
        }
        let configuration = binding
            .agent_configuration
            .as_ref()
            .ok_or_else(|| err("invalid_configuration", "正式会话缺少冻结 Agent"))?;
        require(
            configuration.agent_instance_id == prepared.agent_instance
                && configuration.agent_revision_id == prepared.command.payload["agent"]
                && configuration.integration_lock.as_ref()
                    == Some(&prepared.request.integration_lock)
                && binding
                    .permissions
                    .as_ref()
                    .and_then(|p| p.binding_lock.as_ref())
                    == Some(&prepared.request.permissions),
            "正式会话绑定改变了原配置冻结",
        )?;
        require(
            binding.work_type
                == if prepared.source["kind"] == "workflow" {
                    WorkType::Workflow
                } else {
                    WorkType::Orchestration
                },
            "正式会话不能替换工作类型",
        )?;
        let catalog = self.store.configuration_snapshot()?;
        let source = catalog
            .state
            .configuration(&configuration.agent_revision_id)?;
        let composition = catalog.state.resolve_agent(&source.id)?;
        let instance = catalog.state.configuration(&prepared.agent_instance)?;
        require(
            source.content_hash == configuration.composition_hash
                && instance.r#type == "agent_instance"
                && !configuration_retired(&catalog.state, instance)
                && catalog.is_head(instance)
                && instance.data["agent_revision_id"] == source.id
                && configuration.model_revision_id == composition.model_ref
                && configuration.harness_revision_id == composition.harness_ref,
            "正式会话运行身份与确切源不符",
        )?;
        let model: ModelDefinition = catalog
            .state
            .configuration(&composition.model_ref)?
            .decode()?;
        require(
            binding.model == model.model
                && binding.model_connection
                    == catalog.state.resolve_model_connection(
                        &model,
                        catalog.state.configuration(&composition.harness_ref)?,
                    )?,
            "正式会话不能替换模型连接",
        )?;
        let owner = WorkOwnerFacts::read(self.store.as_ref(), c.target.as_deref().unwrap_or(""))?;
        owner.require_revision(c.expected_revision)?;
        let mut read_set = prepared
            .basis
            .merge(owner.read_set.versions().iter().cloned())?
            .merge([
                version(&prepared.grant),
                ResourceVersion {
                    resource: ResourceKey::new("catalog_clock", "configuration"),
                    revision: catalog.revision,
                },
            ])?;
        if prepared.source["kind"] == "workflow" {
            let authority = serde_json::from_value(prepared.source["execution_authority"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            let live = self.store.workflow_tool_facts(&authority, &owner.project)?;
            if live.suspended {
                return Err(err(
                    "workflow_tool_suspended",
                    "原流程暂停，正式会话准备保留原冻结请求",
                ));
            }
            require(live.allowed, "正式会话原步骤资格已失效")?;
            read_set = read_set.merge(live.versions)?;
        } else {
            let row = self.store.read_resources(&[
                ResourceKey::new(
                    "controller_authority",
                    text(&prepared.source, "controller_id")?,
                ),
                ResourceKey::new(
                    "controller_lease_active",
                    json!([prepared.source["controller_id"], prepared.source["epoch"]]).to_string(),
                ),
            ])?;
            require(
                row[0].value.as_ref().is_some_and(|r| {
                    r["data"]["epoch"] == prepared.source["epoch"]
                        && r["data"]["status"] == "active"
                }) && row[1].revision == 1,
                "正式会话原经理资格或租约已失效",
            )?;
            read_set = read_set.merge(row.iter().map(version))?;
        }

        let (_, feature, task) = material_parts(&owner.material);
        let session_scope = if task.is_some() {
            SessionScope::Task
        } else if feature.is_some() {
            SessionScope::Feature
        } else {
            SessionScope::Project
        };
        let predecessor = opt(&c.payload, "predecessor");
        let snapshot = opt(&c.payload, "snapshot_id");
        if let Some(previous) = &predecessor {
            require(snapshot.is_some(), "正式后继会话需要明确继承快照")?;
            let keys = [
                ResourceKey::new("session", previous),
                ResourceKey::new("session_input_state", previous),
                ResourceKey::new("session_live_workers", previous),
            ];
            let rows = self.store.read_resources(&keys)?;
            require(
                rows[0]
                    .value
                    .as_ref()
                    .is_some_and(|s| s["project_id"] == owner.project)
                    && rows[1].value.as_ref().is_some_and(|s| s["active"] == false)
                    && rows[2]
                        .value
                        .as_ref()
                        .and_then(Value::as_array)
                        .is_some_and(Vec::is_empty),
                "原正式会话尚未静止或跨项目",
            )?;
            read_set = read_set.merge(rows.iter().map(version))?;
        }
        if let Some(snapshot) = &snapshot {
            read_set = read_set.merge(
                self.session_snapshot_scope(snapshot, &owner.project, &owner.id)?
                    .versions()
                    .iter()
                    .cloned(),
            )?;
        }
        let id = stable_object_id(self.store.as_ref(), &c.command_id, "session")?;
        let work_owner = owner.work_reference();
        let session = Session {
            id: id.clone(),
            // The backend session partition remains project-scoped for entities.
            // The immutable formal association retains the logical work owner.
            owner_id: if matches!(work_owner, WorkReference::Entity { .. }) {
                owner.project.clone()
            } else {
                owner.id
            },
            agent: source.id.clone(),
            harness: binding.installation.id.clone(),
            sandbox: format!("docker-image:{}", binding.installation.id),
            snapshot_id: snapshot,
            instance: None,
            origin: Origin::External,
            project_id: Some(owner.project.clone()),
            feature_id: feature,
            task_id: task,
            scope: Some(session_scope),
            lifecycle: Some(SessionLifecycle::Prepared),
            runtime_binding: Some(binding.clone()),
            external_session: None,
            predecessor,
            revision: 1,
        };
        let mut changes = vec![ResourceChange {
            resource: ResourceKey::new("session", &id),
            value: self.store.encode_runtime_value(&json!(session))?,
        }];
        let scope = Scope::project(&owner.project);
        let mut link = record(
            "agent_session",
            id.clone(),
            scope.clone(),
            instance.name.clone(),
            1,
            None,
            json!({"agent_instance_id":prepared.agent_instance,"agent_revision_id":source.id,"composition_hash":source.content_hash,"model_ref":composition.model_ref,"harness_ref":composition.harness_ref,"binding":binding,"step_execution_id":prepared.source["execution_id"],"work_owner":work_owner}),
            true,
        );
        link.id = format!("agent-session:{id}");
        changes.push(ResourceChange {
            resource: ResourceKey::new("governance", &link.id),
            value: self.store.encode_runtime_record(&link)?,
        });
        let mut association = if prepared.source["kind"] == "workflow" {
            record(
                "configured_session",
                id.clone(),
                serde_json::from_value(prepared.source["scope"].clone())
                    .map_err(|e| err("storage", e.to_string()))?,
                "步骤任务来源".into(),
                1,
                None,
                json!({"execution_id":prepared.source["execution_id"],"instance_id":prepared.source["instance_id"],"plan_revision_id":prepared.source["plan_revision_id"],"workflow_revision_id":prepared.source["workflow_revision_id"],"step":prepared.source["step"],"work_owner":work_owner}),
                true,
            )
        } else {
            record(
                "controller_session",
                id.clone(),
                scope.clone(),
                "项目经理会话".into(),
                1,
                None,
                json!({"controller_id":prepared.source["controller_id"],"epoch":prepared.source["epoch"],"work_owner":work_owner}),
                true,
            )
        };
        association.id = format!("formal-session-source:{id}");
        changes.push(ResourceChange {
            resource: ResourceKey::new("governance", &association.id),
            value: self.store.encode_runtime_record(&association)?,
        });
        let mut audit = record(
            "command_context",
            c.command_id.clone(),
            scope,
            "session.open".into(),
            1,
            None,
            json!({"origin":prepared.source["origin"],"formal_session_preparation_id":prepared.grant.resource.id,"target":c.target,"request_digest":fingerprint}),
            true,
        );
        audit.id = format!("command-context:{}", configuration_hash(&c.command_id));
        changes.push(ResourceChange {
            resource: ResourceKey::new("governance", &audit.id),
            value: audit.to_storage(),
        });
        for change in &changes {
            read_set = read_set.merge([ResourceVersion {
                resource: change.resource.clone(),
                revision: 0,
            }])?;
        }
        let mut grant = ConfigurationRecord::from_storage(prepared.grant.value.clone().unwrap())?;
        let mut data = grant.data.clone();
        data["status"] = json!("committed");
        data["session_id"] = json!(id);
        update_record(&mut grant, data);
        changes.push(ResourceChange {
            resource: prepared.grant.resource.clone(),
            value: grant.to_storage(),
        });
        self.store
            .commit_changes(&PreparedChangeSet {
                command_id: c.command_id.clone(),
                fingerprint,
                read_set,
                changes,
                result: json!({"id":id,"reused":false,"revision":1}),
                time: now() as i64,
            })
            .map(|r| r.result)
    }
}
