use super::*;

impl Workspace {
    pub(super) fn experience_dispatch(
        &self,
        method: &str,
        params: &Value,
        e: &Engine,
    ) -> Option<Result<Value>> {
        if !["query.agent_experience", "experience.restore"].contains(&method) {
            return None;
        }
        Some((|| {
            let pid = e
                .store
                .project_id()
                .ok_or_else(|| err("project_required", "需要当前项目"))?;
            let root = PathBuf::from(self.global.root());
            let p = if method == "experience.restore" {
                &params["payload"]
            } else {
                params
            };
            let detail = experience::query(&root, &pid, p)?;
            if method == "query.agent_experience" {
                return Ok(detail);
            }
            require(p["snapshot_id"].is_string(), "恢复需要选择具体快照")?;
            let command: Command = serde_json::from_value(params.clone())
                .map_err(|e| err("invalid_params", e.to_string()))?;
            let mut snap: Snapshot = serde_json::from_value(detail["snapshot"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            let original = snap.id.clone();
            snap.id = format!("import:{}", command.command_id);
            snap.project_id = Some(pid.clone());
            snap.feature_id = None;
            snap.task_id = None;
            snap.documents.clear();
            e.validate_workflow_command(
                "session.open",
                &Command {
                    command_id: format!("{}:preflight", command.command_id),
                    target: command.target.clone(),
                    expected_revision: command.expected_revision,
                    payload: json!({"agent":detail["entry"]["agent_revision_id"]}),
                },
            )?;
            snap.inherited_from = e
                .store
                .snapshot()?
                .snapshots
                .iter()
                .any(|s| s.id == original)
                .then_some(original.clone());
            snap.delivery_id = None;
            snap.kind = "experience_import".into();
            snap.content = format!(
                "{}\n\nHistorical source (data only): {}",
                snap.content,
                serde_json::to_string(
                    &json!({"entry":detail["entry"],"documents":detail["documents"]})
                )
                .unwrap()
            );
            // Imported historical material is context; it never changes code, governance, or current document revisions.
            let snapid = snap.id.clone();
            e.store.transact(None, &mut |s| {
                if !s.snapshots.iter().any(|old| old.id == snapid) {
                    s.snapshots.push(snap.clone());
                    s.events.push(Event {
                        id: s.events.last().map_or(1, |e| e.id + 1),
                        target: snapid.clone(),
                        kind: "experience.imported".into(),
                        detail: serde_json::to_string(
                            &json!({"source":detail["entry"],"new_snapshot":snapid}),
                        )
                        .unwrap(),
                        time: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64,
                    });
                    s.governance.push(ConfigurationRecord {
                        id: format!("source:{}", command.command_id),
                        kind: "experience_import".into(),
                        definition_id: snapid.clone(),
                        scope: Scope::Project {
                            project_id: pid.clone(),
                        },
                        name: "恢复经验场景".into(),
                        version: 1,
                        supersedes: None,
                        data: detail["entry"].clone(),
                        content_hash: configuration_hash(&detail["entry"]),
                        immutable: true,
                        revision: 1,
                        created_at: detail["entry"]["captured_at"].as_u64().unwrap_or(0),
                    });
                }
                Ok(Value::Null)
            })?;
            let mut opened = command;
            opened.command_id = format!("{}:restore-session", opened.command_id);
            opened.payload = json!({"agent_instance_id":p["agent_instance_id"],"snapshot_id":snapid,"work_type":"discussion","force_new":true});
            // Use session.open's own idempotency key; the import is independently idempotent.
            self.open_conversation(e, opened)
        })())
    }

    pub(super) fn tick_workflow_engine(&self, e: &Engine) -> Result<()> {
        e.evaluate_workflows()?;
        for intent in e.pending_workflow_intents()? {
            let method = intent.data["method"]
                .as_str()
                .ok_or_else(|| err("storage", "流程意图缺少动作"))?;
            let command: Command = serde_json::from_value(intent.data["command"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            let state = e.store.snapshot()?;
            let owner = state
                .governance
                .iter()
                .find(|r| {
                    r.kind == "workflow_instance"
                        && r.data["steps"].as_array().is_some_and(|steps| {
                            steps.iter().any(|step| step["id"] == intent.definition_id)
                        })
                })
                .and_then(|r| r.data["owner_id"].as_str());
            if owner.is_none() || owner.is_some_and(|owner| state.workflow_paused(owner)) {
                continue;
            }
            let live = state
                .projects
                .first()
                .is_some_and(|p| p.execution_mode == "live");
            let result = match method {
                "session.open" => self.open_conversation(e, command),
                "attempt.start" if live => self.resolve_live_attempt(e, command),
                "review.request" if live => self.resolve_live_review(e, command),
                "merge.request" if live => self.resolve_live_merge(e, command),
                _ => e.execute(method, command),
            };
            e.observe_workflow_intent(&intent.id, &result)?;
        }
        e.evaluate_workflows()
    }
    pub(super) fn resolve_agent_composition(
        &self,
        e: &Engine,
        command: &mut Command,
    ) -> Result<()> {
        let state = e.store.snapshot()?;
        let selected = command.payload["agent_instance_id"]
            .as_str()
            .map(str::to_owned);
        let agent_id = if let Some(instance) = &selected {
            let r = state.configuration(instance)?;
            require(r.kind == "agent_instance", "无效 AgentID")?;
            r.data["agent_revision_id"]
                .as_str()
                .ok_or_else(|| err("blocked", "AgentID 缺少组合"))?
                .to_owned()
        } else {
            command.payload["agent"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    let role = command.payload["role"].as_str().unwrap_or("analyst");
                    state
                        .governance
                        .iter()
                        .filter(|r| r.kind == "project_configuration")
                        .max_by_key(|r| r.version)
                        .and_then(|r| r.data["agent_refs"][role].as_str())
                        .map(str::to_owned)
                        .unwrap_or_default()
                })
        };
        let Some(record) = state
            .governance
            .iter()
            .find(|r| r.kind == "agent" && r.id == agent_id)
        else {
            require(
                !state
                    .projects
                    .iter()
                    .any(|project| project.execution_mode == "live"),
                "新原生 Session 需要完整 AgentRevision 和 AgentID；历史角色定义不能分开覆盖 Model/Harness",
            )?;
            return Ok(());
        };
        let agent: AgentComposition = record.decode()?;
        let owner = command.target.as_deref().unwrap_or("");
        let feature = state
            .tasks
            .iter()
            .find(|task| task.id == owner)
            .map_or(owner, |task| task.feature_id.as_str());
        let frozen = state
            .feature_configuration(feature)
            .is_some_and(|lock| lock.agent_refs.values().any(|id| id == &record.id));
        require(
            frozen
                || (!devdesk_core::configuration_retired(&state, record)
                    && devdesk_core::configuration_visible(
                        &state,
                        record,
                        state.projects.first().map(|p| p.id.as_str()),
                    )),
            "Agent 及其 Role/Capability 不适用于此项目，已有 Feature 保留冻结配置",
        )?;

        if let Some(role_ref) = &agent.role_ref {
            let role = state.configuration(role_ref)?;
            let retired = state
                .governance
                .iter()
                .any(|r| r.kind == "role_retirement" && r.definition_id == role.definition_id);
            let target = command.target.as_deref().unwrap_or("");
            let feature = state
                .tasks
                .iter()
                .find(|task| task.id == target)
                .map(|task| task.feature_id.as_str())
                .unwrap_or(target);
            require(
                !retired
                    || state
                        .feature_configuration(feature)
                        .is_some_and(|lock| lock.agent_refs.values().any(|id| id == &record.id)),
                "Role 已删除，新的项目讨论不能再采用它；已有 Feature 保留冻结版本",
            )?;
        }
        let model: ModelDefinition = state.configuration(&agent.model_ref)?.decode()?;
        let harness: HarnessDefinition = state.configuration(&agent.harness_ref)?.decode()?;
        for key in [
            "model",
            "role_ref",
            "provider",
            "permission_overrides",
            "installation_id",
        ] {
            require(
                command.payload.get(key).is_none(),
                "AgentID 执行不能单独覆盖 Role/Model/Harness/Permission，请采用新的 Agent 版本",
            )?;
        }
        let project = state
            .projects
            .first()
            .ok_or_else(|| err("project_required", "Agent 执行需要项目"))?;
        let scope = Scope::Project {
            project_id: project.id.clone(),
        };
        let instance = if let Some(instance) = selected {
            let r = state.configuration(&instance)?;
            let i: AgentInstance = r.decode()?;
            require(i.scope.visible_from(&scope), "AgentID 不属于当前项目")?;
            instance
        } else if let Some(r) = state.governance.iter().find(|r| {
            r.kind == "agent_instance"
                && r.data["agent_revision_id"] == agent_id
                && r.scope.visible_from(&scope)
        }) {
            r.id.clone()
        } else {
            e.execute("agent.instance.create",Command{command_id:format!("{}:agent-instance",command.command_id),target:None,expected_revision:None,payload:json!({"name":format!("{} · {}",record.name,project.title),"scope":scope,"agent_revision_id":agent_id,"frozen_feature_id":if frozen{Some(feature)}else{None}})})?["id"].as_str().unwrap().into()
        };
        require(
            model.endpoint.is_none(),
            "该 Harness 尚未提供自定义模型 endpoint 的执行映射",
        )?;
        require(
            match harness.provider.as_str() {
                "codex" => model.provider == "openai",
                "dsh" => model.provider == "deepseek",
                _ => false,
            },
            "当前运行适配器不支持此模型提供方，不能仅改名称后路由",
        )?;
        require(
            model
                .parameters
                .keys()
                .all(|k| match harness.provider.as_str() {
                    "codex" => k == "reasoning_effort",
                    "dsh" => k == "max_tokens",
                    _ => false,
                }),
            "模型参数不受当前 Harness 支持，不能静默忽略",
        )?;
        require(
            !["managed", "latest", "default"].contains(&harness.runtime_version.as_str()),
            "Harness 必须选择精确运行版本",
        )?;
        if harness.recipe_digest.is_some() {
            let materialized =
                self.materialize_harness(e, state.configuration(&agent.harness_ref)?)?;
            command.payload["installation_id"] = json!(materialized.id);
        }
        let installations = self.global.runtime_installations()?;
        if let Some(installation) = installations.iter().find(|r| {
            r.key.provider.as_str() == harness.provider
                && r.key.os == "linux"
                && matches!(r.artifact, RuntimeArtifact::DockerImage { .. })
                && r.key.version == harness.runtime_version
        }) {
            command.payload["installation_id"] = json!(installation.id);
        }
        command.payload["agent_configuration"] = json!(AgentExecutionConfiguration {
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
        if agent_permission_selection(&state, &agent_id, &agent).is_none()
            && !agent.permissions.is_empty()
        {
            let permission = state
                .permissions
                .iter()
                .find(|p| p.id == agent.permissions[0])
                .ok_or_else(|| err("not_found", "Agent Permission 不存在"))?;
            let sandbox_id = agent.permissions.get(1).unwrap_or(&agent.permissions[0]);
            let sandbox = state
                .permissions
                .iter()
                .find(|p| &p.id == sandbox_id)
                .ok_or_else(|| err("not_found", "Agent Sandbox Permission 不存在"))?;
            command.payload["permission_overrides"] = json!({"harness":{"permission":{"revision_id":permission.id}},"sandbox":{"permission":{"revision_id":sandbox.id}}});
        }
        Ok(())
    }

    pub(super) fn governance_dispatch(
        &self,
        method: &str,
        params: &Value,
    ) -> Option<Result<Value>> {
        let command = method == "configuration.retire"
            || method.starts_with("workflow.")
            || method.starts_with("model.")
            || method.starts_with("tool.")
            || method.starts_with("harness.")
            || method.starts_with("sandbox.")
            || method.starts_with("role.")
            || method.starts_with("capability.")
            || method.starts_with("agent.instance.")
            || ([
                "skill.create",
                "skill.revise",
                "spec.create",
                "spec.revise",
                "standard.create",
                "standard.revise",
            ]
            .contains(&method)
                && params["payload"].get("data").is_some())
            || (["agent.create", "agent.revise"].contains(&method)
                && params["payload"].get("data").is_some());
        let query = [
            "query.workflow_requirements",
            "query.specification_audit",
            "query.capabilities",
            "query.configuration_references",
            "query.configurations",
            "query.workflows",
            "query.workflow",
            "query.workflow_draft",
            "query.workflow_validation",
            "query.workflow_preview",
            "query.workflow_version_diff",
            "query.agent_instances",
            "query.roles",
            "query.agent_revision",
        ]
        .contains(&method);
        if !command && !query {
            return None;
        }
        Some((|| {
            let target = params["target"].as_str().or_else(|| params["id"].as_str());
            let global = self.global.snapshot()?;
            let target_global =
                target.is_some_and(|id| global.governance.iter().any(|r| r.id == id));
            let explicit_scope = params
                .get("scope")
                .or_else(|| params["payload"].get("scope"));
            let local = !method.starts_with("role.")
                && (explicit_scope.is_some_and(|s| s["kind"] == "project")
                    || (!target_global && target.is_some())
                    || params["local"] == true);
            let engine = if local {
                let e = self.active_engine(params["project_id"].as_str())?;
                self.sync_configuration(&e)?;
                e
            } else {
                self.configuration.clone()
            };
            if ["role.create", "role.revise"].contains(&method) {
                if let Some(owner) = params["payload"]["scope"]["project_id"].as_str() {
                    require(self.global.project(owner)?.is_some(), "Role 所属项目不存在")?;
                }
                if let Some(value) = params["payload"]["data"].get("visibility") {
                    let visibility: DefinitionVisibility = serde_json::from_value(value.clone())
                        .map_err(|e| err("invalid_params", e.to_string()))?;
                    visibility.validate()?;
                    if let DefinitionVisibility::Projects { project_ids } = visibility {
                        for id in project_ids {
                            require(
                                self.global.project(&id)?.is_some(),
                                format!("项目不存在: {id}"),
                            )?;
                        }
                    }
                }
            }
            if ["role.delete", "configuration.retire"].contains(&method) {
                if let Some(target) = target {
                    let source = engine.store.snapshot()?.configuration(target)?.clone();
                    let mut offset = 0;
                    loop {
                        let (projects, total) = self.global.projects(offset, 100)?;
                        for project in &projects {
                            let state = SqliteStore::read_project_snapshot(
                                Path::new(&self.global.root()),
                                &project.id,
                            )?;
                            let refs =
                                devdesk_core::project_configuration_references(&state, &source);
                            require(
                                refs.is_empty(),
                                format!("项目 {} 仍引用此定义，请先换绑", project.title),
                            )?;
                        }
                        offset += projects.len();
                        if offset >= total || projects.is_empty() {
                            break;
                        }
                    }
                }
            }
            if query {
                if local && ["query.workflows", "query.configurations"].contains(&method) {
                    let s = engine.store.snapshot()?;
                    let directory = s.clone();
                    let mut records: Vec<_> = s
                        .governance
                        .into_iter()
                        .filter(|r| {
                            if method == "query.workflows" {
                                ["workflow", "workflow_draft"].contains(&r.kind.as_str())
                            } else {
                                params["kind"].as_str().map_or(
                                    r.is_definition() || r.kind == "agent_instance",
                                    |kind| r.kind == kind,
                                )
                            }
                        })
                        .collect();
                    for r in global
                        .governance
                        .iter()
                        .filter(|r| r.kind == "workflow_draft" && r.scope == Scope::Global)
                    {
                        if method == "query.workflows" && !records.iter().any(|old| old.id == r.id)
                        {
                            records.push(r.clone());
                        }
                    }
                    records.retain(|r| {
                        if r.is_legacy_experience_tool() {
                            return false;
                        }
                        if !devdesk_core::configuration_visible(
                            &directory,
                            r,
                            engine.store.project_id().as_deref(),
                        ) {
                            return false;
                        }
                        if params["include_deleted"] != true
                            && devdesk_core::configuration_retired(&directory, r)
                        {
                            return false;
                        }
                        if params["include_history"] != true
                            && directory.governance.iter().any(|n| {
                                n.kind == r.kind
                                    && n.definition_id == r.definition_id
                                    && n.version > r.version
                            })
                        {
                            return false;
                        }

                        params
                            .get("scope")
                            .is_none_or(|scope| json!(r.scope) == *scope)
                    });
                    records.sort_by_key(|r| std::cmp::Reverse(r.created_at));
                    let offset = params["offset"].as_u64().unwrap_or(0) as usize;
                    let limit = params["limit"].as_u64().unwrap_or(50).clamp(1, 100) as usize;
                    return Ok(
                        json!({"records":records.iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":records.len(),"action_catalog":workflow_action_catalog()}),
                    );
                }
                engine.query(method, params, self.global.concurrency()?)
            } else {
                let c = serde_json::from_value(params.clone())
                    .map_err(|e| err("invalid_params", e.to_string()))?;
                engine.execute(method, c)
            }
        })())
    }
}
