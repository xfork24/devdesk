//! Generic work definitions. Business identifiers never select an executor.
use crate::governance::{record, reference, update_record};
use crate::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfiguredWorkflowPlan {
    pub workflow_revision_id: Id,
    pub document: WorkflowDefinition,
    pub roles: BTreeMap<Id, Value>,
    pub definitions: BTreeMap<Id, ConfigurationRecord>,
    pub permission_locks: BTreeMap<Id, VersionedPermissionLock>,
    pub tool_bindings: BTreeMap<Id, BoundTool>,
    pub integration_locks: BTreeMap<Id, IntegrationLock>,
    pub specification_snapshot: Vec<Value>,
}

impl Engine {
    pub fn project_configuration_status(&self) -> Result<Value> {
        let state = self.store.snapshot()?;
        let configuration = state
            .governance
            .iter()
            .filter(|r| r.kind == "project_configuration")
            .max_by_key(|r| r.version);
        let result = (|| {
            let configuration = configuration
                .ok_or_else(|| err("configuration_required", "请选择 Workflow 并配置角色 Agent"))?;
            let workflow = reference(
                &state,
                configuration.data["workflow_ref"].as_str().unwrap_or(""),
                "workflow",
                &configuration.scope,
            )?;
            let bindings: BTreeMap<Id, Id> =
                serde_json::from_value(configuration.data["role_agent_bindings"].clone())
                    .map_err(|e| err("invalid_configuration", e.to_string()))?;
            let plan = self.compile_configured_workflow(
                &state,
                workflow,
                &configuration.scope,
                Some(&bindings),
            )?;
            if let Some(id) = configuration.data["standard_ref"].as_str() {
                self.check_configured_baseline(&state, &plan, id, &configuration.scope)?;
            }
            Ok::<_, Error>(())
        })();
        Ok(match result {
            Ok(()) => json!({"ready":true,"issues":[]}),
            Err(error) => json!({"ready":false,"issues":[error.message]}),
        })
    }

    pub fn compile_configured_workflow(
        &self,
        state: &EngineeringState,
        workflow: &ConfigurationRecord,
        scope: &Scope,
        bindings: Option<&BTreeMap<Id, Id>>,
    ) -> Result<ConfiguredWorkflowPlan> {
        fn visit(
            engine: &Engine,
            state: &EngineeringState,
            workflow: &ConfigurationRecord,
            scope: &Scope,
            bindings: Option<&BTreeMap<Id, Id>>,
            stack: &mut Vec<Id>,
            count: &mut usize,
            plan: &mut ConfiguredWorkflowPlan,
            path: &str,
        ) -> Result<()> {
            require(!stack.contains(&workflow.id), "子流程引用存在环")?;
            require(stack.len() < WORKFLOW_DEPTH_LIMIT, "子流程深度超过 8")?;
            stack.push(workflow.id.clone());
            let doc: WorkflowDefinition = workflow.decode()?;
            doc.validate()?;
            *count += doc.steps.len();
            require(*count <= WORKFLOW_STEP_LIMIT, "展开流程超过 1000 个步骤")?;
            plan.definitions
                .insert(workflow.id.clone(), workflow.clone());
            for step in &doc.steps {
                let location = format!("{path}/{}", step.key);
                if let Some(role) = doc.role(step) {
                    let record = crate::configuration::role_record(state, role)?;
                    require(
                        crate::configuration::visible(state, record, scope.project())
                            && !crate::configuration::retired(state, record),
                        "Role 不适用于当前范围",
                    )?;
                    plan.roles.insert(record.definition_id.clone(),json!({"role_ref":record.definition_id,"role_id":record.definition_id,"name":record.name,"role":record}));
                    plan.definitions.insert(record.id.clone(), record.clone());
                    if let Some(bindings) = bindings {
                        let agent = bindings.get(&record.definition_id).ok_or_else(|| {
                            err(
                                "configuration_required",
                                format!("Role {} 未绑定 Agent", record.name),
                            )
                        })?;
                        let agent = reference(state, agent, "agent", scope)?;
                        let composition: AgentComposition = agent.decode()?;
                        let selected = crate::configuration::role_record(
                            state,
                            composition.role_ref.as_deref().unwrap_or(""),
                        )?;
                        require(
                            selected.definition_id == record.definition_id,
                            "Agent 必须属于指定 Role 身份",
                        )?;
                        crate::governance::validate_composition(state, &composition, scope)?;
                        let selection =
                            composition.permission_selection.as_ref().ok_or_else(|| {
                                err("permission_unconfigured", "Agent 需要明确 Permission")
                            })?;
                        plan.permission_locks.insert(
                            agent.id.clone(),
                            resolve_permission_lock(state, &composition.harness_ref, selection)?,
                        );
                        plan.integration_locks.insert(
                            agent.id.clone(),
                            crate::integrations::freeze_integrations(
                                state,
                                &composition,
                                scope.project(),
                            )?,
                        );
                        require(
                            step.required_skill_refs
                                .iter()
                                .all(|id| composition.skills.contains(id)),
                            format!("{location}: Agent 缺少必需 Skill"),
                        )?;
                        require(
                            step.access
                                .tool_refs
                                .iter()
                                .all(|id| composition.tools.contains(id)),
                            format!("{location}: Step Tool 不属于 Agent 能力集合"),
                        )?;
                        for id in std::iter::once(&agent.id)
                            .chain(composition.role_ref.iter())
                            .chain(composition.capability_ref.iter())
                            .chain(std::iter::once(&composition.model_ref))
                            .chain(std::iter::once(&composition.harness_ref))
                            .chain(composition.skills.iter())
                            .chain(composition.tools.iter())
                        {
                            if let Ok(definition) = state.configuration(id) {
                                plan.definitions.insert(id.clone(), definition.clone());
                            }
                        }
                        for tool in &composition.tools {
                            let definition = reference(state, tool, "tool", scope)?;
                            if definition.data["implementation"] != "platform" {
                                continue;
                            }
                            let definition: ToolDefinition = definition.decode()?;
                            let validator = engine
                                .tool_executor
                                .as_ref()
                                .ok_or_else(|| err("unsupported", "缺少结构校验器"))?;
                            validator.validate_schema(&definition.input_schema)?;
                            validator.validate_schema(&definition.output_schema)?;
                            if let Some(runtime) = definition.runtime {
                                let harness =
                                    reference(state, &runtime.harness_ref, "harness", scope)?;
                                plan.definitions.insert(harness.id.clone(), harness.clone());
                                plan.permission_locks.insert(
                                    tool.clone(),
                                    resolve_permission_lock(
                                        state,
                                        &runtime.harness_ref,
                                        &runtime.permission_selection,
                                    )?,
                                );
                            } else {
                                require(
                                    definition.qualified_name != "tests.run",
                                    "测试工具必须选择冻结运行环境",
                                )?;
                            }
                        }
                    }
                }
                for skill in &step.required_skill_refs {
                    let definition = reference(state, skill, "skill", scope)?;
                    plan.definitions
                        .insert(definition.id.clone(), definition.clone());
                }
                for (name, port) in step.outputs.iter().chain(&doc.inputs) {
                    let output = reference(state, &port.delivery_ref, "delivery_type", scope)?;
                    let definition: DeliveryTypeDefinition = output.decode()?;
                    definition.validate()?;
                    if let Some(schema) = &definition.content_schema {
                        engine
                            .tool_executor
                            .as_ref()
                            .ok_or_else(|| err("unsupported", "缺少结构校验器"))?
                            .validate_schema(schema)?;
                    }
                    require(!name.is_empty(), "端口名称不能为空")?;
                    plan.definitions.insert(output.id.clone(), output.clone());
                }
                for spec in &step.spec_refs {
                    let definition = reference(state, spec, "spec", scope)?;
                    require(
                        definition.data["body"]
                            .as_str()
                            .is_some_and(|s| !s.trim().is_empty()),
                        "Spec 正文不能为空",
                    )?;
                    if definition.data["schema_version"] == 3 {
                        let spec: ConfiguredSpecification = definition.decode()?;
                        spec.validate()?;
                        let deliveries: Vec<_> = step
                            .outputs
                            .values()
                            .map(|p| p.delivery_ref.as_str())
                            .collect();
                        require(
                            spec.applicable_to.is_empty()
                                || spec.applicable_to.iter().any(|selector| {
                                    selector.matches(
                                        doc.role(step)
                                            .and_then(|id| {
                                                crate::configuration::role_record(state, id).ok()
                                            })
                                            .map(|role| role.definition_id.as_str()),
                                        &deliveries,
                                        &step.tags,
                                    )
                                }),
                            format!("{location}: Spec {} 不适用", definition.name),
                        )?;
                    }
                    plan.definitions
                        .insert(definition.id.clone(), definition.clone());
                }
                if step.outputs.values().any(|p| p.content_checks) {
                    let mut keys = std::collections::BTreeSet::new();
                    for id in &step.spec_refs {
                        let spec: ConfiguredSpecification = plan.definitions[id].decode()?;
                        spec.validate()?;
                        for check in spec.checks {
                            require(
                                keys.insert(check.key),
                                "同一步骤的检查标识重复，请调整 Spec",
                            )?;
                        }
                    }
                    require(!keys.is_empty(), "内容判断输出需要 Spec 检查项")?;
                }
                plan.specification_snapshot.push(json!({"path":location,"step":step.key,"role_ref":doc.role(step).and_then(|id|crate::configuration::role_record(state,id).ok()).map(|role|&role.definition_id),
                    "spec_refs":step.spec_refs,"outputs":step.outputs,"completion":step.completion,"tags":step.tags}));
                match &step.executor {
                    StepExecutor::Tool { tool_ref, .. } => {
                        let tool = reference(state, tool_ref, "tool", scope)?;
                        require(
                            tool.data["schema_version"] == 2
                                || tool.data["implementation"] == "platform",
                            "工具 Step 需要可调用工具，原生工具组不能独立调用",
                        )?;
                        plan.definitions.insert(tool.id.clone(), tool.clone());
                        if tool.data["implementation"] == "platform" {
                            let definition: ToolDefinition = tool.decode()?;
                            require(
                                crate::platform_tools::PLATFORM_HANDLERS
                                    .contains(&definition.qualified_name.as_str()),
                                "平台工具处理器不受支持",
                            )?;
                            let validator = engine
                                .tool_executor
                                .as_ref()
                                .ok_or_else(|| err("unsupported", "缺少结构校验器"))?;
                            validator.validate_schema(&definition.input_schema)?;
                            validator.validate_schema(&definition.output_schema)?;
                            if let Some(runtime) = definition.runtime {
                                let harness =
                                    reference(state, &runtime.harness_ref, "harness", scope)?;
                                plan.definitions.insert(harness.id.clone(), harness.clone());
                                let lock = resolve_permission_lock(
                                    state,
                                    &runtime.harness_ref,
                                    &runtime.permission_selection,
                                )?;
                                plan.permission_locks.insert(tool.id.clone(), lock);
                            }
                            require(
                                definition.qualified_name != "tests.run"
                                    || tool.data["runtime"].is_object(),
                                "测试工具必须选择冻结运行环境",
                            )?;
                        }
                        if tool.data["schema_version"] == 2 {
                            let definition: ExternalTool = tool.decode()?;
                            definition.validate()?;
                            let integration = reference(
                                state,
                                &definition.integration_ref,
                                "integration",
                                scope,
                            )?;
                            plan.definitions
                                .insert(integration.id.clone(), integration.clone());
                            if bindings.is_some() {
                                let project = scope.project().ok_or_else(|| {
                                    err("project_required", "工具 Step 需要明确项目绑定")
                                })?;
                                let configuration = state
                                    .governance
                                    .iter()
                                    .filter(|r| {
                                        r.kind == "integration_project_configuration"
                                            && r.definition_id == project
                                    })
                                    .max_by_key(|r| r.version)
                                    .ok_or_else(|| {
                                        err("integration_binding_required", "请配置项目接入绑定")
                                    })?;
                                let binding_id = configuration.data[&integration.id]
                                    .as_str()
                                    .ok_or_else(|| {
                                        err("integration_binding_required", "工具缺少项目接入绑定")
                                    })?;
                                let binding =
                                    reference(state, binding_id, "integration_binding", scope)?;
                                let bound: IntegrationBinding = binding.decode()?;
                                require(
                                    bound.integration_ref == integration.id
                                        && bound.allowed_tools.contains(&tool.id),
                                    "接入绑定未授权此工具",
                                )?;
                                let selection = AgentPermissionSelection {
                                    sandbox_ref: bound.sandbox_ref.clone(),
                                    harness: PermissionSelection::FollowTarget,
                                    sandbox: PermissionSelection::FollowTarget,
                                };
                                plan.permission_locks.insert(
                                    tool.id.clone(),
                                    resolve_permission_lock(state, &bound.harness_ref, &selection)?,
                                );
                                plan.tool_bindings.insert(
                                    tool.id.clone(),
                                    BoundTool {
                                        revision_id: tool.id.clone(),
                                        content_hash: tool.content_hash.clone(),
                                        definition,
                                        integration: integration.decode()?,
                                        integration_hash: integration.content_hash.clone(),
                                        binding_ref: binding.id.clone(),
                                        binding: bound,
                                        binding_hash: binding.content_hash.clone(),
                                    },
                                );
                                plan.definitions.insert(binding.id.clone(), binding.clone());
                            }
                        }
                    }
                    StepExecutor::Workflow {
                        workflow_ref,
                        foreach,
                    } => {
                        let child = reference(state, workflow_ref, "workflow", scope)?;
                        let child_doc: WorkflowDefinition = child.decode()?;
                        for (name, port) in &child_doc.inputs {
                            if port.required {
                                require(
                                    step.inputs.contains_key(name)
                                        || foreach.as_ref().is_some_and(|c| c.item_input == *name),
                                    format!("{location}: 子流程缺少输入 {name}"),
                                )?;
                            }
                            if let Some(InputBinding::Source { source }) = step.inputs.get(name) {
                                let source_port = doc.source(source, &doc.ancestors(step))?;
                                require(
                                    source_port.delivery_ref == port.delivery_ref,
                                    "子流程输入类型修订不匹配",
                                )?;
                            }
                        }
                        for (name, port) in &step.outputs {
                            let source = child_doc.outputs.get(name).ok_or_else(|| {
                                err(
                                    "invalid_workflow",
                                    format!("{location}: 子流程未声明输出 {name}"),
                                )
                            })?;
                            let keys = child_doc.steps.iter().map(|s| s.key.as_str()).collect();
                            let child_type = child_doc.source(source, &keys)?;
                            require(
                                foreach.is_some() || child_type.delivery_ref == port.delivery_ref,
                                "子流程输出类型修订不匹配",
                            )?;
                        }
                        visit(
                            engine, state, child, scope, bindings, stack, count, plan, &location,
                        )?;
                    }
                    _ => {}
                }
            }
            stack.pop();
            Ok(())
        }
        let document: WorkflowDefinition = workflow.decode()?;
        let mut plan = ConfiguredWorkflowPlan {
            workflow_revision_id: workflow.id.clone(),
            document,
            roles: BTreeMap::new(),
            definitions: BTreeMap::new(),
            permission_locks: BTreeMap::new(),
            tool_bindings: BTreeMap::new(),
            integration_locks: BTreeMap::new(),
            specification_snapshot: vec![],
        };
        visit(
            self,
            state,
            workflow,
            scope,
            bindings,
            &mut vec![],
            &mut 0,
            &mut plan,
            "",
        )?;
        Ok(plan)
    }

    pub(crate) fn configured_workflow_command(
        &self,
        method: &str,
        command: &Command,
        state: &mut EngineeringState,
    ) -> Result<Option<Value>> {
        let p = &command.payload;
        let document = p.get("document");
        let selected = [
            command.target.as_deref(),
            p["base_revision_id"].as_str(),
            p["template_ref"].as_str(),
            p["workflow_ref"].as_str(),
        ]
        .into_iter()
        .flatten()
        .find_map(|id| state.configuration(id).ok())
        .cloned();
        let family = method.split('.').next().unwrap_or("");
        if ["spec", "standard"].contains(&family)
            && ["create", "revise"].contains(&method.split('.').nth(1).unwrap_or(""))
            && p["data"]["schema_version"] == 3
        {
            let old = if method.ends_with("revise") {
                Some(
                    selected
                        .clone()
                        .ok_or_else(|| err("not_found", "定义不存在"))?,
                )
            } else {
                None
            };
            if let Some(old) = &old {
                require(
                    old.kind == family && command.expected_revision == Some(old.revision),
                    "配置修订冲突",
                )?;
            }
            let scope: Scope = old
                .as_ref()
                .map(|r| Ok(r.scope.clone()))
                .unwrap_or_else(|| {
                    serde_json::from_value(
                        p.get("scope").cloned().unwrap_or(json!({"kind":"global"})),
                    )
                    .map_err(|e| err("invalid_params", e.to_string()))
                })?;
            self.check_configured_workflow_scope(&scope)?;
            if family == "spec" {
                serde_json::from_value::<ConfiguredSpecification>(p["data"].clone())
                    .map_err(|e| err("invalid_configuration", e.to_string()))?
                    .validate()?;
            } else {
                let definition: ConfiguredStandard = serde_json::from_value(p["data"].clone())
                    .map_err(|e| err("invalid_configuration", e.to_string()))?;
                definition.validate()?;
                for rule in &definition.baseline_rules {
                    for spec in &rule.required_spec_refs {
                        reference(state, spec, "spec", &scope)?;
                    }
                }
            }
            let row = record(
                family,
                old.as_ref()
                    .map(|r| r.definition_id.clone())
                    .unwrap_or_else(id),
                scope,
                text(p, "name")?,
                old.as_ref().map_or(1, |r| r.version + 1),
                old.map(|r| r.id),
                p["data"].clone(),
                true,
            );
            let result = json!(row);
            state.governance.push(row);
            return Ok(Some(result));
        }
        let is_current = document.is_some_and(|doc| doc["schema_version"] == 4)
            || selected.as_ref().is_some_and(|r| {
                r.data["schema_version"] == 4 || r.data["document"]["schema_version"] == 4
            });
        if !is_current {
            return Ok(None);
        }
        match method {
            "project.configuration.update" => {
                let project = command
                    .target
                    .as_deref()
                    .and_then(|id| state.projects.iter().find(|p| p.id == id))
                    .ok_or_else(|| err("not_found", "Project 不存在"))?
                    .clone();
                require(
                    command.expected_revision == Some(project.revision),
                    "项目修订冲突",
                )?;
                let scope = Scope::Project {
                    project_id: project.id.clone(),
                };
                let workflow = selected.unwrap();
                let bindings: BTreeMap<Id, Id> =
                    serde_json::from_value(p["role_agent_bindings"].clone())
                        .map_err(|e| err("invalid_params", e.to_string()))?;
                let plan =
                    self.compile_configured_workflow(state, &workflow, &scope, Some(&bindings))?;
                let baseline = if let Some(id) = p["standard_ref"].as_str() {
                    Some(self.check_configured_baseline(state, &plan, id, &scope)?)
                } else {
                    None
                };
                let old = state
                    .governance
                    .iter()
                    .filter(|r| r.kind == "project_configuration" && r.definition_id == project.id)
                    .max_by_key(|r| r.version)
                    .cloned();
                let row = record(
                    "project_configuration",
                    project.id.clone(),
                    scope,
                    project.title,
                    old.as_ref().map_or(1, |r| r.version + 1),
                    old.map(|r| r.id),
                    json!({"schema_version":3,"workflow_ref":workflow.id,"role_agent_bindings":bindings,"standard_ref":p["standard_ref"],
                    "specification_snapshot":plan.specification_snapshot,"baseline_check":baseline}),
                    true,
                );
                let result = json!(row);
                state.governance.push(row);
                state
                    .projects
                    .iter_mut()
                    .find(|p| p.id == project.id)
                    .unwrap()
                    .revision += 1;
                Ok(Some(result))
            }
            "workflow.create" | "workflow.draft.create" => {
                let scope: Scope =
                    serde_json::from_value(p.get("scope").cloned().unwrap_or_else(|| {
                        selected
                            .as_ref()
                            .map(|r| json!(r.scope))
                            .unwrap_or(json!({"kind":"global"}))
                    }))
                    .map_err(|e| err("invalid_params", e.to_string()))?;
                self.check_configured_workflow_scope(&scope)?;
                if let Some(source) = &selected {
                    require(source.scope.visible_from(&scope), "流程来源不可见")?;
                }
                let doc = document
                    .cloned()
                    .or_else(|| selected.as_ref().map(|r| r.data.clone()))
                    .ok_or_else(|| err("invalid_params", "缺少流程正文"))?;
                let definition: WorkflowDefinition = serde_json::from_value(doc.clone())
                    .map_err(|e| err("invalid_workflow", e.to_string()))?;
                require(definition.schema_version == 4, "草稿需要 Workflow v4")?;
                let identity = if method == "workflow.draft.create" {
                    selected
                        .as_ref()
                        .filter(|r| r.scope == scope)
                        .map(|r| r.definition_id.clone())
                        .unwrap_or_else(id)
                } else {
                    id()
                };
                let name = opt(p, "name")
                    .or_else(|| selected.as_ref().map(|r| r.name.clone()))
                    .ok_or_else(|| err("invalid_params", "缺少名称"))?;
                let row = record(
                    "workflow_draft",
                    identity,
                    scope,
                    name,
                    1,
                    None,
                    json!({"document":doc,"base_revision_id":selected.as_ref().map(|r|&r.id)}),
                    false,
                );
                let result = json!(row);
                state.governance.push(row);
                Ok(Some(result))
            }
            "workflow.draft.update" => {
                let old = selected.unwrap();
                self.check_configured_workflow_scope(&old.scope)?;
                require(
                    old.kind == "workflow_draft"
                        && old.data["published_revision_id"].is_null()
                        && command.expected_revision == Some(old.revision),
                    "草稿修订冲突",
                )?;
                let doc = document.cloned().unwrap_or(old.data["document"].clone());
                let definition: WorkflowDefinition = serde_json::from_value(doc.clone())
                    .map_err(|e| err("invalid_workflow", e.to_string()))?;
                require(definition.schema_version == 4, "草稿需要 Workflow v4")?;
                let row = state
                    .governance
                    .iter_mut()
                    .find(|r| r.id == old.id)
                    .unwrap();
                if let Some(name) = p["name"].as_str() {
                    row.name = name.into();
                }
                let mut data = old.data.clone();
                data["document"] = doc;
                update_record(row, data);
                Ok(Some(json!(row)))
            }
            "workflow.publish" => {
                let old = selected.unwrap();
                self.check_configured_workflow_scope(&old.scope)?;
                require(
                    old.kind == "workflow_draft"
                        && old.data["published_revision_id"].is_null()
                        && command.expected_revision == Some(old.revision),
                    "草稿修订冲突",
                )?;
                let mut candidate = old.clone();
                candidate.data = old.data["document"].clone();
                self.compile_configured_workflow(state, &candidate, &old.scope, None)?;
                require(
                    !state.governance.iter().any(|r| {
                        r.kind == "workflow"
                            && r.definition_id == old.definition_id
                            && old.data["base_revision_id"]
                                .as_str()
                                .is_some_and(|base| r.id != base)
                            && old.data["base_revision_id"]
                                .as_str()
                                .and_then(|base| state.configuration(base).ok())
                                .is_some_and(|base| r.version > base.version)
                    }),
                    "流程已有新修订，请重新建立草稿",
                )?;
                let version = state
                    .governance
                    .iter()
                    .filter(|r| r.kind == "workflow" && r.definition_id == old.definition_id)
                    .map(|r| r.version)
                    .max()
                    .unwrap_or(0)
                    + 1;
                let row = record(
                    "workflow",
                    old.definition_id.clone(),
                    old.scope.clone(),
                    old.name.clone(),
                    version,
                    old.data["base_revision_id"].as_str().map(str::to_owned),
                    candidate.data,
                    true,
                );
                let mut data = old.data.clone();
                data["published_revision_id"] = json!(row.id);
                update_record(
                    state
                        .governance
                        .iter_mut()
                        .find(|r| r.id == old.id)
                        .unwrap(),
                    data,
                );
                let result = json!(row);
                state.governance.push(row);
                Ok(Some(result))
            }
            _ => Ok(None),
        }
    }

    fn check_configured_workflow_scope(&self, scope: &Scope) -> Result<()> {
        match scope {
            Scope::Global => require(
                self.store.project_id().is_none(),
                "全局定义必须在全局目录维护",
            ),
            Scope::Project { project_id } => require(
                self.store.project_id().as_ref() == Some(project_id),
                "配置项目范围不匹配",
            ),
            Scope::Feature { .. } => Err(err("invalid_scope", "不允许创建可变 Feature 定义")),
        }
    }

    pub(crate) fn check_configured_baseline(
        &self,
        state: &EngineeringState,
        plan: &ConfiguredWorkflowPlan,
        id: &str,
        scope: &Scope,
    ) -> Result<Value> {
        let record = reference(state, id, "standard", scope)?;
        let standard: ConfiguredStandard = record.decode()?;
        standard.validate()?;
        for rule in &standard.baseline_rules {
            let matches: Vec<_> = plan
                .specification_snapshot
                .iter()
                .filter(|binding| {
                    let deliveries: Vec<_> = binding["outputs"]
                        .as_object()
                        .into_iter()
                        .flat_map(|o| o.values())
                        .filter_map(|v| v["delivery_ref"].as_str())
                        .collect();
                    let tags: Vec<String> = binding["tags"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect();
                    rule.selector
                        .matches(binding["role_ref"].as_str(), &deliveries, &tags)
                })
                .collect();
            require(
                matches.len() as u64 >= rule.min_matches,
                "基线规则没有足够的实际步骤落点",
            )?;
            for binding in matches {
                for spec in &rule.required_spec_refs {
                    require(
                        binding["spec_refs"]
                            .as_array()
                            .is_some_and(|v| v.contains(&json!(spec))),
                        format!("{} 缺少规范 {spec}", binding["path"]),
                    )?;
                }
            }
        }
        Ok(json!({"standard_ref":id,"valid":true,"errors":[]}))
    }

    pub(crate) fn configured_workflow_query(
        &self,
        method: &str,
        params: &Value,
    ) -> Option<Result<Value>> {
        if ![
            "query.workflow_requirements",
            "query.workflow_validation",
            "query.workflow_preview",
            "query.workflow_task_preview",
        ]
        .contains(&method)
        {
            return None;
        }
        let state = match self.store.snapshot() {
            Ok(state) => state,
            Err(error) => return Some(Err(error)),
        };
        let mut row = if let Some(document) = params.get("document") {
            record(
                "workflow",
                "preview".into(),
                Scope::Global,
                "任务预览".into(),
                1,
                None,
                document.clone(),
                true,
            )
        } else {
            match params["id"]
                .as_str()
                .and_then(|id| state.configuration(id).ok())
            {
                Some(row) => row.clone(),
                None => return None,
            }
        };
        if row.kind == "workflow_draft" {
            row.data = row.data["document"].clone();
        }
        if row.data["schema_version"] != 4 {
            return None;
        }
        Some((|| {
            let scope = params["project_id"]
                .as_str()
                .map(|id| Scope::Project {
                    project_id: id.into(),
                })
                .unwrap_or(Scope::Global);
            let bindings: Option<BTreeMap<Id, Id>> = params
                .get("role_agent_bindings")
                .map(|v| {
                    serde_json::from_value(v.clone())
                        .map_err(|e| err("invalid_params", e.to_string()))
                })
                .transpose()?;
            let mut plan =
                self.compile_configured_workflow(&state, &row, &scope, bindings.as_ref())?;
            if method == "query.workflow_task_preview" {
                let key = text(params, "step")?;
                let step = plan
                    .document
                    .steps
                    .iter()
                    .find(|s| s.key == key)
                    .ok_or_else(|| err("not_found", "步骤不存在"))?;
                require(
                    matches!(step.executor, StepExecutor::Agent { .. }),
                    "此步骤不调用 Agent",
                )?;
                let mut selected_agent = None;
                if let Some(agent) = params["preview_agent_ref"].as_str() {
                    let agent = reference(&state, agent, "agent", &scope)?;
                    let composition: AgentComposition = agent.decode()?;
                    let role = crate::configuration::role_record(
                        &state,
                        plan.document.role(step).unwrap(),
                    )?;
                    require(
                        crate::configuration::role_record(
                            &state,
                            composition.role_ref.as_deref().unwrap_or(""),
                        )?
                        .definition_id
                            == role.definition_id,
                        "预览 Agent 不属于此步骤的 Role",
                    )?;
                    require(
                        step.required_skill_refs
                            .iter()
                            .all(|id| composition.skills.contains(id)),
                        "预览 Agent 缺少所需 Skill",
                    )?;
                    for id in std::iter::once(&agent.id)
                        .chain(composition.role_ref.iter())
                        .chain(composition.skills.iter())
                    {
                        if let Ok(definition) = state.configuration(id) {
                            plan.definitions.insert(id.clone(), definition.clone());
                        }
                    }
                    selected_agent = Some(agent.id.clone());
                }
                let step = step.clone();
                let execution = StepExecution {
                    id: "preview".into(),
                    workflow_instance_id: "preview".into(),
                    workflow_revision_id: row.id.clone(),
                    path: format!("/{}", step.key),
                    step: step.key.clone(),
                    round: 1,
                    sequence: 1,
                    status: StepExecutionStatus::Queued,
                    input_digest: String::new(),
                    budget_wait_started: None,
                    budget_waited_ms: 0,
                    inputs: step
                        .inputs
                        .iter()
                        .map(|(key, binding)| {
                            (
                                key.clone(),
                                match binding {
                                    InputBinding::Constant { value } => value.clone(),
                                    InputBinding::Source { source } => {
                                        json!({"unresolved_source":source})
                                    }
                                },
                            )
                        })
                        .collect(),
                    outputs: BTreeMap::new(),
                    agent_revision_id: selected_agent,
                    session_id: None,
                    turn_id: None,
                    tool_call_id: None,
                    started: None,
                    finished: None,
                    reason: String::new(),
                };
                let instance = ConfiguredWorkflowInstance {
                    schema_version: 4,
                    owner_id: "preview".into(),
                    workflow_ref: row.id.clone(),
                    project_id: scope.project().unwrap_or("").into(),
                    plan,
                    role_agent_bindings: BTreeMap::new(),
                    inputs: BTreeMap::new(),
                    steps: vec![execution.clone()],
                    status: StepExecutionStatus::Queued,
                    paused: false,
                    parent_execution_id: None,
                    root_instance_id: "preview".into(),
                    cancel_requested: false,
                };
                let rendered =
                    crate::configured_runtime::task_input(&instance, &step, &execution, None)?;
                let content: Value = serde_json::from_str(&rendered)
                    .map_err(|e| err("serialization", e.to_string()))?;
                return Ok(
                    json!({"configured":true,"task":content,"rendered_input":rendered,"diagnostics":["预览使用与实际派发相同的任务组装器；未执行的输入显示明确来源，运行时替换为确切值。"]}),
                );
            }
            let baseline = params["standard_ref"].as_str().map(|id| {
                self.check_configured_baseline(&state, &plan, id, &scope)
                    .unwrap_or_else(
                        |e| json!({"standard_ref":id,"valid":false,"errors":[e.message]}),
                    )
            });
            Ok(
                json!({"valid":baseline.as_ref().is_none_or(|b|b["valid"]==true),"schema_version":4,"roles":plan.roles.values().collect::<Vec<_>>(),"specification_snapshot":plan.specification_snapshot,
                "baseline_check":baseline,"document":plan.document,"definition_hashes":plan.definitions.iter().map(|(id,r)|(id,&r.content_hash)).collect::<BTreeMap<_,_>>()}),
            )
        })())
    }
}
