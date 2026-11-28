//! Application assembly of immutable Agent/Session choices. Environment facts arrive via ports.
use crate::*;
impl Engine {
    /// Import only the selected identity and exact source closure for detached discussion.
    pub fn import_discussion_agent(
        &self,
        records: &[ConfigurationRecord],
        instance: &str,
    ) -> Result<()> {
        let selected = records
            .iter()
            .find(|r| r.r#type == "agent_instance" && r.id == instance)
            .ok_or_else(|| err("not_found", "Agent 实例不存在"))?;
        self.import_discussion_source(
            records,
            instance,
            vec![
                instance.to_owned(),
                text(&selected.data, "agent_revision_id")?,
            ],
        )
    }

    pub fn import_discussion_agent_revision(
        &self,
        catalog: &ConfigurationCatalogSnapshot,
        agent: &str,
    ) -> Result<()> {
        let selected = catalog
            .record(agent)
            .filter(|r| {
                r.r#type == "agent"
                    && r.scope.is_global()
                    && !catalog.is_archived(r)
                    && !catalog.is_retired(r)
            })
            .ok_or_else(|| err("not_found", "此工作台没有可供全局讨论的 Agent 修订"))?;
        self.import_discussion_source(&catalog.state.governance, agent, vec![selected.id.clone()])
    }

    fn import_discussion_source(
        &self,
        records: &[ConfigurationRecord],
        selection: &str,
        mut pending: Vec<Id>,
    ) -> Result<()> {
        let before = self.store.configuration_snapshot()?;
        require(
            before.state.projects.is_empty(),
            "独立讨论只能装配到讨论命名空间",
        )?;
        let mut candidate = before.state.as_ref().clone();
        let state = &mut candidate;
        let mut visited = std::collections::BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id.clone()) {
                continue;
            }
            let source = records
                .iter()
                .find(|r| r.id == id)
                .ok_or_else(|| err("invalid_configuration", format!("实例依赖缺失: {id}")))?;
            require(
                source.is_definition() || source.r#type == "agent_instance",
                "讨论只能导入身份及配置源",
            )?;
            for edge in configuration_edges(source)? {
                if edge.relation != "content_digest" {
                    pending.push(edge.to);
                }
            }
            for package in records
                .iter()
                .filter(|r| r.r#type == "skill_package" && r.definition_id == source.id)
            {
                if !state.governance.iter().any(|r| r.id == package.id) {
                    state.governance.push(package.clone());
                }
            }
            if let Some(old) = state.governance.iter().find(|r| r.id == source.id) {
                require(old.content_hash == source.content_hash, "实例依赖内容冲突")?;
            } else {
                state.governance.push(source.clone());
            }
        }
        state.refresh_configuration_views()?;
        let candidate =
            ConfigurationCatalogSnapshot::new(candidate, before.revision, before.repository_ready)?;
        let publication =
            prepare_configuration_publication(self.store.as_ref(), &before, &candidate)?;
        if !publication.changes.is_empty() {
            let digest = configuration_hash(&(
                "discussion-source-import/v1",
                self.store.namespace(),
                self.store.root(),
                selection,
                before.revision,
                &candidate.fingerprint,
            ));
            self.store
                .commit_changes(&crate::concurrency::PreparedChangeSet {
                    command_id: format!("discussion-source-import:{digest}"),
                    fingerprint: digest,
                    read_set: publication.read_set,
                    changes: publication.changes,
                    result: Value::Null,
                    time: now() as i64,
                })?;
        }
        Ok(())
    }
    pub fn resolve_session_composition(
        &self,
        resources: &dyn ports::SessionResources,
        command: &mut Command,
    ) -> Result<()> {
        let formal = self.prepare_formal_session(command)?;
        self.resolve_session_composition_at(resources, command, formal.as_ref())
    }
    fn resolve_session_composition_at(
        &self,
        resources: &dyn ports::SessionResources,
        command: &mut Command,
        formal: Option<&crate::formal_sessions::PreparedFormalSession>,
    ) -> Result<()> {
        let e = self;

        let catalog = e.store.configuration_snapshot()?;
        let state = catalog.state.as_ref();
        let selected = formal
            .map(|prepared| prepared.agent_instance.clone())
            .or_else(|| {
                command.payload["agent_instance_id"]
                    .as_str()
                    .map(str::to_owned)
            });
        let agent_id = if let Some(instance) = &selected {
            let r = state.configuration(instance)?;
            require(
                r.r#type == "agent_instance" && !configuration_retired(state, r),
                "无效或已移除 Agent 实例",
            )?;
            r.data["agent_revision_id"]
                .as_str()
                .ok_or_else(|| err("blocked", "Agent 缺少组合"))?
                .to_owned()
        } else {
            command.payload["agent"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| err("invalid_params", "请选择完整 Agent 或 AgentID"))?
        };
        let Some(record) = state
            .governance
            .iter()
            .find(|r| r.r#type == "agent" && r.id == agent_id)
        else {
            require(
                !state
                    .projects
                    .iter()
                    .any(|project| project.execution_mode == "live"),
                "新原生 Session 需要完整 AgentRevision 和 Agent；历史角色定义不能分开覆盖 Model/Harness",
            )?;
            return Ok(());
        };
        let agent = state.resolve_agent(&record.id)?;
        let owner = command.target.as_deref().unwrap_or("");
        let configured = formal.map(|prepared| prepared.request.clone());
        let frozen = configured.is_some();
        require(
            frozen
                || (!configuration_retired(state, record)
                    && configuration_visible(
                        state,
                        record,
                        selected
                            .as_ref()
                            .and_then(|id| state.configuration(id).ok())
                            .and_then(|r| r.scope.project_id())
                            .or_else(|| state.projects.first().map(|p| p.id.as_str())),
                    )),
            "Agent 及其 Role/Capability 不适用于此项目，已有 Feature 保留冻结配置",
        )?;

        if let Some(role_ref) = &agent.role_ref {
            let role = state.configuration(role_ref)?;
            let retired = state
                .governance
                .iter()
                .any(|r| r.r#type == "role_retirement" && r.definition_id == role.definition_id);
            require(
                !retired || frozen,
                "Role 已删除，新的项目讨论不能再采用它；已有 Feature 保留冻结版本",
            )?;
        }
        let model: ModelDefinition = state.configuration(&agent.model_ref)?.decode()?;
        let harness_record = state.configuration(&agent.harness_ref)?.clone();
        let harness: HarnessDefinition = harness_record.decode()?;
        for key in [
            "model",
            "role_ref",
            "provider",
            "permission_overrides",
            "installation_id",
        ] {
            require(
                command.payload.get(key).is_none(),
                "Agent 执行不能单独覆盖 Role/Model/Harness/Permission，请采用新的 Agent 版本",
            )?;
        }
        let project = state.projects.first();
        require(
            project.is_some() || owner == "global",
            "Agent 执行需要项目或全局讨论范围",
        )?;
        let scope = if owner == "global" {
            selected
                .as_ref()
                .and_then(|id| state.configuration(id).ok())
                .map(|r| r.scope.clone())
                .unwrap_or_default()
        } else {
            project.map_or(Scope::global(), |p| Scope::project(p.id.clone()))
        };
        let title = project.map_or("全局 / 跨项目", |p| p.title.as_str());
        let instance = if let Some(instance) = selected {
            let r = state.configuration(&instance)?;
            let i: AgentInstance = r.decode()?;
            require(i.scope.visible_from(&scope), "Agent 不属于当前项目")?;
            instance
        } else if let Some(r) = state.governance.iter().find(|r| {
            r.r#type == "agent_instance"
                && r.data["agent_revision_id"] == agent_id
                && !configuration_retired(state, r)
                && !state.governance.iter().any(|next| {
                    next.r#type == "agent_instance"
                        && next.definition_id == r.definition_id
                        && next.version > r.version
                })
                && r.scope.visible_from(&scope)
        }) {
            r.id.clone()
        } else {
            e.execute("agent.instance.create",Command{command_id:format!("{}:agent-instance",command.command_id),target:None,expected_revision:None,payload:json!({"name":format!("{} · {}",record.name,title),"scope":scope,"agent_revision_id":agent_id})})?["id"].as_str().unwrap().into()
        };
        state.resolve_model_connection(&model, &harness_record)?;
        let materialized = if let Some(configured) = &configured {
            e.materialize_harness_in_sandbox(
                resources,
                &harness_record,
                &configured.permissions.sandbox.binding.target.revision_id,
            )?
        } else {
            e.materialize_harness(resources, &harness_record)?
        };
        command.payload["installation_id"] = json!(materialized.id);
        let integration_lock = match configured {
            Some(configured) => configured.integration_lock,
            None => freeze_discussion_integrations(
                state,
                &agent,
                state.projects.first().map(|p| p.id.as_str()),
            )?,
        };
        command.payload["agent_configuration"] = json!(AgentExecutionConfiguration {
            context_protocol: Some(AGENT_CONTEXT_PROTOCOL.into()),
            integration_lock: Some(integration_lock),
            agent_instance_id: instance.clone(),
            agent_revision_id: agent_id.clone(),
            composition_hash: record.content_hash.clone(),
            model_revision_id: agent.model_ref.clone(),
            harness_revision_id: agent.harness_ref.clone(),
            model_provider: model.provider.clone(),
            model_parameters: model.parameters.clone(),
            tool_revisions: agent.tools.clone()
        });
        command.payload["agent"] = json!(agent_id);
        command.payload["agent_instance_id"] = json!(instance);
        command.payload["model"] = json!(model.model);
        command.payload["provider"] = json!(harness.provider);
        Ok(())
    }
    pub fn open_prepared_session(
        &self,
        resources: &dyn ports::SessionResources,
        mut command: Command,
    ) -> Result<Value> {
        let e = self;

        require(!command.command_id.is_empty(), "command_id 不能为空")?;
        require(
            command.payload.get("binding").is_none()
                && command.payload.get("prepared_catalog_revision").is_none(),
            "会话绑定由协调器解析，不能由客户端提供",
        )?;
        let fingerprint = command_fingerprint("session.open", &command);
        if let Some(previous) = e.store.replay(&command.command_id, &fingerprint)? {
            return Ok(previous);
        }
        if let Some(source) = command.payload.get("continue_from") {
            require(
                source.is_object()
                    && source["session_id"].is_string()
                    && command.payload["configured_execution_id"].is_null()
                    && command.payload["controller_id"].is_null()
                    && command.payload["agent_instance_id"].is_string(),
                "续聊需要明确来源和 Agent 实例，并作为独立讨论打开",
            )?;
        }
        let formal = e.prepare_formal_session(&command)?;
        let configured = formal.as_ref().map(|prepared| prepared.request.clone());
        e.resolve_session_composition_at(resources, &mut command, formal.as_ref())?;
        let catalog = e.store.configuration_snapshot()?;
        let state = catalog.state.as_ref();
        let agent = command.payload["agent"]
            .as_str()
            .ok_or_else(|| err("invalid_params", "需要 Agent 修订 ID"))?;
        let definition = state
            .definitions
            .iter()
            .find(|d| d.id == agent && d.agent_profile.is_some())
            .ok_or_else(|| err("not_found", "Agent 角色修订不存在"))?;
        let work_type: WorkType = command
            .payload
            .get("work_type")
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()
            .map_err(|_| err("invalid_params", "无效工作类型"))?
            .unwrap_or(WorkType::Discussion);
        require(
            configured.is_some() || work_type == WorkType::Discussion,
            "Workflow 执行须来自冻结的步骤意图",
        )?;
        let provider: HarnessProvider = serde_json::from_value(command.payload["provider"].clone())
            .map_err(|_| err("invalid_configuration", "Agent 缺少 Harness 后端"))?;
        let installation =
            resources.ensure_runtime(provider, command.payload["installation_id"].as_str())?;
        if let Some(agent) = command.payload["agent_configuration"]["harness_revision_id"].as_str()
        {
            let h: HarnessDefinition = state.configuration(agent)?.decode()?;
            if let Some(recipe) = &h.recipe_digest {
                // Content match: the selected installation must originate from the exact
                // recipe the Agent's Harness revision pinned.
                require(
                    resources.installation_recipe(&installation) == *recipe,
                    "安装配方与 Agent 锁定的 Harness 配方不匹配",
                )?;
            }
        }
        resources.validate_installation(&installation)?;
        require(
            resources.runtime_verified(&installation.id)?,
            "运行时镜像内容或平台未通过核验",
        )?;
        let model = command.payload["model"]
            .as_str()
            .ok_or_else(|| err("invalid_configuration", "Agent 缺少 Model 修订"))?
            .to_owned();
        require(
            !model.trim().is_empty() && model.len() <= 160,
            "模型 ID 无效",
        )?;
        let policy = if let Some(configured) = &configured {
            if configured.access.writable_paths.is_empty() {
                ToolPolicy::ReadOnly
            } else if configured.access.writable_paths == ["."] {
                ToolPolicy::WorkspaceWrite
            } else {
                ToolPolicy::ScopedWrite
            }
        } else {
            ToolPolicy::ReadOnly
        };
        require(
            command.payload.get("writable_paths").is_none(),
            "写目录只能由正式工程入口解析",
        )?;
        let writable_paths: Vec<String> = if let Some(configured) = &configured {
            configured.access.writable_paths.clone()
        } else if policy == ToolPolicy::ReadOnly {
            vec![]
        } else {
            serde_json::from_value(command.payload["writable_paths"].clone())
                .map_err(|_| err("invalid_params", "缺少冻结写目录"))?
        };
        let read_only_paths = if let Some(configured) = &configured {
            configured.access.read_only_paths.clone()
        } else {
            vec![]
        };
        require(
            command.payload.get("permission_overrides").is_none(),
            "旧权限覆盖已移除",
        )?;
        let composition = state.resolve_agent(agent)?;
        let selection = permission_selection(state)?;
        let lock = if let Some(configured) = &configured {
            configured.permissions.clone()
        } else {
            resolve_permission_lock(state, &composition.harness_ref, &selection)?
        };
        resources.verify_permissions(state, &lock, &installation)?;
        let resolved_permissions = resources.compile_permissions(
            state,
            &lock,
            Some(ExecutionScope {
                work_type,
                writable_paths: writable_paths.clone(),
                read_only_paths: read_only_paths.clone(),
            }),
        )?;
        let writable_paths = resolved_permissions.scope.writable_paths.clone();
        let read_only_paths = resolved_permissions.scope.read_only_paths.clone();
        let policy = if writable_paths.is_empty() {
            ToolPolicy::ReadOnly
        } else if writable_paths == ["."] {
            ToolPolicy::WorkspaceWrite
        } else {
            ToolPolicy::ScopedWrite
        };
        let adapter = resources.adapter_identity(provider);
        let agent_configuration: Option<AgentExecutionConfiguration> = command
            .payload
            .get("agent_configuration")
            .map(|v| {
                serde_json::from_value(v.clone())
                    .map_err(|_| err("invalid_params", "无效 Agent 执行配置"))
            })
            .transpose()?;
        let model_connection = if provider.traits().host_model_route {
            let a = agent_configuration
                .as_ref()
                .ok_or_else(|| err("invalid_configuration", "缺少 Agent 执行配置"))?;
            let m: ModelDefinition = state.configuration(&a.model_revision_id)?.decode()?;
            let h = state.configuration(&a.harness_revision_id)?;
            require(m.model == model, "Model 修订与执行模型不一致")?;
            let connection = state.resolve_model_connection(&m, h)?;
            require(
                resolved_permissions.sandbox.policy.network != NetworkPermission::Disabled,
                "Sandbox 禁网，无法连接模型服务",
            )?;
            connection
        } else {
            None
        };
        let credential_ref =
            resources.credential_reference(provider, model_connection.as_ref(), state)?;
        let model_profile_digest = model_connection
            .as_ref()
            .map(|c| resources.model_profile_digest(provider, c, &model))
            .transpose()?;
        let mut binding = RuntimeBinding {
            model_connection,
            model_profile_digest,
            agent_configuration,
            permissions: Some(resolved_permissions),
            installation: installation.clone(),
            adapter_version: adapter,
            model,
            model_parameters: Default::default(),
            tool_policy: policy,
            writable_paths,
            read_only_paths,
            work_type,
            configuration_digest: String::new(),
            credential_ref,
        };
        binding.configuration_digest = resources.runtime_digest(definition, &binding)?;
        command.payload["binding"] = json!(binding);
        command.payload["prepared_catalog_revision"] = json!(catalog.revision);
        if let Some(formal) = formal.as_ref() {
            e.admit_formal_session(&command, &binding, formal)
        } else {
            e.execute_resolved("session.open", command, &fingerprint)
        }
    }
}
