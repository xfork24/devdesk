//! Durable execution of configuration-owned steps using the existing native ports.
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfiguredWorkflowInstance {
    pub active_plan_revision_id: Id,
    pub control_version: u64,
    pub schema_version: u64,
    pub owner_id: Id,
    pub workflow_ref: Id,
    pub project_id: Id,
    pub plan: ConfiguredWorkflowPlan,
    pub role_agent_bindings: BTreeMap<Id, Id>,
    pub inputs: BTreeMap<String, Value>,
    /// Original user request, frozen independently of typed Workflow input ports.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub user_request: Value,
    /// Remains enabled even if a later, human-approved plan removes a checkpoint.
    #[serde(default)]
    pub human_governance: bool,
    pub steps: Vec<StepExecution>,
    pub status: StepExecutionStatus,
    pub paused: bool,
    #[serde(default)]
    pub parent_execution_id: Option<Id>,
    pub root_instance_id: Id,
    #[serde(default)]
    pub cancel_requested: bool,
}

#[derive(Debug, Clone)]
pub struct ConfiguredToolRequest {
    pub authority: crate::tool_calls::WorkflowToolAuthority,
    pub provenance: crate::tool_calls::WorkflowToolProvenance,
    pub execution_id: Id,
    pub owner_id: Id,
    pub project_id: Id,
    pub tool: BoundTool,
    pub permissions: VersionedPermissionLock,
    pub arguments: Value,
}
#[derive(Debug, Clone)]
pub struct ConfiguredAgentRequest {
    pub integration_lock: IntegrationLock,
    pub access: StepAccess,
    pub permissions: VersionedPermissionLock,
}

pub(crate) fn checkpoint_workspace(execution: &StepExecution, value: &Value) -> Value {
    execution
        .rework_workspace
        .as_ref()
        .filter(|checkpoint| {
            ["workspace_id", "owner_id", "revision"].iter().all(|key| {
                value.get(key).is_some() && value.get(key) == checkpoint.previous_workspace.get(key)
            })
        })
        .map(|checkpoint| checkpoint.workspace.clone())
        .unwrap_or_else(|| value.clone())
}

pub(crate) fn frozen_tool_arguments(
    facts: &WorkflowFacts,
    bindings: &BTreeMap<String, InputBinding>,
    execution: &StepExecution,
) -> Result<Value> {
    let arguments = bindings
        .iter()
        .map(|(name, binding)| {
            let value = match binding {
                InputBinding::Constant { value } => value.clone(),
                InputBinding::Material { material_ref } => facts
                    .values
                    .get(&("$material".into(), material_ref.clone()))
                    .cloned()
                    .ok_or_else(|| err("missing_material", "工具材料不存在"))?,
                InputBinding::Source { source } => facts
                    .value(source)
                    .cloned()
                    .ok_or_else(|| err("input_unavailable", "工具参数来源不存在"))?,
            };
            Ok((
                name.clone(),
                if name == "workspace" {
                    checkpoint_workspace(execution, &value)
                } else {
                    value
                },
            ))
        })
        .collect::<Result<serde_json::Map<String, Value>>>()?;
    Ok(Value::Object(arguments))
}

pub(crate) fn material_inputs(
    step: &StepDefinition,
    facts: &WorkflowFacts,
) -> Result<BTreeMap<String, Value>> {
    step.inputs
        .iter()
        .map(|(name, binding)| {
            let value = match binding {
                InputBinding::Constant { value } => value.clone(),
                InputBinding::Material { material_ref } => facts
                    .values
                    .get(&("$material".into(), material_ref.clone()))
                    .cloned()
                    .ok_or_else(|| err("missing_material", "材料不存在"))?,
                InputBinding::Source { source } => facts
                    .value(source)
                    .cloned()
                    .ok_or_else(|| err("input_unavailable", format!("输入 {name} 尚未就绪")))?,
            };
            Ok((name.clone(), value))
        })
        .collect()
}

pub fn task_input(
    instance: &ConfiguredWorkflowInstance,
    step: &StepDefinition,
    execution: &StepExecution,
    supplemental: Option<&str>,
) -> Result<String> {
    let definitions = &instance.plan.definitions;
    let checks = crate::spec_lint::step_content_checks(instance, step)?;
    let port_schemas:BTreeMap<_,_>=step.outputs.iter().map(|(name,port)| {
        let definition:DeliveryTypeDefinition=definitions.get(&port.delivery_ref).ok_or_else(||err("configuration_required","交付类型不在配置闭包中"))?.decode()?;
        let mut constraints=vec![];
        if ["markdown","text","diff"].contains(&definition.format.as_str()) {constraints.push(json!({"type":"string","description":"Text content encoded as a JSON string; do not wrap it in a title/body object."}));}
        if let Some(schema)=definition.content_schema {constraints.push(schema);}
        if port.content_checks {
            constraints.push(json!({"type":"object","required":["approve","summary","checks"],"properties":{"approve":{"type":"boolean"},"summary":{"type":"string"},"checks":{"type":"array","minItems":checks.len(),"maxItems":checks.len(),"items":{"type":"object","required":["key","passed","evidence"],"properties":{"key":{"enum":checks.iter().map(|check|&check.key).collect::<Vec<_>>()},"passed":{"type":"boolean"},"evidence":{"type":"string","minLength":1},"locations":{"type":"array","maxItems":16,"items":EvidenceLocation::json_schema()}},"additionalProperties":false}}},"additionalProperties":false}));
        }
        Ok((name.clone(),match constraints.len(){0=>json!({}),1=>constraints.remove(0),_=>json!({"allOf":constraints})}))
    }).collect::<Result<_>>()?;
    let outputs = step.outputs.iter().map(|(name,port)|Ok((name.clone(),json!({"required":port.required,"schema":port_schemas[name],"content_checks":port.content_checks,
        "required_checks":if port.content_checks {json!(checks)}else{Value::Null},
        "check_report_contract":if port.content_checks {json!({"required":["approve","summary","checks"],"check_fields":["key","passed","evidence","locations"],"locations":{"requirement":"Required for evidence_required or machine assertions; optional for other semantic checks.","source_ref":"Copy the exact input_sources.<input>.evidence_source_ref value into this source_ref field.","value_digest":"Copy input_sources.<input>.value_digest.","locator":"Use the typed locator in the supplied schema: lines are 1-based inclusive, characters are 0-based with exclusive end; json_pointer addresses the frozen input value.","quote":EVIDENCE_QUOTE_INSTRUCTIONS},"approve":"true exactly when all declared checks pass"})}else{Value::Null},
        "definition":definitions.get(&port.delivery_ref).ok_or_else(||err("configuration_required","交付类型不在配置闭包中"))?}))))
        .collect::<Result<BTreeMap<_,_>>>()?;
    let mut input_sources:BTreeMap<_,_>=step.inputs.iter().map(|(name,binding)| {
        let source_output_id=match binding {InputBinding::Source{source} if source.step!="$input"=>instance.steps.iter().find(|step|step.step==source.step).and_then(|step|step.outputs.get(&source.port)),_=>None};
        (name.clone(),json!({"binding":match binding {InputBinding::Source{source}=>json!({"type":"source","source":source}),InputBinding::Constant{..}=>json!({"type":"constant"}),InputBinding::Material{material_ref}=>json!({"type":"material","material_ref":material_ref})},"source_output_id":source_output_id,"evidence_source_ref":format!("execution-input:{}:{}",execution.id,name),"value_digest":execution.inputs.get(name).map(configuration_hash)}))
    }).collect();
    for (name, source) in &mut input_sources {
        if let Some(runs) = execution.runtime_context["inputs"][name].get("runtime_executions") {
            source["runtime_executions"] = runs.clone();
        }
    }
    if let Some(checkpoint) = &execution.rework_workspace
        && let Some(name) = &step.access.workspace_input
        && execution.inputs.get(name) == Some(&checkpoint.workspace)
    {
        let source = input_sources.get_mut(name).unwrap();
        source["original_source_output_id"] = source["source_output_id"].clone();
        source["source_output_id"] = json!(checkpoint.output_id);
        source["rework_checkpoint"] = json!(checkpoint);
    }
    let mut rendered_inputs = execution.inputs.clone();
    for (name, value) in &execution.inputs {
        let bytes = serde_json::to_vec(value).map_err(|e| err("serialization", e.to_string()))?;
        if bytes.len() > 32768
            && let Some(id) = input_sources
                .get(name)
                .and_then(|source| source["source_output_id"].as_str())
        {
            let pointer = match &step.inputs[name] {
                _ if input_sources[name].get("rework_checkpoint").is_some() => "",
                InputBinding::Source { source } => source.pointer.as_str(),
                _ => "",
            };
            let reader = instance.plan.definitions.values().find(|r| {
                r.r#type == "tool"
                    && r.data["qualified_name"] == "workflow.outputs.read"
                    && step.access.tool_refs.contains(&r.id)
            });
            require(
                reader.is_some(),
                "大产物需要当前步骤授权的 workflow.outputs.read 完整读取能力",
            )?;
            rendered_inputs.insert(name.clone(), json!({"complete":false,"source_output_id":id,"pointer":pointer,"value_digest":configuration_hash(value),"total_bytes":bytes.len(),"read_tool":OUTPUT_READ_CALL_NAME,"arguments":{"ids":[id],"pointer":pointer,"offset":0,"max_bytes":OUTPUT_READ_PAGE_BYTES},"instructions":"Read every chunk through next_offset before judging this input. The reference is not the content."}));
        }
    }
    let input_deliveries = step.inputs.values().filter_map(|binding| match binding {
        InputBinding::Source { source } if source.step == "$input" => {
            instance.plan.document.inputs.get(&source.port)
        }
        InputBinding::Source { source } => instance
            .plan
            .document
            .steps
            .iter()
            .find(|step| step.key == source.step)
            .and_then(|step| step.outputs.get(&source.port)),
        _ => None,
    });
    let document_specs: BTreeSet<_> = step
        .outputs
        .values()
        .chain(input_deliveries)
        .filter_map(|port| definitions.get(&port.delivery_ref))
        .filter_map(|r| reference_list(&r.refs, "spec_refs").ok())
        .flatten()
        .filter(|reference| !step.spec_refs.contains(reference))
        .collect();
    let mut input = json!({"renderer_version":AGENT_TASK_RENDERER,"source":{"workflow_revision_id":instance.workflow_ref,"standard_ref":instance.plan.standard_ref,"work_configuration_ref":instance.plan.work_configuration_ref,"execution_id":execution.id,
        "workflow_instance_id":execution.workflow_instance_id,"plan_revision_id":execution.plan_revision_id,"node_id":execution.node_id,"contract_digest":execution.contract_digest,"path":execution.path,"step":step.key,"round":execution.round,"sequence":execution.sequence},
        "owner_id":instance.owner_id,"user_request":instance.user_request,
        "agent":execution.agent_revision_id.as_ref().and_then(|id|definitions.get(id)),
        "role":instance.plan.document.role(step).and_then(|id|instance.plan.roles.get(id).or_else(||instance.plan.roles.values().find(|role|role["role"]["id"]==id))),
        "stage":instance.plan.document.stages.iter().find(|s|s.key == step.stage),
        "task_instructions":step.task_instructions,"required_skills":step.required_skill_refs.iter().filter_map(|id|definitions.get(id)).collect::<Vec<_>>(),
        "specifications":step.spec_refs.iter().filter_map(|id|definitions.get(id)).collect::<Vec<_>>(),"document_specifications":document_specs.iter().filter_map(|id|definitions.get(id)).collect::<Vec<_>>(),"inputs":rendered_inputs,"input_sources":input_sources,
        "budget":step.budget,"authorization":{"workspace_input":step.access.workspace_input,"working_directory":step.access.working_directory,"writable_paths":step.access.writable_paths,"read_only_paths":step.access.read_only_paths,"tool_refs":step.access.tool_refs},"outputs":outputs,"completion":step.completion,"supplemental_input":supplemental,"rework":execution.rework,
        "output_contract":{"type":"object","required":["status","summary","outputs"],"additionalProperties":false,
            "description":"When status is completed, include every declared required output port. For needs_input or blocked, outputs may be empty.",
            "allOf":[{"if":{"properties":{"status":{"const":"completed"}}},"then":{"properties":{"outputs":{"required":step.outputs.iter().filter(|(_,port)|port.required).map(|(name,_)|name).collect::<Vec<_>>()}}}}],
            "properties":{"status":{"enum":["completed","needs_input","blocked"]},"summary":{"type":"string"},"outputs":{"type":"object","properties":port_schemas,"additionalProperties":false}},
            "instructions":"Use the supplied input values and provenance. Request missing information only when required by this step. Return one JSON object. Output keys must be declared ports. Values must match their declared delivery schemas. Platform receipts are references, never model-generated evidence."}});
    if execution.rework.is_none() {
        input.as_object_mut().unwrap().remove("rework");
    }
    if instance.user_request.is_null() {
        input.as_object_mut().unwrap().remove("user_request");
    }
    if instance.human_governance {
        input["human_governance"] = json!({
            "instructions":"Use the user's original request and confirmed upstream documents as the business authority. Decide whether clarification is necessary using the required Specs. If necessary facts are missing, or scope, acceptance criteria or a key design must change, return needs_input with concrete questions and wait for the human's answer. Never invent a human answer, approval or waiver. Keep confirmed business decisions unchanged until the human decides. Spec review and human confirmation are separate requirements."
        });
    }
    if !execution.runtime_context.is_null() {
        input["runtime_context"] = execution.runtime_context.clone();
    }
    if execution.runtime_context["technical_recovery"] == true {
        let contract = &mut input["output_contract"];
        contract["properties"]["status"]["enum"] =
            json!(["completed", "needs_input", "needs_manager", "blocked"]);
        contract["properties"]["recovery"] = json!({
            "type":"object","additionalProperties":false,
            "required":["diagnosis","attempted_actions"],
            "properties":{"diagnosis":{"type":"string","minLength":1,"maxLength":2048},
                "attempted_actions":{"type":"array","minItems":1,"maxItems":8,
                    "items":{"type":"string","minLength":1,"maxLength":512}}}
        });
        contract["allOf"].as_array_mut().unwrap().push(json!({
            "if":{"properties":{"status":{"const":"needs_manager"}}},
            "then":{"required":["recovery"]}
        }));
        contract["description"] = json!(
            "When completing, include all required output ports. For needs_input, needs_manager or blocked, outputs may be empty."
        );
        contract["instructions"] = json!(
            "Use the supplied inputs and provenance. Diagnose technical problems and try safe repairs within the assigned permissions before escalating. If unresolved, return needs_manager with the diagnosis, actions actually attempted and a specific suggested next step in the summary. Use needs_input only for missing business information the human must decide. Preserve frozen requirements and approvals. Return one JSON object whose outputs match declared ports. Never manufacture platform evidence."
        );
    }
    let input = serde_json::to_string(&input).map_err(|e| err("serialization", e.to_string()))?;
    require(
        input.len() <= TASK_PAYLOAD_LIMIT,
        "步骤任务及输入超过 512 KiB",
    )?;
    Ok(input)
}

pub(crate) fn mapped_tool_outputs(
    step: &StepDefinition,
    value: &Value,
) -> Result<serde_json::Map<String, Value>> {
    let StepExecutor::Tool { output_mapping, .. } = &step.executor else {
        return Err(err("invalid_workflow", "输出映射仅适用于工具"));
    };
    step.outputs
        .keys()
        .map(|name| {
            let pointer = output_mapping.get(name).map(String::as_str).unwrap_or("");
            let value = if pointer.is_empty() {
                Some(value)
            } else {
                value.pointer(pointer)
            }
            .ok_or_else(|| {
                err(
                    "output_invalid",
                    format!("工具结果缺少端口 {name} 的映射 {pointer}"),
                )
            })?;
            Ok((name.clone(), value.clone()))
        })
        .collect()
}

impl Engine {
    pub(crate) fn validate_workflow_inputs(
        &self,
        state: &EngineeringState,
        plan: &ConfiguredWorkflowPlan,
        inputs: &BTreeMap<String, Value>,
    ) -> Result<()> {
        for (name, port) in &plan.document.inputs {
            if port.required {
                require(inputs.contains_key(name), format!("缺少流程输入 {name}"))?;
            }
            if let Some(value) = inputs.get(name) {
                self.enforce_delivery_value(
                    state,
                    plan.definitions
                        .get(&port.delivery_ref)
                        .ok_or_else(|| err("configuration_required", "输入类型不在闭包中"))?,
                    value,
                )?;
            }
        }
        require(
            inputs
                .keys()
                .all(|key| plan.document.inputs.contains_key(key)),
            "流程包含未声明输入",
        )
    }

    pub fn configured_agent_request(
        &self,
        command: &Command,
    ) -> Result<Option<ConfiguredAgentRequest>> {
        self.prepare_formal_session(command)
            .map(|prepared| prepared.map(|prepared| prepared.request))
    }

    pub(crate) fn validate_configured_value(
        &self,
        definition: &DeliveryTypeDefinition,
        value: &Value,
    ) -> Result<()> {
        if ["markdown", "text", "diff"].contains(&definition.format.as_str()) {
            require(value.is_string(), "文本交付物需要字符串内容")?;
        }
        if let Some(schema) = &definition.content_schema {
            self.tool_executor
                .as_ref()
                .ok_or_else(|| err("unsupported", "缺少结构校验器"))?
                .validate_arguments(schema, value)?;
        }
        Ok(())
    }

    pub fn configured_tool_request(&self, command: &Command) -> Result<ConfiguredToolRequest> {
        use crate::{concurrency::*, workflow_resources::WorkflowResources};
        let (intent, _) = self
            .original_runtime_intent("workflow.tool.execute", command)?
            .ok_or_else(|| err("permission_denied", "工具缺少原执行意图"))?;
        let authority: crate::tool_calls::WorkflowToolAuthority =
            serde_json::from_value(intent.data["execution_authority"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        let instance = authority
            .instances
            .first()
            .ok_or_else(|| err("storage", "原流程身份缺失"))?;
        let resources =
            WorkflowResources::read(self.store.as_ref(), instance, &authority.execution_id)?;
        let live = self
            .store
            .workflow_tool_facts(&authority, &resources.instance.project_id)?;
        if live.suspended {
            return Err(err("workflow_tool_suspended", "原流程暂停，保留原请求"));
        }
        require(
            live.allowed
                && intent.definition_id == authority.execution_id
                && command.payload["execution_id"] == authority.execution_id,
            "工具原步骤资格已失效",
        )?;
        let step = resources
            .instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == resources.execution.step)
            .ok_or_else(|| err("storage", "原工具步骤缺失"))?;
        let StepExecutor::Tool { tool_ref, .. } = &step.executor else {
            return Err(err("invalid_workflow", "此步骤不调用工具"));
        };
        let arguments = command.payload["arguments"].clone();
        require(
            arguments.is_object()
                && command.payload["arguments_digest"] == configuration_hash(&arguments),
            "工具参数必须来自确切原派发意图",
        )?;
        let selected = self
            .store
            .read_resources(&[ResourceKey::new(
                "workflow_dispatch_source",
                json!([authority.execution_id, authority.plan_revision_id]).to_string(),
            )])?
            .remove(0);
        let dispatch = ConfigurationRecord::from_storage(
            selected
                .value
                .ok_or_else(|| err("permission_denied", "工具原派发缺失"))?,
        )?;
        let decision = self
            .store
            .read_resources(&[ResourceKey::new(
                "governance",
                text(&dispatch.data, "decision_id")?,
            )])?
            .remove(0);
        let decision = ConfigurationRecord::from_storage(
            decision
                .value
                .ok_or_else(|| err("permission_denied", "工具原派发决定缺失"))?,
        )?;
        let decided: WorkflowDecision = decision.decode()?;
        require(
            dispatch.r#type == "workflow_dispatch"
                && dispatch.immutable
                && decision.r#type == "workflow_decision"
                && decision.immutable
                && dispatch.scope == resources.record.scope
                && decision.scope == resources.record.scope
                && dispatch.definition_id == authority.execution_id
                && dispatch.data["decision_id"] == decision.id
                && decided.instance_id == resources.record.id
                && decided.execution_id.as_deref() == Some(authority.execution_id.as_str())
                && decided.plan_revision_id == authority.plan_revision_id
                && decided.root_instance_id == resources.instance.root_instance_id
                && decided.operation == "dispatch",
            "工具原决定或 Actor 归属不符",
        )?;
        Ok(ConfiguredToolRequest {
            provenance: crate::tool_calls::WorkflowToolProvenance {
                intent,
                dispatch,
                context: InvocationContext::workflow(
                    decided.actor,
                    Some(resources.instance.project_id.clone()),
                ),
                decision,
            },
            execution_id: authority.execution_id.clone(),
            authority,
            owner_id: resources.instance.owner_id.clone(),
            project_id: resources.instance.project_id.clone(),
            tool: resources
                .instance
                .plan
                .tool_bindings
                .get(tool_ref)
                .ok_or_else(|| err("integration_binding_required", "工具绑定未冻结"))?
                .clone(),
            permissions: resources
                .instance
                .plan
                .permission_locks
                .get(tool_ref)
                .ok_or_else(|| err("permission_unconfigured", "工具权限未冻结"))?
                .clone(),
            arguments,
        })
    }
    pub fn evaluate_configured_workflows(&self) -> Result<()> {
        self.evaluate_native_workflows()
    }

    pub(crate) fn configured_runtime_query(
        &self,
        method: &str,
        p: &Value,
    ) -> Option<Result<Value>> {
        match method {
            "query.workflow_instance" => Some(self.workflow_instance_detail(p)),
            "query.execution_task" => {
                let turn = match crate::read_models::identifier(p, "turn_id") {
                    Ok(turn) => turn,
                    Err(error) => return Some(Err(error)),
                };
                if p["project_id"]
                    .as_str()
                    .is_some_and(|project| self.store.project_id().as_deref() != Some(project))
                {
                    return Some(Err(err("scope_violation", "冻结任务不属于当前项目")));
                }
                match self.store.execution_task_detail(&turn) {
                    Ok(Some(task)) => Some(Ok(task)),
                    Ok(None) => None,
                    Err(error) => Some(Err(error)),
                }
            }
            _ => None,
        }
    }
}
