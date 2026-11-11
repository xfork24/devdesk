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
            let pid = if params["scope"] == "global" {
                GLOBAL_DISCUSSION_NAMESPACE.into()
            } else {
                e.store
                    .project_id()
                    .ok_or_else(|| err("project_required", "需要当前项目"))?
            };
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
            snap.project_id = (pid != GLOBAL_DISCUSSION_NAMESPACE).then(|| pid.clone());
            snap.feature_id = None;
            snap.task_id = None;
            snap.documents.clear();
            snap.inherited_from = e
                .store
                .snapshot()?
                .snapshots
                .iter()
                .any(|s| s.id == original)
                .then_some(original.clone());
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
                        scope: if pid == GLOBAL_DISCUSSION_NAMESPACE {
                            Scope::Global
                        } else {
                            Scope::Project {
                                project_id: pid.clone(),
                            }
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

    pub(super) fn tick_workflow_engine(
        &self,
        e: &Engine,
        should_stop: &dyn Fn() -> bool,
    ) -> Result<()> {
        if should_stop() {
            return Ok(());
        }
        e.evaluate_workflows()?;
        for intent in e.pending_workflow_intents()? {
            if should_stop() {
                return Ok(());
            }
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
                .and_then(|r| r.data["owner_id"].as_str())
                .or_else(|| {
                    state
                        .governance
                        .iter()
                        .find(|r| r.kind == "workflow_controller" && r.id == intent.definition_id)
                        .and_then(|r| r.data["owner_id"].as_str())
                });
            if owner.is_none()
                || (intent.data["controller"] != true
                    && owner.is_some_and(|owner| state.workflow_paused(owner)))
            {
                continue;
            }
            if should_stop() {
                return Ok(());
            }
            let result = match method {
                "workflow.tool.execute" => self.execute_configured_tool(e, command),
                "session.open" => self.open_conversation(e, command),
                _ => e.execute(method, command),
            };
            // Persist the dispatched result even if shutdown was requested during the call.
            e.observe_workflow_intent(&intent.id, &result)?;
        }
        if should_stop() {
            return Ok(());
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
            require(r.kind == "agent_instance", "无效 Agent")?;
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
            .find(|r| r.kind == "agent" && r.id == agent_id)
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
        let feature = state
            .tasks
            .iter()
            .find(|task| task.id == owner)
            .map_or(owner, |task| task.feature_id.as_str());
        let configured = e.configured_agent_request(command)?;
        let frozen = configured.is_some();
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
            require(
                !retired || frozen,
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
                "Agent 执行不能单独覆盖 Role/Model/Harness/Permission，请采用新的 Agent 版本",
            )?;
        }
        let project = state.projects.first();
        require(
            project.is_some() || owner == "global",
            "Agent 执行需要项目或全局讨论范围",
        )?;
        let scope = project.map_or(Scope::Global, |p| Scope::Project {
            project_id: p.id.clone(),
        });
        let title = project.map_or("全局 / 跨项目", |p| p.title.as_str());
        let instance = if let Some(instance) = selected {
            let r = state.configuration(&instance)?;
            let i: AgentInstance = r.decode()?;
            require(i.scope.visible_from(&scope), "Agent 不属于当前项目")?;
            instance
        } else if let Some(r) = state.governance.iter().find(|r| {
            r.kind == "agent_instance"
                && r.data["agent_revision_id"] == agent_id
                && r.scope.visible_from(&scope)
        }) {
            r.id.clone()
        } else {
            e.execute("agent.instance.create",Command{command_id:format!("{}:agent-instance",command.command_id),target:None,expected_revision:None,payload:json!({"name":format!("{} · {}",record.name,title),"scope":scope,"agent_revision_id":agent_id,"frozen_feature_id":if frozen && !command.payload["controller_id"].is_string(){Some(feature)}else{None},"frozen_controller_id":command.payload["controller_id"]})})?["id"].as_str().unwrap().into()
        };
        resolve_model_connection(&model, &harness)?;
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
        let integration_lock = match configured {
            Some(configured) => configured.integration_lock,
            None => {
                let mut lock = devdesk_core::freeze_integrations(
                    &state,
                    &agent,
                    state.projects.first().map(|p| p.id.as_str()),
                )?;
                // Managed platform operations require a frozen Workflow caller and scope.
                lock.platform_tools.clear();
                lock
            }
        };
        command.payload["agent_configuration"] = json!(AgentExecutionConfiguration {
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

    pub(super) fn governance_dispatch(
        &self,
        method: &str,
        params: &Value,
    ) -> Option<Result<Value>> {
        let command = method == "standard.capture"
            || method == "standard.archive"
            || method == "work.configuration.update"
            || method == "configuration.retire"
            || (method.starts_with("workflow.")
                && ![
                    "workflow.inspect",
                    "workflow.change.preview",
                    "workflow.execution.inspect",
                    "workflow.outputs.read",
                    "workflow.events.read",
                ]
                .contains(&method))
            || method.starts_with("model.")
            || method.starts_with("tool.")
            || method.starts_with("harness.")
            || method.starts_with("sandbox.")
            || method.starts_with("role.")
            || method.starts_with("capability.")
            || method.starts_with("delivery_type.")
            || method.starts_with("agent.instance.")
            || (["skill.create", "skill.revise", "spec.create", "spec.revise"].contains(&method)
                && params["payload"].get("data").is_some())
            || (["agent.create", "agent.revise"].contains(&method)
                && params["payload"].get("data").is_some());
        let query = [
            "workflow.inspect",
            "workflow.change.preview",
            "workflow.execution.inspect",
            "workflow.outputs.read",
            "workflow.events.read",
            "standard.capture.preview",
            "query.standard",
            "query.standard_diff",
            "query.standard_usage",
            "query.work_standard_diff",
            "query.work_standard_choices",
            "query.work_configuration",
            "query.standard_catalog",
            "query.workflow_task_preview",
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
            if [
                "query.work_standard_diff",
                "query.work_standard_choices",
                "query.standard_diff",
            ]
            .contains(&method)
            {
                let mut states = Vec::new();
                let mut offset = 0;
                loop {
                    let (projects, total) = self.global.projects(offset, 100)?;
                    for project in &projects {
                        let state = SqliteStore::read_project_snapshot(
                            Path::new(&self.global.root()),
                            &project.id,
                        )?;
                        states.push((project.clone(), state));
                    }
                    offset += projects.len();
                    if offset >= total || projects.is_empty() {
                        break;
                    }
                }
                if method == "query.work_standard_choices" {
                    let records = states
                        .iter()
                        .flat_map(|(project, state)| {
                            work_standard_choices(state).into_iter().map(|mut value| {
                                value["project_title"] = json!(project.title);
                                value
                            })
                        })
                        .collect::<Vec<_>>();
                    let offset = params["offset"].as_u64().unwrap_or(0) as usize;
                    let limit = params["limit"].as_u64().unwrap_or(100).clamp(1, 100) as usize;
                    return Ok(
                        json!({"total":records.len(),"offset":offset,"records":records.into_iter().skip(offset).take(limit).collect::<Vec<_>>()}),
                    );
                }
                if method == "query.work_standard_diff" {
                    let find = |key: &str, project_key: &str| -> Result<&EngineeringState> {
                        let owner = params[key]
                            .as_str()
                            .ok_or_else(|| err("invalid_params", format!("缺少 {key}")))?;
                        states
                            .iter()
                            .find(|(project, state)| {
                                params[project_key]
                                    .as_str()
                                    .is_none_or(|id| project.id == id)
                                    && work_reference(state, &owner).is_ok()
                            })
                            .map(|(_, state)| state)
                            .ok_or_else(|| err("not_found", "工作不在可访问的项目中"))
                    };
                    return compare_work_standards(
                        find("before_owner_id", "before_project_id")?,
                        find("after_owner_id", "after_project_id")?,
                        params,
                    );
                }
                let resolve =
                    |key: &str, project_key: &str, entry_key: &str| -> Result<StandardSnapshot> {
                        let reference = params[key]
                            .as_str()
                            .ok_or_else(|| err("invalid_params", format!("缺少 {key}")))?;
                        let (state, scope) = if let Some((project, state)) =
                            states.iter().find(|(project, state)| {
                                params[project_key].as_str() == Some(project.id.as_str())
                                    && state.configuration(&reference).is_ok()
                            }) {
                            (
                                state,
                                Scope::Project {
                                    project_id: project.id.clone(),
                                },
                            )
                        } else if params[project_key].is_string() {
                            return Err(err("not_found", "规范不在指定项目中"));
                        } else if global.configuration(&reference).is_ok() {
                            (&global, Scope::Global)
                        } else {
                            let (project, state) = states
                                .iter()
                                .find(|(_, state)| state.configuration(&reference).is_ok())
                                .ok_or_else(|| err("not_found", "规范不在可访问的项目中"))?;
                            (
                                state,
                                Scope::Project {
                                    project_id: project.id.clone(),
                                },
                            )
                        };
                        let snapshot = standard_snapshot(state, &reference, &scope)?;
                        if let Some(entry) = params[entry_key].as_str() {
                            standard_entry_snapshot(&snapshot, entry)
                        } else {
                            Ok(snapshot)
                        }
                    };
                return Ok(diff_standard_snapshots(
                    &resolve("before_ref", "before_project_id", "before_workflow_ref")?,
                    &resolve("after_ref", "after_project_id", "after_workflow_ref")?,
                ));
            }

            if method == "query.standard_usage" && params["project_id"].is_null() && target_global {
                let reference = target.unwrap();
                standard_snapshot(&global, reference, &Scope::Global)?;
                let mut records = Vec::new();
                let mut offset = 0;
                loop {
                    let (projects, total) = self.global.projects(offset, 100)?;
                    for project in &projects {
                        let state = SqliteStore::read_project_snapshot(
                            Path::new(&self.global.root()),
                            &project.id,
                        )?;
                        records.extend(
                            state
                                .governance
                                .iter()
                                .filter(|r| {
                                    [
                                        "project_configuration",
                                        "work_configuration",
                                        "workflow_plan_revision",
                                    ]
                                    .contains(&r.kind.as_str())
                                        && r.data["standard_ref"] == reference
                                })
                                .map(|r| json!(r)),
                        );
                    }
                    offset += projects.len();
                    if offset >= total || projects.is_empty() {
                        break;
                    }
                }
                let offset = params["offset"].as_u64().unwrap_or(0) as usize;
                let limit = params["limit"].as_u64().unwrap_or(100).clamp(1, 100) as usize;
                return Ok(
                    json!({"total":records.len(),"offset":offset,"records":records.into_iter().skip(offset).take(limit).collect::<Vec<_>>()}),
                );
            }
            let explicit_scope = params
                .get("scope")
                .or_else(|| params["payload"].get("scope"));
            let local = !method.starts_with("role.")
                && ((params["project_id"].is_string()
                    && (method.starts_with("query.workflow")
                        || method == "query.specification_audit"
                        || method.starts_with("query.standard")
                        || method.starts_with("query.work_")
                        || method == "standard.capture.preview"))
                    || explicit_scope.is_some_and(|s| s["kind"] == "project")
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
                        if params["include_archived"] != true
                            && !template_record_current(&directory, r)
                        {
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
                                    && (params["include_archived"] == true
                                        || template_record_current(&directory, n))
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
                        json!({"records":records.iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":records.len()}),
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

#[cfg(test)]
mod standard_comparison_tests {
    use super::*;
    use crate::workspace::tests::{create, select};
    #[test]
    fn comparisons_read_each_project_and_work_without_switching_the_active_project() {
        let root = tempfile::tempdir().unwrap();
        let w =
            Workspace::open_with_adapters(root.path(), crate::test_runtime::adapters()).unwrap();
        let draft=w.dispatch("workflow.create",json!({"command_id":id(),"payload":{"name":"Shared contract","document":{"schema_version":6,"stages":[{"key":"work","name":"Work"}],"steps":[{"key":"one","name":"Same normative step","stage":"work","executor":{"type":"control"}}]}}})).unwrap();
        let workflow=w.dispatch("workflow.publish",json!({"command_id":id(),"target":draft["id"],"expected_revision":draft["revision"],"payload":{}})).unwrap();
        let mut projects = Vec::new();
        let mut features = Vec::new();
        let mut standards = Vec::new();
        for title in ["Compare A", "Compare B"] {
            let project = create(&w, title);
            let current = w.global.project(&project).unwrap().unwrap();
            let configuration=w.dispatch("project.configuration.update",json!({"command_id":id(),"project_id":project,"target":project,"expected_revision":current.revision,"payload":{"workflow_ref":workflow["id"],"role_agent_bindings":{}}})).unwrap();
            let feature=w.dispatch("feature.create",json!({"command_id":id(),"project_id":project,"payload":{"project_id":project,"title":"Shared work"}})).unwrap();
            projects.push(project);
            features.push(feature["id"].as_str().unwrap().to_owned());
            standards.push(configuration["data"]["standard_ref"].clone());
        }
        select(&w, &projects[1]);
        assert_ne!(standards[0], standards[1]);
        let snapshots = w
            .dispatch(
                "query.standard_diff",
                json!({"before_ref":standards[0],"after_ref":standards[1]}),
            )
            .unwrap();
        assert_eq!(snapshots["equal"], true);
        let configured = w
            .dispatch(
                "query.work_standard_diff",
                json!({"before_owner_id":projects[0],"after_owner_id":projects[1]}),
            )
            .unwrap();
        assert_eq!(configured["before"]["basis"], "configured");
        assert_eq!(configured["diff"]["equal"], true);
        let executed = w
            .dispatch(
                "query.work_standard_diff",
                json!({"before_owner_id":features[0],"after_owner_id":features[1]}),
            )
            .unwrap();
        assert_eq!(executed["before"]["basis"], "execution");
        assert_eq!(executed["entry_diff"]["equal"], true);
        assert_eq!(w.active_id(), Some(projects[1].clone()));
        let choices = w
            .dispatch("query.work_standard_choices", json!({"limit":100}))
            .unwrap();
        assert_eq!(choices["total"], 4);
        assert!(w.dispatch("query.work_standard_diff",json!({"before_owner_id":features[0],"after_owner_id":features[1],"before_plan_revision_id":executed["after"]["plan_revision_id"]})).is_err());
    }
}
