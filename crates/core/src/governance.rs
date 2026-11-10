//! Configuration commands, immutable publication and workflow runtime coordination.
use crate::*;
use std::collections::BTreeMap;

pub(crate) fn record(
    kind: &str,
    definition_id: Id,
    scope: Scope,
    name: String,
    version: u64,
    supersedes: Option<Id>,
    data: Value,
    immutable: bool,
) -> ConfigurationRecord {
    ConfigurationRecord {
        id: id(),
        kind: kind.into(),
        definition_id,
        scope,
        name,
        version,
        supersedes,
        content_hash: configuration_hash(&data),
        data,
        immutable,
        revision: 1,
        created_at: now(),
    }
}
pub(crate) fn update_record(r: &mut ConfigurationRecord, data: Value) {
    if r.data != data {
        r.content_hash = configuration_hash(&data);
        r.data = data;
        r.revision += 1;
    }
}
fn decode<T: serde::de::DeserializeOwned>(v: &Value) -> Result<T> {
    serde_json::from_value(v.clone()).map_err(|e| err("invalid_params", e.to_string()))
}
fn role_key(role: AgentRole) -> &'static str {
    match role {
        AgentRole::Analyst => "analyst",
        AgentRole::Architect => "architect",
        AgentRole::Developer => "developer",
        AgentRole::Tester => "tester",
        AgentRole::Reviewer => "reviewer",
    }
}

pub(crate) fn default_workflow(task: bool) -> WorkflowDocument {
    let stages = if task {
        vec![("implementation", "Task 执行"), ("delivery", "Task 交付")]
    } else {
        vec![
            ("planning", "Feature 规划"),
            ("development", "Task 并行开发"),
            ("delivery", "Feature 收尾与交付"),
        ]
    };
    let mut doc = WorkflowDocument {
        input_ports: vec![],
        output_ports: BTreeMap::new(),
        schema_version: 1,
        process_kind: if task { "task" } else { "feature" }.into(),
        stages: stages
            .into_iter()
            .map(|(key, name)| WorkflowStage {
                description: String::new(),
                key: key.into(),
                name: name.into(),
            })
            .collect(),
        binding_slots: BTreeMap::new(),
        steps: vec![],
        subworkflow_refs: BTreeMap::new(),
    };
    for role in [
        AgentRole::Analyst,
        AgentRole::Architect,
        AgentRole::Developer,
        AgentRole::Tester,
        AgentRole::Reviewer,
    ] {
        doc.binding_slots.insert(
            role_key(role).into(),
            WorkflowSlot {
                role,
                agent_ref: None,
                capabilities: vec![],
            },
        );
    }
    let specs: Vec<(&str, &str, &str, &str, Option<&str>, &str, bool)> = if task {
        vec![
            (
                "implementation",
                "TDD 与实现",
                "implementation",
                "attempt.execute",
                Some("developer"),
                "implementation",
                false,
            ),
            (
                "review",
                "Task 交付评审",
                "delivery",
                "delivery.review",
                Some("reviewer"),
                "implementation",
                false,
            ),
            (
                "merge_prepare",
                "准备 Task 合入候选",
                "delivery",
                "merge.prepare",
                Some("tester"),
                "",
                false,
            ),
            (
                "merge_review",
                "Task 候选评审",
                "delivery",
                "merge.review",
                Some("reviewer"),
                "",
                false,
            ),
            (
                "merge_apply",
                "应用 Task 合入",
                "delivery",
                "merge.apply",
                None,
                "",
                false,
            ),
            (
                "snapshot",
                "Task 最终快照",
                "delivery",
                "snapshot.capture",
                None,
                "",
                false,
            ),
        ]
    } else {
        vec![
            (
                "initial_snapshot",
                "建立初始上下文快照",
                "planning",
                "snapshot.capture",
                None,
                "",
                false,
            ),
            (
                "requirement_discussion",
                "需求头脑风暴",
                "planning",
                "agent.interact",
                Some("analyst"),
                "requirement",
                false,
            ),
            (
                "requirement_adopt",
                "采纳需求",
                "planning",
                "proposal.adopt",
                Some("analyst"),
                "requirement",
                false,
            ),
            (
                "requirement_execute",
                "需求正式交付",
                "planning",
                "attempt.execute",
                Some("analyst"),
                "requirement_definition",
                false,
            ),
            (
                "requirement_review",
                "需求评审",
                "planning",
                "delivery.review",
                Some("reviewer"),
                "requirement",
                false,
            ),
            (
                "design_discussion",
                "讨论方案设计",
                "planning",
                "agent.interact",
                Some("architect"),
                "design",
                false,
            ),
            (
                "design_adopt",
                "采纳设计",
                "planning",
                "proposal.adopt",
                Some("architect"),
                "design",
                false,
            ),
            (
                "task_discussion",
                "讨论 Task 与 Test 计划",
                "planning",
                "agent.interact",
                Some("architect"),
                "task_plan",
                false,
            ),
            (
                "task_adopt",
                "采纳 Task 与 Test 计划",
                "planning",
                "proposal.adopt",
                Some("architect"),
                "task_plan",
                false,
            ),
            (
                "test_discussion",
                "讨论 Feature 测试计划",
                "planning",
                "agent.interact",
                Some("tester"),
                "test_plan",
                false,
            ),
            (
                "test_adopt",
                "采纳 Feature 测试计划",
                "planning",
                "proposal.adopt",
                Some("tester"),
                "test_plan",
                false,
            ),
            (
                "solution_execute",
                "方案规划交付",
                "planning",
                "attempt.execute",
                Some("architect"),
                "solution_planning",
                false,
            ),
            (
                "solution_review",
                "方案规划评审",
                "planning",
                "delivery.review",
                Some("reviewer"),
                "plan",
                false,
            ),
            (
                "tasks",
                "并行 Task 子流程",
                "development",
                "tasks.foreach",
                None,
                "",
                false,
            ),
            (
                "closure",
                "Feature 完整测试与文档",
                "delivery",
                "attempt.execute",
                Some("tester"),
                "closure",
                false,
            ),
            (
                "closure_review",
                "Feature 交付评审",
                "delivery",
                "delivery.review",
                Some("reviewer"),
                "closure",
                false,
            ),
            (
                "merge_prepare",
                "准备主干候选",
                "delivery",
                "merge.prepare",
                Some("tester"),
                "",
                false,
            ),
            (
                "merge_review",
                "主干候选评审并确认",
                "delivery",
                "merge.review",
                Some("reviewer"),
                "",
                true,
            ),
            (
                "merge_apply",
                "应用主干合入",
                "delivery",
                "merge.apply",
                None,
                "",
                false,
            ),
            (
                "snapshot",
                "Feature 最终快照",
                "delivery",
                "snapshot.capture",
                None,
                "",
                false,
            ),
            (
                "cleanup",
                "资源处置",
                "delivery",
                "resource.cleanup",
                None,
                "",
                false,
            ),
        ]
    };
    let mut previous = None;
    for (key, name, stage, kind, slot, phase, manual) in specs {
        let mut params = BTreeMap::new();
        if !phase.is_empty() {
            params.insert("phase".into(), json!(phase));
        }
        let prompts = crate::default_tasks::legacy_node_prompts(phase);
        if prompts.as_object().is_some_and(|prompts| !prompts.is_empty()) {
            params.insert("agent_prompts".into(), prompts);
        }
        let recipe = if phase == "implementation" {
            vec![
                ("test_author", AgentRole::Tester, vec![], true),
                ("red", AgentRole::Tester, vec!["test_author"], false),
                ("implement", AgentRole::Developer, vec!["red"], true),
                ("green", AgentRole::Tester, vec!["implement"], false),
                ("docs", AgentRole::Developer, vec!["implement"], false),
                ("verify", AgentRole::Tester, vec!["green", "docs"], false),
            ]
            .into_iter()
            .map(|(key, role, deps, writes)| WorkflowRunNode {
                task_instructions: None,
                role_ref: None,
                key: key.into(),
                role,
                dependencies: deps.into_iter().map(str::to_owned).collect(),
                writes,
            })
            .collect()
        } else {
            vec![]
        };
        doc.steps.push(WorkflowStep {
            delivery_inputs: BTreeMap::new(),
            delivery_outputs: BTreeMap::new(),
            task_instructions: None,
            role_ref: None,
            spec_refs: vec![],
            subworkflow_ref: None,
            key: key.into(),
            name: name.into(),
            stage: stage.into(),
            kind: kind.into(),
            contract_version: 1,
            dependencies: previous.iter().cloned().collect(),
            participant_slot: slot.map(str::to_owned),
            inputs: BTreeMap::new(),
            interaction: InteractionContract {
                mode: if phase == "requirement" {
                    "brainstorm"
                } else {
                    "discussion"
                }
                .into(),
                goal: crate::default_tasks::legacy_goal(key, task).unwrap_or(name).into(),
                output_kind: phase.into(),
                max_turns: 30,
                ..Default::default()
            },
            advance_mode: if manual {
                AdvanceMode::HumanConfirmation
            } else {
                AdvanceMode::Automatic
            },
            condition: WorkflowCondition::Always,
            parameters: params,
            execution_recipe: recipe,
            rework_scope: vec![],
            retry_limit: 3,
        });
        previous = Some(key.to_owned());
    }
    if !task {
        doc.subworkflow_refs
            .insert("task".into(), "builtin-workflow-task-v1".into());
    }
    doc
}

pub(crate) fn seed_governance(s: &mut EngineeringState) {
    for (task, key, name) in [
        (true, "builtin-workflow-task-v1", "标准 Task 开发流程"),
        (
            false,
            "builtin-workflow-feature-v1",
            "标准 Feature 开发流程",
        ),
    ] {
        if !s.governance.iter().any(|r| r.id == key) {
            let mut r = record(
                "workflow",
                key.trim_end_matches("-v1").into(),
                Scope::Global,
                name.into(),
                1,
                None,
                json!(default_workflow(task)),
                true,
            );
            r.id = key.into();
            s.governance.push(r);
        }
    }
    crate::builtin_defaults::seed_defaults(s);
}

fn check_scope(s: &EngineeringState, scope: &Scope, store_project: Option<&str>) -> Result<()> {
    match scope {
        Scope::Global => require(store_project.is_none(), "全局定义必须由全局配置入口编辑"),
        Scope::Project { project_id } => require(
            store_project.is_none_or(|p| p == project_id)
                && s.projects.iter().any(|p| &p.id == project_id),
            "配置 Project 作用域不匹配",
        ),
        Scope::Feature { .. } => Err(err("invalid_scope", "不能直接创建 Feature 局部可变定义")),
    }
}
pub(crate) fn reference<'a>(
    s: &'a EngineeringState,
    id: &str,
    kind: &str,
    scope: &Scope,
) -> Result<&'a ConfigurationRecord> {
    let r = s.configuration(id)?;
    require(
        !crate::configuration::retired(s, r),
        "该定义已停用，不能新绑定；已有 Feature 保持原配置",
    )?;
    require(
        r.kind == kind && r.immutable && (r.kind == "role" || r.scope.visible_from(scope)),
        format!("{id}: 类型或 Scope 不兼容"),
    )?;
    require(
        crate::configuration::visible(s, r, scope.project()),
        "该定义不适用于此项目范围",
    )?;
    if kind == "agent" {
        let capability = r.data["capability_ref"].as_str().or_else(|| {
            s.governance
                .iter()
                .find(|link| link.kind == "agent_capability" && link.definition_id == r.id)
                .and_then(|link| link.data["capability_ref"].as_str())
        });
        if let Some(id) = capability {
            reference(s, id, "capability", scope)?;
        }
    }
    Ok(r)
}
pub(crate) fn validate_composition(
    s: &EngineeringState,
    a: &AgentComposition,
    scope: &Scope,
) -> Result<()> {
    require(!a.instructions.trim().is_empty(), "Agent 指令不能为空")?;
    let capability = a.capability_ref.as_deref();
    if let Some(id) = capability {
        let cap: CapabilityDefinition = reference(s, id, "capability", scope)?.decode()?;
        require(
            cap.model_ref == a.model_ref
                && cap.harness_ref == a.harness_ref
                && cap.tools == a.tools
                && cap.skills == a.skills,
            "Agent 执行字段与 Capability 修订不一致",
        )?;
    }

    if let Some(role_ref) = &a.role_ref {
        let role = reference(s, role_ref, "role", scope)?;
        let definition: RoleDefinition = role.decode()?;
        require(
            !crate::builtin_defaults::role_deleted(s, role),
            "Role 已删除，不能用于新绑定",
        )?;
        require(
            definition.execution_role == a.role,
            "Agent 的执行职责与 Role 修订不一致",
        )?;
    }
    require(
        !a.tools.is_empty()
            && (a.permission_selection.is_some() || (1..=2).contains(&a.permissions.len())),
        "Agent 必须明确 Tools 及 Harness/Sandbox Permission",
    )?;
    let model: ModelDefinition = reference(s, &a.model_ref, "model", scope)?.decode()?;
    let harness: HarnessDefinition = reference(s, &a.harness_ref, "harness", scope)?.decode()?;
    if let Some(selection) = &a.permission_selection {
        let ht = permission_target(s, PermissionTargetKind::Harness, &a.harness_ref)?;
        let st = permission_target(s, PermissionTargetKind::Sandbox, &selection.sandbox_ref)?;
        for (target, choice) in [(ht, &selection.harness), (st, &selection.sandbox)] {
            if matches!(choice, PermissionSelection::Pinned { .. }) {
                resolve_bound_permission(s, target, choice)?;
            }
        }
    }
    require(
        !model.model.trim().is_empty() && !model.provider.trim().is_empty(),
        "Model 后端与标识不能为空",
    )?;
    require(
        harness.model_providers.is_empty() || harness.model_providers.contains(&model.provider),
        "Harness 不支持该 Model 后端",
    )?;

    resolve_model_connection(&model, &harness)?;

    for tool in &a.tools {
        let record = reference(s, tool, "tool", scope)?;
        // Old immutable compositions remain usable; this reference no longer enables a Tool.
        if record.is_legacy_experience_tool() {
            continue;
        }
        if record.data["schema_version"] == 2 {
            let t: ExternalTool = record.decode()?;
            t.validate()?;
            reference(s, &t.integration_ref, "integration", scope)?;
            require(harness.provider == "codex", "此 Harness 尚不支持外部工具")?;
            continue;
        }
        let t: ToolDefinition = record.decode()?;
        require(
            t.implementation == "harness.native",
            "Tool 尚无可执行处理器",
        )?;
        require(
            t.harness_provider
                .as_ref()
                .is_none_or(|p| p == &harness.provider),
            "原生工具组不适用于所选 Harness",
        )?;
        require(
            t.capabilities
                .iter()
                .all(|c| harness.capabilities.contains(c)),
            "Harness 不支持 Tool 所需能力",
        )?;
    }
    require(
        a.tools
            .iter()
            .filter_map(|id| s.configuration(id).ok())
            .filter_map(|r| r.decode::<ToolDefinition>().ok())
            .any(|t| t.implementation == "harness.native"),
        "当前 Harness 必须显式声明原生 Tool 集合",
    )?;
    for skill in &a.skills {
        require(
            s.definitions
                .iter()
                .any(|d| &d.id == skill && d.family == "skill")
                || s.governance
                    .iter()
                    .any(|r| &r.id == skill && r.kind == "skill" && r.scope.visible_from(scope)),
            "Skill 修订不存在",
        )?;
    }
    for permission in &a.permissions {
        require(
            s.permissions.iter().any(|p| &p.id == permission),
            "Permission 修订不存在",
        )?;
    }
    Ok(())
}
pub(crate) fn install_agent_profile(
    s: &mut EngineeringState,
    r: &ConfigurationRecord,
    a: &AgentComposition,
) {
    if s.definitions.iter().any(|d| d.id == r.id) {
        return;
    }
    let policy = match a.role {
        AgentRole::Developer => ToolPolicy::TaskWrite,
        AgentRole::Tester => ToolPolicy::TestWrite,
        _ => ToolPolicy::ReadOnly,
    };
    let mut work = vec![WorkType::Discussion];
    match a.role {
        AgentRole::Architect => work.push(WorkType::Design),
        AgentRole::Developer => work.push(WorkType::Implementation),
        AgentRole::Tester => work.push(WorkType::Testing),
        AgentRole::Reviewer => work.push(WorkType::Review),
        _ => {}
    }
    s.definitions.push(Definition {
        id: r.id.clone(),
        family: "agent".into(),
        name: r.name.clone(),
        version: r.version,
        supersedes: r.supersedes.clone(),
        body: a
            .role_ref
            .as_ref()
            .and_then(|id| s.configuration(id).ok())
            .and_then(|r| r.decode::<RoleDefinition>().ok())
            .map(|role| {
                if role.instructions == a.instructions {
                    a.instructions.clone()
                } else {
                    format!(
                        "{}\n\nAgent-specific instructions:\n{}",
                        role.instructions, a.instructions
                    )
                }
            })
            .unwrap_or_else(|| a.instructions.clone()),
        slots: vec![],
        rules: vec![],
        agent_profile: Some(AgentProfile {
            role: a.role,
            skill_revisions: a.skills.clone(),
            tool_policy: policy,
            allowed_work: work,
            output_schemas: match a.role {
                AgentRole::Analyst => vec!["requirement/v1".into(), "feature/v1".into()],
                AgentRole::Architect => vec!["design/v1".into(), "task_plan/v1".into()],
                AgentRole::Tester => vec!["test_plan/v1".into(), "test_result/v1".into()],
                AgentRole::Reviewer => vec!["review/v1".into()],
                AgentRole::Developer => vec!["implementation/v1".into()],
            },
            permission_overrides: None,
        }),
    });
}

impl Engine {
    pub(crate) fn freeze_legacy_features(&self, s: &mut EngineeringState) {
        let features: Vec<_> = s
            .features
            .iter()
            .filter(|f| {
                s.feature_configuration(&f.id).is_none()
                    && s.projects
                        .iter()
                        .any(|p| p.id == f.project_id && p.execution_mode == "live")
                    && !s
                        .governance
                        .iter()
                        .any(|r| r.kind == "legacy_feature_lock" && r.definition_id == f.id)
            })
            .cloned()
            .collect();
        for f in features {
            let sessions:Vec<_>=s.sessions.iter().filter(|session|session.feature_id.as_deref()==Some(&f.id)||session.owner_id==f.id||s.tasks.iter().any(|t|t.id==session.owner_id&&t.feature_id==f.id)).map(|session|json!({"session":session.id,"agent":session.agent,"runtime_binding":session.runtime_binding})).collect();
            s.governance.push(record("legacy_feature_lock",f.id.clone(),Scope::Feature{project_id:f.project_id,feature_id:f.id},"历史 Feature 配置".into(),1,None,json!({"binding":f.binding,"known_sessions":sessions,"agent_instance_id":null,"model":"unknown unless present in a recorded runtime_binding","policy":"Do not infer AgentID or route new work using latest preferences; create a successor Feature"}),true));
        }
    }

    pub(crate) fn index_experience(
        &self,
        s: &mut EngineeringState,
        snapshot_id: &str,
    ) -> Result<()> {
        let snap = s
            .snapshots
            .iter()
            .find(|snap| snap.id == snapshot_id)
            .ok_or_else(|| err("not_found", "Snapshot 不存在"))?
            .clone();
        let feature = snap
            .feature_id
            .as_ref()
            .and_then(|id| s.features.iter().find(|f| &f.id == id));
        let pid = feature
            .map(|f| f.project_id.clone())
            .or(snap.project_id.clone())
            .unwrap_or_else(|| GLOBAL_DISCUSSION_NAMESPACE.into());
        let project_title = if pid == GLOBAL_DISCUSSION_NAMESPACE {
            "全局 / 跨项目".to_owned()
        } else {
            s.projects
                .iter()
                .find(|p| p.id == pid)
                .ok_or_else(|| err("not_found", "Project 不存在"))?
                .title
                .clone()
        };
        let feature_name = feature.map(|f| f.title.clone());
        for session_id in &snap.capture.sessions {
            let link = s
                .governance
                .iter()
                .find(|r| r.kind == "agent_session" && r.definition_id == *session_id)
                .cloned();
            if let Some(link) = link {
                if !s.governance.iter().any(|r| {
                    r.kind == "experience"
                        && r.definition_id == snapshot_id
                        && r.data["session_id"] == *session_id
                }) {
                    let work = s
                        .turns
                        .iter()
                        .rev()
                        .find(|t| t.session_id == *session_id)
                        .and_then(|t| t.finished.or(t.started));
                    s.governance.push(record("experience",snapshot_id.into(),if pid==GLOBAL_DISCUSSION_NAMESPACE {Scope::Global}else{Scope::Project{project_id:pid.clone()}},feature_name.clone().unwrap_or(project_title.clone()),1,None,json!({"agent_instance_id":link.data["agent_instance_id"],"agent_revision_id":link.data["agent_revision_id"],"snapshot_id":snapshot_id,"session_id":session_id,"project_id":pid,"project_name":project_title,"feature_id":snap.feature_id,"feature_name":feature_name.clone().unwrap_or_default(),"task_id":snap.task_id,"captured_at":snap.capture.captured_at,"work_time":work,"binding":link.data["binding"],"kind":snap.kind}),true));
                }
            }
        }
        Ok(())
    }

    pub fn workflow_recipe(
        &self,
        s: &EngineeringState,
        owner: &str,
        phase: &str,
        step_instance: Option<&str>,
    ) -> Option<Vec<WorkflowRunNode>> {
        let task = s.tasks.iter().find(|t| t.id == owner);
        let feature = task.map_or(owner, |t| t.feature_id.as_str());
        let lock = s.feature_configuration(feature)?;
        let doc = if task.is_some() {
            &lock.task_plan
        } else {
            &lock.plan
        };
        let key = step_instance.and_then(|id| {
            s.governance
                .iter()
                .filter(|r| r.kind == "workflow_instance" && r.definition_id == owner)
                .filter_map(|r| r.decode::<WorkflowRuntime>().ok())
                .flat_map(|r| r.steps)
                .find(|step| step.id == id)
                .map(|s| s.key)
        });
        doc.steps
            .iter()
            .find(|step| {
                key.as_ref().is_none_or(|key| key == &step.key)
                    && step.kind == "attempt.execute"
                    && step.parameters.get("phase").and_then(Value::as_str) == Some(phase)
            })
            .filter(|step| !step.execution_recipe.is_empty())
            .map(|step| step.execution_recipe.clone())
    }
    pub(crate) fn governance_after_command(
        &self,
        method: &str,
        c: &Command,
        s: &mut EngineeringState,
        result: &Value,
    ) -> Result<()> {
        self.freeze_action_specifications(s, method, c, result)?;
        self.freeze_action_tasks(s, method, c, result)?;
        if method == "conversation.send" {
            self.freeze_turn_task(s, &text(result, "id")?)?;
        }
        if method == "session.open" {
            if let Some(instance_id) = opt(&c.payload, "agent_instance_id") {
                let instance_record = s.configuration(&instance_id)?.clone();
                require(instance_record.kind == "agent_instance", "无效 Agent")?;
                let instance: AgentInstance = instance_record.decode()?;
                let session_id = text(result, "id")?;
                let session = s
                    .sessions
                    .iter()
                    .find(|v| v.id == session_id)
                    .ok_or_else(|| err("not_found", "Session 不存在"))?;
                let project = session
                    .project_id
                    .clone()
                    .ok_or_else(|| err("invalid_scope", "Session 缺少项目"))?;
                require(
                    instance
                        .scope
                        .visible_from(&if session.scope == Some(SessionScope::Global) {
                            Scope::Global
                        } else {
                            Scope::Project {
                                project_id: project.clone(),
                            }
                        }),
                    "Agent 不能用于其他项目",
                )?;
                require(
                    session.agent == instance.agent_revision_id,
                    "Agent 与 AgentRevision 不匹配",
                )?;
                let agent: AgentComposition =
                    s.configuration(&instance.agent_revision_id)?.decode()?;
                let model: ModelDefinition = s.configuration(&agent.model_ref)?.decode()?;
                let harness: HarnessDefinition = s.configuration(&agent.harness_ref)?.decode()?;
                let binding = session
                    .runtime_binding
                    .as_ref()
                    .ok_or_else(|| err("blocked", "Agent Session 缺少冻结绑定"))?;
                require(
                    binding.model == model.model
                        && binding.installation.key.provider.as_str() == harness.provider,
                    "执行绑定改变了 Agent 的 Model/Harness",
                )?;
                let resolved = resolve_model_connection(&model, &harness)?;
                if binding.model_connection.is_some() {
                    require(
                        binding.model_connection == resolved,
                        "执行绑定改变了 Model 连接或容量",
                    )?;
                } else {
                    require(
                        ["openai", "deepseek"].contains(&model.provider.as_str()),
                        "新服务商缺少冻结连接",
                    )?;
                }
                if let Some(old) = s
                    .governance
                    .iter()
                    .find(|r| r.kind == "agent_session" && r.definition_id == session_id)
                {
                    require(
                        old.data["agent_instance_id"] == instance_id,
                        "不能将 Session 重新归属其他 Agent",
                    )?;
                } else {
                    s.governance.push(record("agent_session",session_id,if session.scope==Some(SessionScope::Global){Scope::Global}else{Scope::Project{project_id:project}},instance_record.name,1,None,json!({"agent_instance_id":instance_id,"agent_revision_id":instance.agent_revision_id,"composition_hash":s.configuration(&instance.agent_revision_id)?.content_hash,"model_ref":agent.model_ref,"harness_ref":agent.harness_ref,"binding":binding,"workflow_step_instance_id":c.payload["workflow_step_instance_id"],"workflow_recipe_node":c.payload["workflow_recipe_node"]}),true));
                }
            }
        }
        if method == "session.open" {
            if let (Some(step), Some(session)) = (
                c.payload["workflow_step_instance_id"].as_str(),
                result["id"].as_str(),
            ) {
                if !s.governance.iter().any(|r| {
                    r.kind == "agent_session_use"
                        && r.definition_id == session
                        && r.data["step_id"] == step
                }) {
                    let scope = s
                        .configuration(c.payload["agent_instance_id"].as_str().unwrap_or(""))
                        .ok()
                        .map(|r| r.scope.clone())
                        .unwrap_or(Scope::Global);
                    s.governance.push(record("agent_session_use",session.into(),scope,"步骤会话关联".into(),1,None,json!({"step_id":step,"agent_instance_id":c.payload["agent_instance_id"],"agent_revision_id":c.payload["agent"]}),true));
                }
            }
        }
        if ["document.revise", "proposal.adopt"].contains(&method) {
            let docid = if method == "document.revise" {
                result["id"].as_str()
            } else {
                result["objects"]
                    .as_array()
                    .and_then(|ids| ids.first())
                    .and_then(Value::as_str)
            };
            if let Some(doc) = docid
                .and_then(|id| s.documents.iter().find(|d| d.id == id))
                .cloned()
            {
                let feature = s
                    .features
                    .iter()
                    .find(|f| s.document_in_feature(&doc, &f.id))
                    .map(|f| f.id.clone());
                if let Some(feature) = feature {
                    let default_source = if doc.task_id.is_some() {
                        "implementation"
                    } else {
                        match doc.kind.as_str() {
                            "requirement" => "requirement_execute",
                            "design" => "task_discussion",
                            "test" => "solution_execute",
                            "doc" if doc.title == "Structured Task Plan" => "solution_execute",
                            _ => "",
                        }
                    };
                    let owner = doc.task_id.as_deref().unwrap_or(&feature);
                    let runtime = s
                        .governance
                        .iter()
                        .rev()
                        .find(|r| {
                            r.kind == "workflow_instance"
                                && r.definition_id == owner
                                && r.data["status"] != "cancelled"
                        })
                        .and_then(|r| r.decode::<WorkflowRuntime>().ok());
                    if let Some(runtime) = runtime {
                        let lock: FeatureConfiguration =
                            s.configuration(&runtime.configuration_lock_id)?.decode()?;
                        let plan = if runtime.task_id.is_some() {
                            &lock.task_plan
                        } else {
                            &lock.plan
                        };
                        let source = plan
                            .steps
                            .iter()
                            .find(|step| {
                                if doc.task_id.is_some() {
                                    step.kind == "attempt.execute"
                                } else {
                                    match doc.kind.as_str() {
                                        "requirement" => {
                                            step.kind == "attempt.execute"
                                                && step.parameters.get("phase")
                                                    == Some(&json!("requirement_definition"))
                                        }
                                        "design" => {
                                            step.kind == "agent.interact"
                                                && step.interaction.output_kind == "task_plan"
                                        }
                                        "test" => {
                                            step.kind == "attempt.execute"
                                                && step.parameters.get("phase")
                                                    == Some(&json!("solution_planning"))
                                        }
                                        "doc" if doc.title == "Structured Task Plan" => {
                                            step.kind == "attempt.execute"
                                                && step.parameters.get("phase")
                                                    == Some(&json!("solution_planning"))
                                        }
                                        _ => false,
                                    }
                                }
                            })
                            .map(|step| step.key.as_str())
                            .unwrap_or(default_source);
                        if let Some(step) = runtime
                            .steps
                            .iter()
                            .find(|step| step.key == source && step.status != "pending")
                        {
                            self.workflow_control(
                                "workflow.step.rework",
                                &Command {
                                    command_id: format!("{}:input-rework", c.command_id),
                                    target: Some(step.id.clone()),
                                    expected_revision: Some(step.revision),
                                    payload: json!({"from_step":source}),
                                },
                                s,
                            )?;
                        }
                    }
                }
            }
        }
        if method == "attempt.start" {
            if let Some(aid) = result["id"].as_str() {
                let attempt = s.attempt(aid)?.clone();
                if s.feature_configuration(&attempt.feature_id).is_some() {
                    s.governance.push(record("stage_input_lock",aid.into(),Scope::Feature{project_id:s.feature(&attempt.feature_id)?.project_id.clone(),feature_id:attempt.feature_id.clone()},attempt.phase.clone(),1,None,json!({"phase":attempt.phase,"documents":attempt.documents,"context":attempt.context_content,"binding":attempt.binding,"workflow_step_instance_id":c.payload["workflow_step_instance_id"]}),true));
                }
            }
        }
        if let Some(step_id) = opt(&c.payload, "workflow_step_instance_id") {
            let field = match method {
                "session.open" => Some("session"),
                "conversation.send" => Some("turn"),
                "proposal.extract" => Some("proposal"),
                "attempt.start" => Some("attempt"),
                "delivery.submit" => Some("delivery"),
                "review.request" => Some("review_run"),
                "merge.request" | "merge.apply" => Some("merge"),
                "snapshot.save" => Some("snapshot"),
                _ => None,
            };
            if let (Some(field), Some(id)) = (field, result["id"].as_str()) {
                if let Some(r) = s.governance.iter_mut().find(|r| {
                    r.kind == "workflow_instance"
                        && r.data["steps"]
                            .as_array()
                            .is_some_and(|steps| steps.iter().any(|x| x["id"] == step_id))
                }) {
                    let mut runtime: WorkflowRuntime = r.decode()?;
                    if let Some(step) = runtime.steps.iter_mut().find(|x| x.id == step_id) {
                        step.outputs.insert(field.into(), id.into());
                        step.revision += 1;
                        runtime.dirty = true;
                    }
                    update_record(r, json!(runtime));
                }
            }
        }
        for r in s
            .governance
            .iter_mut()
            .filter(|r| r.kind == "workflow_instance")
        {
            if method != "workflow.pause" && method != "workflow.resume" {
                r.data["dirty"] = json!(true);
                r.content_hash = configuration_hash(&r.data);
            }
        }
        Ok(())
    }
    pub(crate) fn freeze_feature_configuration(
        &self,
        s: &mut EngineeringState,
        feature: &str,
        p: &Value,
    ) -> Result<()> {
        let f = s.feature(feature)?.clone();
        let current = s
            .governance
            .iter()
            .filter(|r| r.kind == "project_configuration" && r.definition_id == f.project_id)
            .max_by_key(|r| r.version)
            .cloned();
        let Some(current) = current else {
            require(
                p["require_configuration"] != true
                    && !s
                        .projects
                        .iter()
                        .any(|p| p.id == f.project_id && p.execution_mode == "live"),
                "请先为 Project 选择 Workflow、Standard 和 Agent 组合",
            )?;
            return Ok(());
        };
        if let Some(expected) = opt(p, "configuration_revision_id") {
            require(
                expected == current.id,
                "Project 默认配置已变化，请刷新创建表单",
            )?;
        }
        if current.data["schema_version"] == 2 {
            return self.freeze_configuration_v2(s, feature, &current);
        }
        let wf = reference(
            s,
            &text(&current.data, "workflow_ref")?,
            "workflow",
            &Scope::Project {
                project_id: f.project_id.clone(),
            },
        )?
        .clone();
        let mut plan: WorkflowDocument = wf.decode()?;
        validate_workflow(&plan)?;
        let scope = Scope::Feature {
            project_id: f.project_id.clone(),
            feature_id: feature.into(),
        };
        let mut agents: BTreeMap<String, String> = decode(&current.data["agent_refs"])?;
        let mut hashes = BTreeMap::new();
        hashes.insert(wf.id.clone(), wf.content_hash.clone());
        for (key, slot) in &mut plan.binding_slots {
            let ar = agents
                .get(key)
                .cloned()
                .or_else(|| slot.agent_ref.clone())
                .ok_or_else(|| err("configuration_required", format!("未绑定 Agent: {key}")))?;
            let r = reference(s, &ar, "agent", &scope)?;
            let a: AgentComposition = r.decode()?;
            validate_composition(s, &a, &scope)?;
            require(a.role == slot.role, "Agent 职责与步骤不符")?;
            let harness: HarnessDefinition = s.configuration(&a.harness_ref)?.decode()?;
            let model: ModelDefinition = s.configuration(&a.model_ref)?.decode()?;
            require(
                slot.capabilities
                    .iter()
                    .all(|c| harness.capabilities.contains(c) || model.capabilities.contains(c)),
                format!("角色 {key} 的完整 Agent 不满足流程能力要求"),
            )?;
            for reference in std::iter::once(&ar)
                .chain(std::iter::once(&a.model_ref))
                .chain(std::iter::once(&a.harness_ref))
                .chain(a.role_ref.iter())
                .chain(a.tools.iter())
            {
                let r = s.configuration(reference)?;
                hashes.insert(r.id.clone(), r.content_hash.clone());
            }
            for reference in a.skills.iter().chain(a.permissions.iter()) {
                let value = s
                    .definitions
                    .iter()
                    .find(|d| &d.id == reference)
                    .map(|d| json!(d))
                    .or_else(|| {
                        s.permissions
                            .iter()
                            .find(|d| &d.id == reference)
                            .map(|d| json!(d))
                    })
                    .or_else(|| {
                        s.governance
                            .iter()
                            .find(|d| &d.id == reference)
                            .map(|d| json!(d))
                    })
                    .ok_or_else(|| err("not_found", "Agent 依赖不存在"))?;
                hashes.insert(reference.clone(), configuration_hash(&value));
            }
            slot.agent_ref = Some(ar.clone());
            agents.insert(key.clone(), ar);
        }
        let task_ref = plan
            .subworkflow_refs
            .get("task")
            .ok_or_else(|| err("invalid_workflow", "Feature 流程缺少 Task 子流程"))?;
        let task_revision = reference(s, task_ref, "workflow", &scope)?;
        let mut task_plan: WorkflowDocument = task_revision.decode()?;
        validate_workflow(&task_plan)?;
        require(
            task_plan.process_kind == "task",
            "Task 子流程必须引用 Task 用途的发布版本",
        )?;
        hashes.insert(task_ref.clone(), task_revision.content_hash.clone());
        for (key, slot) in &mut task_plan.binding_slots {
            slot.agent_ref = agents.get(key).cloned().or_else(|| slot.agent_ref.clone());
        }
        let standard = text(&current.data, "standard_ref")?;
        if let Some(r) = s.governance.iter().find(|r| r.id == standard) {
            reference(s, &standard, "standard", &scope)?;
            hashes.insert(r.id.clone(), r.content_hash.clone());
        }
        let std = s
            .definitions
            .iter()
            .find(|d| d.id == standard && d.family == "standard")
            .ok_or_else(|| err("not_found", "Standard 不存在"))?;
        hashes.insert(
            std.id.clone(),
            s.governance
                .iter()
                .find(|r| r.id == std.id)
                .map(|r| r.content_hash.clone())
                .unwrap_or(configuration_hash(std)),
        );
        for spec in &std.slots {
            let d = s
                .definitions
                .iter()
                .find(|d| d.id == spec.spec)
                .ok_or_else(|| err("not_found", "Spec 不存在"))?;
            hashes.insert(
                d.id.clone(),
                s.governance
                    .iter()
                    .find(|r| r.id == d.id)
                    .map(|r| r.content_hash.clone())
                    .unwrap_or(configuration_hash(d)),
            );
        }
        let defaults_bundle_ref = opt(&current.data, "defaults_bundle_ref");
        if let Some(bundle) = &defaults_bundle_ref {
            let record = s.configuration(bundle)?;
            hashes.insert(record.id.clone(), record.content_hash.clone());
            if let Some(sandbox) = s
                .sandboxes
                .iter()
                .find(|sandbox| sandbox.id == f.binding.sandbox)
            {
                hashes.insert(sandbox.id.clone(), configuration_hash(sandbox));
            }
        }
        let config = FeatureConfiguration {
            integration_locks: BTreeMap::new(),
            specification_snapshot: vec![],
            baseline_check: None,
            role_agent_bindings: BTreeMap::new(),
            defaults_bundle_ref,
            project_configuration_id: current.id,
            workflow_ref: wf.id,
            standard_ref: standard.clone(),
            agent_refs: agents,
            definition_hashes: hashes,
            plan,
            task_plan,
            compiler_version: 1,
        };
        let lock = record(
            "feature_lock",
            feature.into(),
            scope.clone(),
            format!("{} 配置锁", f.title),
            1,
            None,
            json!(config),
            true,
        );
        let lock_id = lock.id.clone();
        s.governance.push(lock);
        let runtime = WorkflowRuntime {
            owner_id: feature.into(),
            feature_id: feature.into(),
            task_id: None,
            parent_id: None,
            configuration_lock_id: lock_id,
            workflow_ref: config.workflow_ref.clone(),
            status: "active".into(),
            paused: false,
            dirty: true,
            steps: config
                .plan
                .steps
                .iter()
                .map(|step| WorkflowStepRuntime {
                    id: id(),
                    key: step.key.clone(),
                    round: 1,
                    status: "pending".into(),
                    input_hash: String::new(),
                    outputs: BTreeMap::new(),
                    reason: String::new(),
                    revision: 1,
                    confirmation: None,
                })
                .collect(),
            task_members: vec![],
            task_plan_revision: None,
            generation: 1,
        };
        s.governance.push(record(
            "workflow_instance",
            feature.into(),
            scope,
            f.title,
            1,
            None,
            json!(runtime),
            false,
        ));
        let f = s.features.iter_mut().find(|f| f.id == feature).unwrap();
        f.binding.standard = standard;
        if let Some(agent) = config.agent_refs.get("developer") {
            f.binding.agent = agent.clone();
            f.binding.skills = s
                .definitions
                .iter()
                .find(|d| &d.id == agent)
                .and_then(|d| d.agent_profile.as_ref())
                .map(|p| p.skill_revisions.clone())
                .unwrap_or_default();
        }
        Ok(())
    }

    pub(crate) fn governance_command(
        &self,
        method: &str,
        c: &Command,
        s: &mut EngineeringState,
    ) -> Result<Option<Value>> {
        let p = &c.payload;
        let target = c.target.as_deref().unwrap_or("");
        if [
            "workflow.pause",
            "workflow.resume",
            "workflow.step.confirm",
            "workflow.step.retry",
            "workflow.step.rework",
        ]
        .contains(&method)
        {
            return self.workflow_control(method, c, s).map(Some);
        }
        let family = method.split('.').next().unwrap_or("");
        let config_create = [
            "delivery_type",
            "model",
            "tool",
            "harness",
            "skill",
            "spec",
            "standard",
            "role",
            "capability",
            "sandbox",
        ]
        .contains(&family)
            && ["create", "revise"].contains(&method.split('.').nth(1).unwrap_or(""))
            || (["agent.create", "agent.revise"].contains(&method) && p.get("data").is_some());
        if config_create {
            let old = if method.ends_with(".revise") {
                Some(s.configuration(target)?.clone())
            } else {
                None
            };
            let scope = old
                .as_ref()
                .map(|r| r.scope.clone())
                .unwrap_or(decode(p.get("scope").unwrap_or(&json!({"kind":"global"})))?);
            if !(family == "role" && self.store.project_id().is_none()) {
                check_scope(s, &scope, self.store.project_id().as_deref())?;
            }
            if let Some(r) = &old {
                require(
                    r.kind == family && c.expected_revision == Some(r.revision),
                    "配置修订冲突或类型错误",
                )?;
                require(
                    !s.governance.iter().any(|next| {
                        next.kind == r.kind
                            && next.definition_id == r.definition_id
                            && next.version > r.version
                    }),
                    "定义已有新版本，请刷新后编辑",
                )?;
            }
            let mut data = p
                .get("data")
                .cloned()
                .ok_or_else(|| err("invalid_params", "缺少配置正文"))?;
            if family == "agent" {
                crate::configuration::normalize_agent(s, &mut data, &scope)?;
            }
            match family {
                "sandbox" => {
                    let sandbox: SandboxDefinition = decode(&data)?;
                    require(
                        sandbox.provider == "docker"
                            && sandbox.runtime_version == "1"
                            && sandbox.enforcement == "docker-worker/v1"
                            && !sandbox.platforms.is_empty()
                            && sandbox
                                .platforms
                                .iter()
                                .all(|v| ["linux/aarch64", "linux/x86_64"].contains(&v.as_str())),
                        "不支持的 Sandbox 执行合同",
                    )?;
                }
                "role" => {
                    let role: RoleDefinition = decode(&data)?;
                    require(
                        !role.instructions.trim().is_empty(),
                        "Role 职责说明不能为空",
                    )?;
                    if let Some(old) = &old {
                        require(
                            !s.governance.iter().any(|r| {
                                r.kind == "role"
                                    && r.definition_id == old.definition_id
                                    && r.version > old.version
                            }),
                            "Role 已有新版本，请重新读取后编辑",
                        )?;
                        require(
                            !crate::builtin_defaults::role_deleted(s, old),
                            "已删除的 Role 不能修改，请新建角色",
                        )?;
                    }
                    if let Some(visibility) = &role.visibility {
                        visibility.validate()?;
                    }
                }
                "capability" => crate::configuration::validate_capability(s, &data, &scope)?,
                "model" => {
                    let m: ModelDefinition = decode(&data)?;
                    validate_model_definition(&m)?;
                }
                "harness" => {
                    let h: HarnessDefinition = decode(&data)?;
                    require(
                        h.recipe_digest.as_ref().is_none_or(|digest| {
                            ["codex", "dsh"].contains(&h.provider.as_str())
                                && digest.len() == 64
                                && digest.bytes().all(|b| b.is_ascii_hexdigit())
                        }),
                        "无效的固定 Harness 配方",
                    )?;
                    require(
                        ["codex", "dsh"].contains(&h.provider.as_str())
                            && !h.runtime_version.is_empty(),
                        "无效 Harness 或运行版本",
                    )?;
                }
                "tool" => {
                    let t: ToolDefinition = decode(&data)?;
                    require(
                        t.harness_provider
                            .as_deref()
                            .is_none_or(|p| ["codex", "dsh"].contains(&p)),
                        "无效原生工具组 Harness",
                    )?;
                    require(
                        t.implementation != "experience.lookup",
                        "Agent 经验属于可检索与恢复的上下文，不能定义为 Tool",
                    )?;
                    require(
                        !t.qualified_name.is_empty()
                            && t.input_schema.is_object()
                            && t.output_schema.is_object(),
                        "无效 Tool 契约",
                    )?;
                }
                "agent" => validate_composition(s, &decode(&data)?, &scope)?,
                "standard" if data.get("standard_ref").is_some() => {
                    let sid = text(&data, "standard_ref")?;
                    require(
                        s.definitions
                            .iter()
                            .any(|d| d.id == sid && d.family == "standard"),
                        "Standard 修订不存在",
                    )?;
                }
                _ => {}
            }
            let definition_id = old
                .as_ref()
                .map(|r| r.definition_id.clone())
                .unwrap_or_else(id);
            let version = s
                .governance
                .iter()
                .filter(|r| r.kind == family && r.definition_id == definition_id)
                .map(|r| r.version)
                .max()
                .unwrap_or(0)
                + 1;
            let name = opt(p, "name")
                .or_else(|| old.as_ref().map(|r| r.name.clone()))
                .ok_or_else(|| err("invalid_params", "需要名称"))?;
            let r = record(
                family,
                definition_id,
                scope,
                name,
                version,
                old.map(|r| r.id),
                data,
                true,
            );
            if family == "agent" {
                install_agent_profile(s, &r, &r.decode()?);
            }
            if family == "delivery_type" { r.decode::<DeliveryTypeDefinition>()?.validate()?; }
            if family == "skill" {
                if let Some(previous) = r.supersedes.as_ref().and_then(|key| {
                    s.governance
                        .iter()
                        .find(|p| p.kind == "skill_package" && p.definition_id == *key)
                }) {
                    let mut package: SkillPackage = previous.decode()?;
                    package.body = r.data["body"].as_str().unwrap_or("").into();
                    package.source = format!("edited:{}", package.source);
                    package.validate()?;
                    s.governance.push(record(
                        "skill_package",
                        r.id.clone(),
                        r.scope.clone(),
                        r.name.clone(),
                        1,
                        None,
                        json!(package),
                        true,
                    ));
                }
            }
            if ["skill", "spec", "standard"].contains(&family) {
                let source = r.data["standard_ref"]
                    .as_str()
                    .and_then(|id| s.definitions.iter().find(|d| d.id == id))
                    .cloned();
                let body = source
                    .as_ref()
                    .map(|d| d.body.clone())
                    .or_else(|| r.data["body"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                let slots: Vec<SpecSlot> = source.as_ref().map(|d| d.slots.clone()).unwrap_or(
                    r.data
                        .get("slots")
                        .map(decode)
                        .transpose()?
                        .unwrap_or_default(),
                );
                let rules: Vec<String> = source.as_ref().map(|d| d.rules.clone()).unwrap_or(
                    r.data
                        .get("rules")
                        .map(decode)
                        .transpose()?
                        .unwrap_or_default(),
                );
                require(!body.trim().is_empty(), "Skill/Spec/Standard 正文不能为空")?;
                if family == "spec" {
                    let specification: SpecificationDefinition = decode(&r.data)?;
                    require(!specification.body.trim().is_empty(), "Spec 正文不能为空")?;
                    for selector in &specification.applicable_actions {
                        selector.validate()?;
                        require(
                            workflow_action_catalog()
                                .as_array()
                                .unwrap()
                                .contains(&json!(selector.kind)),
                            "Spec 指向未知动作",
                        )?;
                    }
                    require(
                        rules.iter().all(|r| {
                            ["traceability", "failure_paths", "sections"].contains(&r.as_str())
                        }),
                        "Spec 需要已有检查器支持的规则",
                    )?;
                }
                if family == "standard" && r.data.get("baseline_rules").is_some() {
                    let standard: StandardDefinition = decode(&r.data)?;
                    check_baseline(&r.id, &standard, &[])?;
                    for rule in &standard.baseline_rules {
                        for id in &rule.required_spec_refs {
                            reference(s, id, "spec", &r.scope)?;
                        }
                    }
                }
                if family == "standard" && r.data.get("baseline_rules").is_none() {
                    for purpose in ["planning", "implementation", "merge"] {
                        let slot =
                            slots.iter().find(|s| s.purpose == purpose).ok_or_else(|| {
                                err("invalid_standard", format!("缺少 {purpose} slot"))
                            })?;
                        require(
                            s.definitions
                                .iter()
                                .any(|d| d.id == slot.spec && d.family == "spec"),
                            "Spec 修订不存在",
                        )?;
                        if s.governance.iter().any(|r| r.id == slot.spec) {
                            reference(s, &slot.spec, "spec", &r.scope)?;
                        }
                    }
                }
                s.definitions.push(Definition {
                    id: r.id.clone(),
                    family: family.into(),
                    name: r.name.clone(),
                    version: r.version,
                    supersedes: r.supersedes.clone(),
                    body,
                    slots,
                    rules,
                    agent_profile: None,
                });
            }

            let result = json!(r);
            s.governance.push(r.clone());
            if family == "agent" {
                crate::configuration::migrate_capabilities(s);
            }

            return Ok(Some(result));
        }
        let result = match method {
            "role.delete" => {
                let role = s.configuration(target)?.clone();
                if self.store.project_id().is_some() {
                    check_scope(s, &role.scope, self.store.project_id().as_deref())?;
                }
                require(
                    role.kind == "role" && c.expected_revision == Some(role.revision),
                    "Role 类型或版本不匹配",
                )?;
                crate::configuration::ensure_unreferenced(s, &role)?;
                if !crate::builtin_defaults::role_deleted(s, &role) {
                    s.governance.push(record("role_retirement",role.definition_id.clone(),role.scope.clone(),role.name.clone(),1,None,json!({"role_ref":role.id,"reason":opt(p,"reason").unwrap_or_else(||"用户删除角色".into())}),true));
                }
                json!({"id":role.id,"deleted":true})
            }
            "configuration.retire" => {
                let source = s.configuration(target)?.clone();
                if !(source.kind == "role" && self.store.project_id().is_none()) {
                    check_scope(s, &source.scope, self.store.project_id().as_deref())?;
                }
                require(source.is_definition(), "只能停用定义修订")?;
                crate::configuration::ensure_unreferenced(s, &source)?;
                require(source.kind != "role", "请使用 role.delete 删除角色")?;
                require(c.expected_revision == Some(source.revision), "版本冲突")?;
                let r = record(
                    "configuration_retirement",
                    target.into(),
                    source.scope,
                    source.name,
                    1,
                    None,
                    json!({"reason":text(p,"reason")?}),
                    true,
                );
                s.governance.push(r);
                json!({"id":target,"retired":true})
            }
            "workflow.create" => {
                let scope: Scope = decode(p.get("scope").unwrap_or(&json!({"kind":"global"})))?;
                check_scope(s, &scope, self.store.project_id().as_deref())?;
                let document = if let Some(source) = opt(p, "template_ref") {
                    reference(s, &source, "workflow", &scope)?.data.clone()
                } else {
                    p.get("document")
                        .cloned()
                        .unwrap_or_else(|| json!(default_workflow(false)))
                };
                let document: WorkflowDocument = decode(&document)?;
                let definition_id = id();
                let r = record(
                    "workflow_draft",
                    definition_id,
                    scope,
                    text(p, "name")?,
                    1,
                    None,
                    json!({"document":document,"base_revision_id":null,"published_revision_id":null}),
                    false,
                );
                let result = json!(r);
                s.governance.push(r);
                result
            }
            "workflow.task_upgrade" => {
                let source = s.configuration(target)?.clone();
                require(
                    ["workflow", "workflow_draft"].contains(&source.kind.as_str()),
                    "需要 Workflow 来源",
                )?;
                check_scope(s, &source.scope, self.store.project_id().as_deref())?;
                require(
                    c.expected_revision == Some(source.revision),
                    "来源修订已改变",
                )?;
                let doc: WorkflowDocument = decode(if source.kind == "workflow_draft" {
                    &source.data["document"]
                } else {
                    &source.data
                })?;
                let (document, diagnostics) = crate::default_tasks::upgrade_draft(&doc)?;
                let base = if source.kind == "workflow_draft" {
                    source.data["base_revision_id"].clone()
                } else {
                    json!(source.id)
                };
                let r = record(
                    "workflow_draft",
                    source.definition_id,
                    source.scope,
                    source.name,
                    source.version + 1,
                    None,
                    json!({"document":document,"base_revision_id":base,"published_revision_id":null,"task_upgrade_source":source.id,"task_upgrade_diagnostics":diagnostics}),
                    false,
                );
                let result = json!(r);
                s.governance.push(r);
                result
            }
            "workflow.draft.create" => {
                let source = s.configuration(&text(p, "base_revision_id")?)?.clone();
                require(source.kind == "workflow", "需要已发布流程版本")?;
                let scope: Scope = p
                    .get("scope")
                    .map(decode)
                    .transpose()?
                    .unwrap_or(source.scope.clone());
                check_scope(s, &scope, self.store.project_id().as_deref())?;
                require(source.scope.visible_from(&scope), "流程来源不可见")?;
                let definition_id = if scope == source.scope {
                    source.definition_id.clone()
                } else {
                    id()
                };
                let r = record(
                    "workflow_draft",
                    definition_id,
                    scope,
                    opt(p, "name").unwrap_or(source.name),
                    source.version + 1,
                    None,
                    json!({"document":source.data,"base_revision_id":source.id,"published_revision_id":null}),
                    false,
                );
                let result = json!(r);
                s.governance.push(r);
                result
            }
            "workflow.draft.update" => {
                let old = s.configuration(target)?.clone();
                check_scope(s, &old.scope, self.store.project_id().as_deref())?;
                require(
                    old.kind == "workflow_draft" && old.data["published_revision_id"].is_null(),
                    "只能编辑未发布草稿",
                )?;
                require(
                    c.expected_revision == Some(old.revision),
                    "草稿已被修改，请重新读取合并",
                )?;
                let mut data = old.data;
                let document: WorkflowDocument = decode(
                    p.get("document")
                        .ok_or_else(|| err("invalid_params", "需要流程正文"))?,
                )?;
                data["document"] = json!(document);
                let r = s.governance.iter_mut().find(|r| r.id == target).unwrap();
                if let Some(name) = opt(p, "name") {
                    r.name = name;
                }
                update_record(r, data);
                json!(r)
            }
            "workflow.publish" => {
                let draft = s.configuration(target)?.clone();
                check_scope(s, &draft.scope, self.store.project_id().as_deref())?;
                require(
                    draft.kind == "workflow_draft" && c.expected_revision == Some(draft.revision),
                    "草稿版本冲突",
                )?;
                if let Some(published) = draft.data["published_revision_id"].as_str() {
                    return Ok(Some(json!(s.configuration(published)?)));
                }
                let doc: WorkflowDocument = decode(&draft.data["document"])?;
                validate_workflow(&doc)?;
                if doc.schema_version >= 2 {
                    let mut proposed = draft.clone();
                    proposed.data = json!(doc);
                    crate::workflow_definition::compile(s, &proposed, None, &draft.scope)?;
                }
                for (key, r) in &doc.subworkflow_refs {
                    require(
                        doc.process_kind == "feature" && key == "task",
                        "当前编译器只支持 Feature 的 Task 子流程引用",
                    )?;
                    let child: WorkflowDocument =
                        reference(s, r, "workflow", &draft.scope)?.decode()?;
                    require(child.process_kind == "task", "Task 引用用途不匹配")?;
                }
                for slot in doc.binding_slots.values() {
                    if let Some(a) = &slot.agent_ref {
                        let a: AgentComposition =
                            reference(s, a, "agent", &draft.scope)?.decode()?;
                        require(a.role == slot.role, "Agent 职责不匹配")?;
                    }
                }
                let version = s
                    .governance
                    .iter()
                    .filter(|r| r.kind == "workflow" && r.definition_id == draft.definition_id)
                    .map(|r| r.version)
                    .max()
                    .unwrap_or(0)
                    + 1;
                let r = record(
                    "workflow",
                    draft.definition_id,
                    draft.scope,
                    draft.name,
                    version,
                    draft.data["base_revision_id"].as_str().map(str::to_owned),
                    json!(doc),
                    true,
                );
                let result = json!(r);
                let mut data = draft.data;
                data["published_revision_id"] = json!(r.id);
                s.governance.push(r);
                update_record(
                    s.governance.iter_mut().find(|r| r.id == target).unwrap(),
                    data,
                );
                result
            }
            "agent.instance.create" => {
                let scope: Scope = decode(&p["scope"])?;
                check_scope(s, &scope, self.store.project_id().as_deref())?;
                require(
                    !matches!(scope, Scope::Feature { .. }),
                    "实例只能绑定 Global 或 Project",
                )?;
                let key = text(p, "agent_revision_id")?;
                let agent = if let Some(feature) = opt(p, "frozen_feature_id") {
                    let f = s.feature(&feature)?;
                    let lock = s
                        .feature_configuration(&feature)
                        .ok_or_else(|| err("configuration_required", "缺少 Feature 配置锁"))?;
                    let agent = s.configuration(&key)?;
                    require(
                        scope.project() == Some(f.project_id.as_str())
                            && lock.agent_refs.values().any(|id| id == &key)
                            && lock.definition_hashes.get(&key) == Some(&agent.content_hash)
                            && agent.kind == "agent"
                            && agent.immutable,
                        "Agent 不属于该项目的冻结 Feature",
                    )?;
                    agent
                } else {
                    reference(s, &key, "agent", &scope)?
                };
                let data = AgentInstance {
                    agent_revision_id: agent.id.clone(),
                    scope: scope.clone(),
                    source_instance_id: opt(p, "source_instance_id"),
                };
                let r = record(
                    "agent_instance",
                    id(),
                    scope,
                    text(p, "name")?,
                    1,
                    None,
                    json!(data),
                    true,
                );
                let result = json!(r);
                s.governance.push(r);
                result
            }
            "project.configuration.update" => {
                let project = s
                    .projects
                    .iter()
                    .find(|x| x.id == target)
                    .ok_or_else(|| err("not_found", "Project 不存在"))?
                    .clone();
                let scope = Scope::Project {
                    project_id: target.into(),
                };
                let wf = reference(s, &text(p, "workflow_ref")?, "workflow", &scope)?.clone();
                let doc: WorkflowDocument = wf.decode()?;
                if doc.schema_version >= 2 {
                    return self.update_project_configuration_v2(s, c, &wf).map(Some);
                }
                require(doc.process_kind == "feature", "项目默认需要 Feature 流程")?;
                validate_workflow(&doc)?;
                let standard = text(p, "standard_ref")?;
                if s.governance.iter().any(|r| r.id == standard) {
                    reference(s, &standard, "standard", &scope)?;
                }
                require(
                    s.definitions
                        .iter()
                        .any(|d| d.id == standard && d.family == "standard"),
                    "Standard 修订不存在",
                )?;
                let agents: BTreeMap<String, String> = p
                    .get("agent_refs")
                    .map(decode)
                    .transpose()?
                    .unwrap_or_default();
                for (key, slot) in &doc.binding_slots {
                    let ar = agents.get(key).or(slot.agent_ref.as_ref()).ok_or_else(|| {
                        err(
                            "configuration_required",
                            format!("角色 {key} 尚未选择完整 Agent"),
                        )
                    })?;
                    let a: AgentComposition = reference(s, ar, "agent", &scope)?.decode()?;
                    require(a.role == slot.role, "Agent 职责不匹配")?;
                    validate_composition(s, &a, &scope)?;
                }
                let previous = s
                    .governance
                    .iter()
                    .filter(|r| r.kind == "project_configuration" && r.definition_id == target)
                    .max_by_key(|r| r.version)
                    .cloned();
                if let Some(expected) = opt(p, "source_configuration_id") {
                    require(
                        previous.as_ref().is_some_and(|r| r.id == expected),
                        "Project 默认配置已改变",
                    )?;
                }
                let r = record(
                    "project_configuration",
                    target.into(),
                    scope,
                    format!("{} 默认配置", project.title),
                    previous.as_ref().map_or(1, |r| r.version + 1),
                    previous.map(|r| r.id),
                    json!({"workflow_ref":wf.id,"standard_ref":standard,"agent_refs":agents}),
                    true,
                );
                let result = json!(r);
                s.governance.push(r);
                let p = s.projects.iter_mut().find(|x| x.id == target).unwrap();
                p.revision += 1;
                result
            }
            _ => return Ok(None),
        };
        Ok(Some(result))
    }

    pub(crate) fn governance_query(&self, method: &str, p: &Value) -> Option<Result<Value>> {
        if ![
            "query.configurations",
            "query.agent_instances",
            "query.roles",
            "query.agent_revision",
            "query.workflows",
            "query.workflow",
            "query.workflow_draft",
            "query.workflow_validation",
            "query.workflow_preview",
            "query.workflow_version_diff",
            "query.workflow_instance",
            "query.workflow_actions",
            "query.project_configuration",
            "query.project_workflow_summary",
        ]
        .contains(&method)
        {
            return None;
        }
        Some((|| {
            let s = self.store.snapshot()?;
            if method == "query.roles" {
                let all = s
                    .governance
                    .iter()
                    .filter(|r| {
                        r.kind == "role"
                            && crate::configuration::visible(&s, r, p["project_id"].as_str())
                            && (p["include_deleted"] == true
                                || !crate::builtin_defaults::role_deleted(&s, r))
                    })
                    .collect::<Vec<_>>();
                let mut rows = all
                    .iter()
                    .filter(|r| {
                        p["include_history"] == true
                            || !all.iter().any(|next| {
                                next.definition_id == r.definition_id && next.version > r.version
                            })
                    })
                    .copied()
                    .collect::<Vec<_>>();
                rows.sort_by_key(|r| std::cmp::Reverse(r.created_at));
                let offset = p["offset"].as_u64().unwrap_or(0) as usize;
                let limit = p["limit"].as_u64().unwrap_or(50).clamp(1, 100) as usize;
                return Ok(
                    json!({"records":rows.iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":rows.len()}),
                );
            }
            let offset = p["offset"].as_u64().unwrap_or(0) as usize;
            let limit = p["limit"].as_u64().unwrap_or(50).clamp(1, 100) as usize;
            if ["query.workflow_validation", "query.workflow_preview"].contains(&method) {
                let doc: WorkflowDocument = if let Some(document) = p.get("document") {
                    decode(document)?
                } else {
                    let r = s.configuration(&text(p, "id")?)?;
                    decode(if r.kind == "workflow_draft" {
                        &r.data["document"]
                    } else {
                        &r.data
                    })?
                };
                return Ok(match validate_workflow(&doc) {
                    Ok(order) => {
                        fn condition(c: &WorkflowCondition, p: &Value) -> bool {
                            match c {
                                WorkflowCondition::Always => true,
                                WorkflowCondition::All { conditions } => {
                                    conditions.iter().all(|c| condition(c, p))
                                }
                                WorkflowCondition::Any { conditions } => {
                                    conditions.iter().any(|c| condition(c, p))
                                }
                                WorkflowCondition::OutputPresent { step } => p["outputs"]
                                    .as_array()
                                    .is_some_and(|outputs| outputs.contains(&json!(step))),
                                WorkflowCondition::Accepted { kind } => p["accepted"]
                                    .as_array()
                                    .is_some_and(|outputs| outputs.contains(&json!(kind))),
                            }
                        }
                        let mut states: BTreeMap<String, String> = BTreeMap::new();
                        let mut evaluation = vec![];
                        for key in &order {
                            let step = doc.steps.iter().find(|s| &s.key == key).unwrap();
                            let ready = step.dependencies.iter().all(|dep| {
                                states
                                    .get(dep)
                                    .is_some_and(|s| ["completed", "skipped"].contains(&s.as_str()))
                            });
                            let state = if step.kind != "join"
                                && step.dependencies.iter().any(|dep| {
                                    states.get(dep).is_some_and(|state| state == "skipped")
                                }) {
                                "skipped"
                            } else if !ready {
                                "waiting_dependencies"
                            } else if !condition(&step.condition, &p["sample"]) {
                                if step.kind == "branch" {
                                    "skipped"
                                } else {
                                    "waiting_condition"
                                }
                            } else if p["sample"]["outputs"]
                                .as_array()
                                .is_some_and(|outputs| outputs.contains(&json!(key)))
                            {
                                if step.advance_mode == AdvanceMode::HumanConfirmation {
                                    "waiting_confirmation"
                                } else {
                                    "completed"
                                }
                            } else if ["agent.interact", "proposal.adopt"]
                                .contains(&step.kind.as_str())
                            {
                                "waiting_input"
                            } else {
                                "ready"
                            };
                            states.insert(key.clone(), state.into());
                            evaluation.push(json!({"key":key,"state":state,"role":step.participant_slot,"condition":step.condition,"reason":if state=="waiting_dependencies"{"前置结果尚未满足"}else if state=="waiting_confirmation"{"样本结果完成，等待精确输入确认"}else if state=="waiting_condition"{"样本条件不满足"}else{"仅作演练，不生成工程事实"},"rework_entries":step.rework_scope}));
                        }
                        json!({"valid":true,"document_hash":configuration_hash(&doc),"order":order,"preview":doc.steps,"evaluation":evaluation,"external_actions":false})
                    }
                    Err(e) => json!({"valid":false,"errors":[e],"external_actions":false}),
                });
            }
            if method == "query.workflow_version_diff" {
                let a = s.configuration(&text(p, "a")?)?;
                let b = s.configuration(&text(p, "b")?)?;
                return Ok(json!({"a":a,"b":b,"changed":a.content_hash!=b.content_hash}));
            }
            if [
                "query.workflow",
                "query.workflow_draft",
                "query.agent_revision",
                "query.workflow_instance",
                "query.workflow_actions",
            ]
            .contains(&method)
            {
                let r = s.configuration(&text(p, "id")?)?;
                require(
                    match method {
                        "query.workflow" => {
                            ["workflow", "workflow_draft"].contains(&r.kind.as_str())
                        }
                        "query.workflow_draft" => r.kind == "workflow_draft",
                        "query.agent_revision" => r.kind == "agent",
                        _ => r.kind == "workflow_instance",
                    },
                    "配置类型与查询不匹配",
                )?;
                if ["query.workflow_instance", "query.workflow_actions"].contains(&method) {
                    let runtime: WorkflowRuntime = r.decode()?;
                    let lock: FeatureConfiguration =
                        s.configuration(&runtime.configuration_lock_id)?.decode()?;
                    let plan = if runtime.task_id.is_some() {
                        &lock.task_plan
                    } else {
                        &lock.plan
                    };
                    let actions: Vec<_> = runtime
                        .steps
                        .iter()
                        .map(|step| {
                            let definition = plan
                                .steps
                                .iter()
                                .find(|definition| definition.key == step.key)
                                .unwrap();
                            let rework_entries = if !definition.rework_scope.is_empty() {
                                definition.rework_scope.clone()
                            } else if definition.kind == "merge.apply" {
                                plan.steps
                                    .iter()
                                    .filter(|definition| definition.kind == "merge.prepare")
                                    .map(|definition| definition.key.clone())
                                    .collect()
                            } else if ["delivery.review", "merge.review"]
                                .contains(&definition.kind.as_str())
                            {
                                definition.dependencies.clone()
                            } else {
                                vec![definition.key.clone()]
                            };
                            let quality_decision = step.outputs.get("delivery").is_some_and(|id| {
                                s.reviews.iter().any(|review| {
                                    &review.delivery_id == id
                                        && [Decision::Pass, Decision::ChangesRequested]
                                            .contains(&review.decision)
                                })
                            });
                            let proved_not_applied = step.outputs.get("merge").is_some_and(|id| {
                                s.advances.iter().any(|advance| {
                                    &advance.id == id && advance.status == AdvanceStatus::NotApplied
                                })
                            });
                            let retry_allowed = !quality_decision
                                && (definition.kind != "merge.apply" || proved_not_applied)
                                && s.governance
                                    .iter()
                                    .filter(|r| {
                                        r.kind == "workflow_retry" && r.definition_id == step.id
                                    })
                                    .count()
                                    < definition.retry_limit as usize;
                            let allowed = match step.status.as_str() {
                                "waiting_confirmation" => {
                                    vec!["workflow.step.confirm", "workflow.step.rework"]
                                }
                                "failed" | "blocked" if retry_allowed => {
                                    vec!["workflow.step.retry", "workflow.step.rework"]
                                }
                                "failed" | "blocked" | "completed" => vec!["workflow.step.rework"],
                                _ => vec![],
                            };
                            let allowed =
                                if ["completed", "cancelled"].contains(&runtime.status.as_str()) {
                                    vec![]
                                } else {
                                    allowed
                                };
                            json!({"id":step.id,"rework_entries":rework_entries,"allowed":allowed,
                                "model_approvals":crate::model_approvals::reports(&s, step)})
                        })
                        .collect();
                    let children: Vec<_> = s.governance.iter().filter(|child| child.kind == "workflow_instance"
                        && child.data["parent_id"] == r.id && child.data["status"] != "cancelled"
                        && child.data["owner_id"].as_str().is_some_and(|id|runtime.task_members.iter().any(|t|t==id)))
                        .map(|child| {
                            let owner=child.data["owner_id"].as_str().unwrap_or("");
                            let parent_step=runtime.steps.iter().find(|step|step.outputs.get("group").and_then(|id|s.configuration(id).ok()).is_some_and(|group|group.data["members"].as_array().is_some_and(|ids|ids.iter().any(|id|id==owner))));
                            json!({"record":{"id":child.id,"revision":child.revision,"data":{"owner_id":child.data["owner_id"],"status":child.data["status"],"paused":child.data["paused"],"task_members":child.data["task_members"]}},"title":s.tasks.iter().find(|t|t.id==owner).map(|t|t.title.as_str()).unwrap_or(&child.name),"parent_step_id":parent_step.map(|step|&step.id)})
                        }).collect();
                    return Ok(
                        json!({"record":r,"plan":plan,"configuration":lock,"actions":actions,"children":children}),
                    );
                }
                return Ok(json!(r));
            }
            if method == "query.project_configuration" {
                return Ok(
                    json!({"configuration":s.governance.iter().filter(|r|r.kind=="project_configuration"&&p["project_id"].as_str().is_none_or(|pid|r.definition_id==pid)).max_by_key(|r|r.version),"standards":s.definitions.iter().filter(|d|d.family=="standard").collect::<Vec<_>>()}),
                );
            }
            let rows: Vec<_> = s
                .governance
                .iter()
                .filter(|r| !r.is_legacy_experience_tool())
                .filter(|r| crate::configuration::visible(&s, r, p["project_id"].as_str()))
                .rev()
                .filter(|r| p["include_deleted"] == true || !crate::configuration::retired(&s, r))
                .filter(|r| {
                    p["include_history"] == true
                        || !s.governance.iter().any(|n| {
                            n.kind == r.kind
                                && n.definition_id == r.definition_id
                                && n.version > r.version
                        })
                })
                .filter(|r| match method {
                    "query.workflows" => ["workflow", "workflow_draft"].contains(&r.kind.as_str()),
                    "query.agent_instances" => r.kind == "agent_instance",
                    "query.project_workflow_summary" => r.kind == "workflow_instance",
                    _ => p["kind"]
                        .as_str()
                        .map_or(r.is_definition() || r.kind == "agent_instance", |kind| {
                            r.kind == kind
                        }),
                })
                .filter(|r| {
                    p.get("scope")
                        .is_none_or(|scope| serde_json::to_value(&r.scope).unwrap() == *scope)
                })
                .collect();
            Ok(
                json!({"records":rows.iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":rows.len(),"action_catalog":workflow_action_catalog(),"action_contracts":workflow_action_contracts()}),
            )
        })())
    }
}
