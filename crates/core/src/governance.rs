//! Configuration commands, immutable publication and workflow runtime coordination.
use crate::*;

pub(crate) fn validate_source_payload(
    s: &EngineeringState,
    family: &str,
    refs: &References,
    data: &Value,
    scope: &Scope,
    old: Option<&ConfigurationRecord>,
) -> Result<()> {
    validate_source_data(family, &refs, &data)?;
    if family == "agent" {
        crate::configuration::normalize_agent(s, &refs, &scope)?;
    }
    match family {
        "sandbox" => {
            let sandbox: SandboxDefinition = decode(&data)?;
            require(
                sandbox.provider == "docker"
                    && !sandbox.runtime_version.is_empty()
                    && !sandbox.enforcement.is_empty()
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
            if let Some(visibility) = &role.visibility {
                visibility.validate()?;
            }
        }
        "permission_context" => {
            validate_source_data("permission_context", &refs, &data)?;
            let context: PermissionContextDefinition = decode(&data)?;
            reference(s, &reference_id(&refs, "sandbox_ref")?, "sandbox", &scope)?;
            for (selection, kind) in [
                (&context.harness, PermissionTargetKind::Harness),
                (&context.sandbox, PermissionTargetKind::Sandbox),
            ] {
                if let PermissionSelection::Pinned {
                    binding_revision_id,
                } = selection
                {
                    let binding: VersionedPermissionBinding =
                        reference(s, binding_revision_id, "permission_binding", &scope)?
                            .decode()?;
                    require(binding.target.kind == kind, "授权绑定层不匹配")?;
                }
            }
            for grant in context.step_grants {
                let workflow: WorkflowDefinition =
                    reference(s, &grant.workflow_ref, "workflow", &scope)?.decode()?;
                require(
                    workflow.steps.iter().any(|step| step.key == grant.step),
                    "授权引用了不存在的步骤",
                )?;
                for tool in grant.tool_refs {
                    reference(s, &tool, "tool", &scope)?;
                }
            }
        }
        "delivery_type" => {
            let delivery: DeliveryTypeDefinition = decode(&data)?;
            s.validate_delivery_specs(&refs, &delivery, &scope)?;
            for spec in reference_list(&refs, "spec_refs")? {
                reference(s, &spec, "spec", &scope)?
                    .decode::<ConfiguredSpecification>()?
                    .validate()?;
            }
        }
        "capability" => crate::configuration::validate_capability(s, &refs, &scope)?,
        "model" => {
            let m: ModelDefinition = decode(&data)?;
            validate_model_definition(&m, &s.provider_facts_any(&m.provider)?)?;
        }
        "model_provider" => crate::configuration::validate_model_provider(
            s,
            &data,
            old.as_ref().map(|r| r.definition_id.as_str()),
        )?,
        "model_catalog" => crate::configuration::validate_model_catalog(s, &data, &scope)?,
        "harness" => crate::configuration::validate_harness(s, &refs, &data, &scope)?,
        "tool" => {
            let t: ToolDefinition = decode(&data)?;
            require(
                match t.implementation.as_str() {
                    "platform" => crate::platform_tools::PLATFORM_HANDLERS
                        .contains(&t.qualified_name.as_str()),
                    "harness.native" => true,
                    _ => false,
                },
                "未知 Tool 执行原语或平台处理器",
            )?;
            require(
                t.harness_provider
                    .as_deref()
                    .is_none_or(known_harness_provider),
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
        "agent" => {
            reference(s, &reference_id(&refs, "role_ref")?, "role", &scope)?;
            reference(
                s,
                &reference_id(&refs, "capability_ref")?,
                "capability",
                &scope,
            )?;
        }
        _ => {}
    }
    Ok(())
}
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
    record_with_refs(
        kind,
        definition_id,
        scope,
        name,
        version,
        supersedes,
        References::new(),
        data,
        immutable,
    )
}
pub(crate) fn record_with_refs(
    kind: &str,
    definition_id: Id,
    scope: Scope,
    name: String,
    version: u64,
    supersedes: Option<Id>,
    refs: References,
    data: Value,
    immutable: bool,
) -> ConfigurationRecord {
    let source_kind = [
        "model",
        "model_provider",
        "model_catalog",
        "runtime_resource",
        "orchestration_policy",
        "tool",
        "harness",
        "agent",
        "workflow",
        "standard",
        "skill",
        "spec",
        "delivery_type",
        "role",
        "capability",
        "integration",
        "integration_binding",
        "sandbox",
        "permission_profile",
        "permission_binding",
        "permission_assignment",
        "permission_context",
        "project_configuration",
        "integration_project_configuration",
    ]
    .contains(&kind);
    ConfigurationRecord {
        id: if source_kind {
            format!("{definition_id}:{version}")
        } else {
            id()
        },
        r#type: kind.into(),
        definition_id,
        scope,
        name,
        version,
        supersedes,
        content_hash: content_digest(&refs, &data),
        refs,
        data,
        immutable,
        revision: 1,
        created_at: if source_kind { 0 } else { now() },
    }
}
pub(crate) fn update_record(r: &mut ConfigurationRecord, data: Value) {
    if r.data != data {
        r.content_hash = content_digest(&r.refs, &data);
        r.data = data;
        r.revision += 1;
    }
}
fn decode<T: serde::de::DeserializeOwned>(v: &Value) -> Result<T> {
    serde_json::from_value(v.clone()).map_err(|e| err("invalid_params", e.to_string()))
}
fn check_scope(s: &EngineeringState, scope: &Scope, store_project: Option<&str>) -> Result<()> {
    if scope.is_global() {
        require(store_project.is_none(), "全局定义必须由全局配置入口编辑")
    } else if scope.is_feature() {
        Err(err("invalid_scope", "不能直接创建 Feature 局部可变定义"))
    } else {
        let project = scope.project_id();
        require(
            store_project.is_none_or(|p| Some(p) == project)
                && project.is_some_and(|id| s.projects.iter().any(|p| p.id == id)),
            "配置 Project 作用域不匹配",
        )
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
        r.r#type == kind && r.immutable && (r.r#type == "role" || r.scope.visible_from(scope)),
        format!("{id}: 类型或 Scope 不兼容"),
    )?;
    require(
        crate::configuration::visible(s, r, scope.project_id()),
        "该定义不适用于此项目范围",
    )?;
    if kind == "agent" {
        let capability = reference_value(&r.refs, "capability_ref")
            .as_str()
            .ok_or_else(|| err("invalid_configuration", "Agent 必须引用 Capability 修订"))?;
        reference(s, capability, "capability", scope)?;
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
        let cap = reference(s, id, "capability", scope)?;
        let cap_tools = reference_list(&cap.refs, "tools")?;
        let cap_skills = reference_list(&cap.refs, "skills")?;
        require(
            reference_value(&cap.refs, "model_ref").as_str() == Some(a.model_ref.as_str())
                && reference_value(&cap.refs, "harness_ref").as_str()
                    == Some(a.harness_ref.as_str())
                && cap_tools == a.tools
                && cap_skills == a.skills,
            "Agent 执行字段与 Capability 修订不一致",
        )?;
    }

    if let Some(role_ref) = &a.role_ref {
        let role = reference(s, role_ref, "role", scope)?;
        require(
            !crate::builtin_defaults::role_deleted(s, role),
            "Role 已删除，不能用于新绑定",
        )?;
        require(
            role.definition_id == a.role,
            "Agent 的业务身份与 Role 修订不一致",
        )?;
    }
    crate::configuration::validate_capability(
        s,
        &References::from([
            ("model_ref".to_string(), json!(a.model_ref)),
            ("harness_ref".to_string(), json!(a.harness_ref)),
            ("tools".to_string(), json!(a.tools)),
            ("skills".to_string(), json!(a.skills)),
        ]),
        scope,
    )
}
pub(crate) fn install_agent_profile(
    s: &mut EngineeringState,
    r: &ConfigurationRecord,
    a: &AgentComposition,
) {
    if s.definitions.iter().any(|d| d.id == r.id) {
        return;
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

        agent_profile: Some(AgentProfile {
            role: a.role.clone(),
            skill_revisions: a.skills.clone(),
        }),
    });
}

impl Engine {
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
                .find(|r| r.r#type == "agent_session" && r.definition_id == *session_id)
                .cloned();
            if let Some(link) = link {
                if !s.governance.iter().any(|r| {
                    r.r#type == "experience"
                        && r.definition_id == snapshot_id
                        && r.data["session_id"] == *session_id
                }) {
                    let work = s
                        .turns
                        .iter()
                        .rev()
                        .find(|t| t.session_id == *session_id)
                        .and_then(|t| t.finished.or(t.started));
                    s.governance.push(record("experience",snapshot_id.into(),if pid==GLOBAL_DISCUSSION_NAMESPACE {Scope::global()}else{Scope::project(pid.clone())},feature_name.clone().unwrap_or(project_title.clone()),1,None,json!({"agent_instance_id":link.data["agent_instance_id"],"agent_revision_id":link.data["agent_revision_id"],"snapshot_id":snapshot_id,"session_id":session_id,"project_id":pid,"project_name":project_title,"feature_id":snap.feature_id,"feature_name":feature_name.clone().unwrap_or_default(),"task_id":snap.task_id,"captured_at":snap.capture.captured_at,"work_time":work,"binding":link.data["binding"],"kind":snap.kind}),true));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn governance_after_command(
        &self,
        method: &str,
        c: &Command,
        s: &mut EngineeringState,
        result: &Value,
    ) -> Result<()> {
        if method == "conversation.send" {
            self.configured_turn_task(s, &text(result, "id")?)?;
        }
        if method == "session.open" {
            self.configured_session_opened(s, c, result)?;
            if let Some(instance_id) = opt(&c.payload, "agent_instance_id") {
                let instance_record = s.configuration(&instance_id)?.clone();
                require(instance_record.r#type == "agent_instance", "无效 Agent")?;
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
                            Scope::global()
                        } else {
                            Scope::project(project.clone())
                        }),
                    "Agent 不能用于其他项目",
                )?;
                require(
                    session.agent == instance.agent_revision_id,
                    "Agent 与 AgentRevision 不匹配",
                )?;
                let agent = s.resolve_agent(&instance.agent_revision_id)?;
                let model: ModelDefinition = s.configuration(&agent.model_ref)?.decode()?;
                let harness_record = s.configuration(&agent.harness_ref)?.clone();
                let harness: HarnessDefinition = harness_record.decode()?;
                let binding = session
                    .runtime_binding
                    .as_ref()
                    .ok_or_else(|| err("blocked", "Agent Session 缺少冻结绑定"))?;
                require(
                    binding.model == model.model
                        && binding.installation.key.provider.as_str() == harness.provider,
                    "执行绑定改变了 Agent 的 Model/Harness",
                )?;
                let resolved = s.resolve_model_connection(&model, &harness_record)?;
                require(
                    binding.model_connection == resolved,
                    "执行绑定改变了 Model 连接或容量",
                )?;
                if let Some(old) = s
                    .governance
                    .iter()
                    .find(|r| r.r#type == "agent_session" && r.definition_id == session_id)
                {
                    require(
                        old.data["agent_instance_id"] == instance_id,
                        "不能将 Session 重新归属其他 Agent",
                    )?;
                } else {
                    s.governance.push(record("agent_session",session_id,if session.scope==Some(SessionScope::Global){Scope::global()}else{Scope::project(project)},instance_record.name,1,None,json!({"agent_instance_id":instance_id,"agent_revision_id":instance.agent_revision_id,"composition_hash":s.configuration(&instance.agent_revision_id)?.content_hash,"model_ref":agent.model_ref,"harness_ref":agent.harness_ref,"binding":binding,"step_execution_id":c.payload["configured_execution_id"]}),true));
                }
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
            .filter(|r| r.r#type == "project_configuration" && r.definition_id == f.project_id)
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
        self.start_configured_feature(s, feature, &current, p)
    }

    pub(crate) fn governance_command(
        &self,
        method: &str,
        c: &Command,
        s: &mut EngineeringState,
    ) -> Result<Option<Value>> {
        let p = &c.payload;
        let target = c.target.as_deref().unwrap_or("");
        let family = method.split('.').next().unwrap_or("");
        let config_create = [
            "delivery_type",
            "model",
            "model_provider",
            "model_catalog",
            "runtime_resource",
            "orchestration_policy",
            "tool",
            "harness",
            "skill",
            "spec",
            "standard",
            "role",
            "capability",
            "permission_context",
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
                .unwrap_or(decode(p.get("scope").unwrap_or(&json!("global")))?);
            if !(family == "role" && self.store.project_id().is_none()) {
                check_scope(s, &scope, self.store.project_id().as_deref())?;
            }
            if let Some(r) = &old {
                require(
                    r.r#type == family && c.expected_revision == Some(r.revision),
                    "配置修订冲突或类型错误",
                )?;
                require(
                    !crate::configuration::retired(s, r),
                    "已停用的配置不能修改，请新建身份",
                )?;
                require(
                    !s.governance.iter().any(|next| {
                        next.r#type == r.r#type
                            && next.definition_id == r.definition_id
                            && next.version > r.version
                    }),
                    "定义已有新版本，请刷新后编辑",
                )?;
            }
            let data = p
                .get("data")
                .cloned()
                .ok_or_else(|| err("invalid_params", "缺少配置正文"))?;
            let mut refs = References::new();
            for key in reference_fields(family) {
                if let Some(value) = p.get(*key) {
                    refs.insert((*key).to_string(), value.clone());
                }
            }
            require(
                p.as_object().is_some_and(|fields| {
                    fields.keys().all(|key| {
                        ["name", "scope", "data", "identity"].contains(&key.as_str())
                            || reference_fields(family).contains(&key.as_str())
                    })
                }),
                "配置参数包含未知字段；引用只能通过声明的引用字段提供",
            )?;
            validate_source_payload(s, family, &refs, &data, &scope, old.as_ref())?;
            let definition_id = old
                .as_ref()
                .map(|r| r.definition_id.clone())
                .unwrap_or_else(id);
            let version = s
                .governance
                .iter()
                .filter(|r| r.r#type == family && r.definition_id == definition_id)
                .map(|r| r.version)
                .max()
                .unwrap_or(0)
                + 1;
            let name = opt(p, "name")
                .or_else(|| old.as_ref().map(|r| r.name.clone()))
                .ok_or_else(|| err("invalid_params", "需要名称"))?;
            let r = record_with_refs(
                family,
                definition_id,
                scope,
                name,
                version,
                old.map(|r| r.id),
                refs,
                data,
                true,
            );
            if family == "agent" {
                let mut view = s.clone();
                view.governance.push(r.clone());
                install_agent_profile(s, &r, &view.resolve_agent(&r.id)?);
            }
            if family == "delivery_type" {
                r.decode::<DeliveryTypeDefinition>()?.validate()?;
            }
            if family == "skill" {
                if let Some(previous) = r.supersedes.as_ref().and_then(|key| {
                    s.governance
                        .iter()
                        .find(|p| p.r#type == "skill_package" && p.definition_id == *key)
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
            require(
                !["spec", "standard"].contains(&family),
                "规范配置已过时，请使用当前结构重建",
            )?;
            if family == "skill" {
                let body = text(&r.data, "body")?;
                s.definitions.push(Definition {
                    id: r.id.clone(),
                    family: family.into(),
                    name: r.name.clone(),
                    version: r.version,
                    supersedes: r.supersedes.clone(),
                    body,

                    agent_profile: None,
                });
            }

            let result = json!(r);
            s.governance.push(r.clone());

            return Ok(Some(result));
        }
        let result = match method {
            "role.delete" => {
                let role = s.configuration(target)?.clone();
                if self.store.project_id().is_some() {
                    check_scope(s, &role.scope, self.store.project_id().as_deref())?;
                }
                require(
                    role.r#type == "role" && c.expected_revision == Some(role.revision),
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
                if !(source.r#type == "role" && self.store.project_id().is_none()) {
                    check_scope(s, &source.scope, self.store.project_id().as_deref())?;
                }
                require(source.is_definition(), "只能停用定义修订")?;
                crate::configuration::ensure_unreferenced(s, &source)?;
                require(source.r#type != "role", "请使用 role.delete 删除角色")?;
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
            "agent.instance.rebind" | "agent.instance.remove" => {
                let old = s.configuration(target)?.clone();
                require(
                    old.r#type == "agent_instance" && c.expected_revision == Some(old.revision),
                    "实例修订冲突",
                )?;
                require(
                    old.scope
                        == self
                            .store
                            .project_id()
                            .map_or(Scope::global(), Scope::project),
                    "实例不能跨配置库修改",
                )?;
                require(
                    !crate::configuration::retired(s, &old)
                        && !s.governance.iter().any(|r| {
                            r.r#type == "agent_instance"
                                && r.definition_id == old.definition_id
                                && r.version > old.version
                        }),
                    "实例已换绑或移除",
                )?;
                if method.ends_with("remove") {
                    let mut retired = record(
                        "agent_instance_retirement",
                        old.definition_id.clone(),
                        old.scope.clone(),
                        old.name.clone(),
                        1,
                        None,
                        json!({"instance_ref":old.id}),
                        true,
                    );
                    retired.id = format!("instance-retired:{}", old.definition_id);
                    s.governance.push(retired);
                    json!({"id":old.id,"removed":true})
                } else {
                    let key = text(p, "agent_revision_id")?;
                    reference(s, &key, "agent", &old.scope)?;
                    let mut next = record(
                        "agent_instance",
                        old.definition_id.clone(),
                        old.scope.clone(),
                        p["name"].as_str().unwrap_or(&old.name).into(),
                        old.version + 1,
                        Some(old.id.clone()),
                        json!(AgentInstance {
                            agent_revision_id: key,
                            scope: old.scope.clone(),
                            source_instance_id: Some(old.id)
                        }),
                        true,
                    );
                    next.id = format!("{}:{}", next.definition_id, next.version);
                    let result = json!(next);
                    s.governance.push(next);
                    result
                }
            }
            "agent.instance.create" => {
                let scope: Scope = decode(&p["scope"])?;
                check_scope(s, &scope, self.store.project_id().as_deref())?;
                require(!scope.is_feature(), "Agent 实例只属于全局或项目")?;
                let key = text(p, "agent_revision_id")?;
                let agent = s.configuration(&key)?;
                require(
                    agent.r#type == "agent" && agent.immutable,
                    "实例需要确切 Agent 修订",
                )?;
                if let Some(controller) = p["frozen_controller_id"].as_str() {
                    let row = s.configuration(controller)?;
                    require(
                        row.r#type == "workflow_controller"
                            && row.scope.project_id() == scope.project_id()
                            && row.data["agent_revision_id"] == key,
                        "Agent 不属于指定编排控制器",
                    )?;
                } else if let Some(work) = p["frozen_work_id"]
                    .as_str()
                    .or_else(|| p["frozen_feature_id"].as_str())
                {
                    let selected_work = work_reference(s, work)?;
                    require(
                        selected_work.scope(s)?.project_id() == scope.project_id(),
                        "冻结工作范围不匹配",
                    )?;
                    require(
                        s.governance.iter().any(|r| {
                            r.r#type == "workflow_instance"
                                && r.data["owner_id"] == work
                                && crate::standards::is_work_root(s, r)
                                && r.scope.project_id() == scope.project_id()
                                && r.data["plan"]["definitions"][&key]["content_hash"]
                                    == agent.content_hash
                        }),
                        "Agent 不属于指定冻结执行",
                    )?;
                } else {
                    reference(s, &key, "agent", &scope)?;
                }
                let mut row = record(
                    "agent_instance",
                    format!("agent-instance.{}", c.command_id),
                    scope.clone(),
                    text(p, "name")?,
                    1,
                    None,
                    json!(AgentInstance {
                        agent_revision_id: key,
                        scope,
                        source_instance_id: opt(p, "source_instance_id")
                    }),
                    true,
                );
                row.id = format!("{}:{}", row.definition_id, row.version);
                let result = json!(row);
                s.governance.push(row);
                result
            }
            "project.configuration.update" => {
                return Err(err("invalid_configuration", "项目配置需要当前 Workflow v6"));
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
                        r.r#type == "role"
                            && (p["include_archived"] == true || template_record_current(&s, r))
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
            if ["query.workflow", "query.workflow_draft"].contains(&method) {
                return Ok(json!(s.configuration(&text(p, "id")?)?));
            }
            if method == "query.workflow_version_diff" {
                let left = s.configuration(&text(p, "left")?)?;
                let right = s.configuration(&text(p, "right")?)?;
                return Ok(
                    json!({"left":left,"right":right,"changed":left.content_hash!=right.content_hash}),
                );
            }
            if method == "query.project_configuration" {
                return Ok({
                    let configuration = s
                        .governance
                        .iter()
                        .filter(|r| {
                            r.r#type == "project_configuration"
                                && p["project_id"]
                                    .as_str()
                                    .is_none_or(|pid| r.definition_id == pid)
                        })
                        .max_by_key(|r| r.version);
                    let pinned = crate::templates::pinned_template_records(
                        &s,
                        configuration.map(|r| &r.data),
                    );
                    json!({"configuration":configuration,"pinned_records":pinned,"uses_archived_templates":pinned.iter().any(|r|!template_record_current(&s,r)),"standards":s.definitions.iter().filter(|d|d.family=="standard").collect::<Vec<_>>()})
                });
            }
            let rows: Vec<_> = s
                .governance
                .iter()
                .filter(|r| p["include_archived"] == true || template_record_current(&s, r))
                .filter(|r| crate::configuration::visible(&s, r, p["project_id"].as_str()))
                .rev()
                .filter(|r| p["include_deleted"] == true || !crate::configuration::retired(&s, r))
                .filter(|r| {
                    p["include_history"] == true
                        || !s.governance.iter().any(|n| {
                            n.r#type == r.r#type
                                && n.definition_id == r.definition_id
                                && n.version > r.version
                                && (p["include_archived"] == true || template_record_current(&s, n))
                        })
                })
                .filter(|r| match method {
                    "query.workflows" => {
                        ["workflow", "workflow_draft"].contains(&r.r#type.as_str())
                    }
                    "query.agent_instances" => r.r#type == "agent_instance",
                    "query.project_workflow_summary" => {
                        r.r#type == "workflow_instance" && r.data["parent_execution_id"].is_null()
                    }
                    _ => p["kind"].as_str().map_or(
                        r.is_definition() || r.r#type == "agent_instance",
                        |kind| {
                            r.r#type == kind
                                && (p["include_archived"] == true || template_record_current(&s, r))
                        },
                    ),
                })
                .filter(|r| {
                    p.get("scope")
                        .is_none_or(|scope| serde_json::to_value(&r.scope).unwrap() == *scope)
                })
                .collect();
            Ok(
                json!({"records":rows.iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":rows.len()}),
            )
        })())
    }
}
