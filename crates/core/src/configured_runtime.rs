//! Durable execution of configuration-owned steps using the existing native ports.
use crate::governance::{record, reference, update_record};
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

fn rework_workspace(
    state: &EngineeringState,
    instance_id: &str,
    instance: &ConfiguredWorkflowInstance,
    execution: &StepExecution,
    step: &StepDefinition,
) -> Result<Option<ReworkWorkspaceCheckpoint>> {
    if step.access.writable_paths.is_empty() {
        return Ok(None);
    }
    // Resolve against the Workflow binding, not the preceding round's effective
    // workspace: queued rounds have no inputs yet, and completed rounds may
    // already consume a checkpoint. Losing that original binding restores the
    // old Red snapshot on the next dispatch.
    let Some(previous) = execution
        .rework_workspace
        .as_ref()
        .map(|checkpoint| &checkpoint.previous_workspace)
        .or_else(|| {
            step.access
                .workspace_input
                .as_ref()
                .and_then(|name| execution.inputs.get(name))
        })
    else {
        return Ok(None);
    };
    require(
        previous["owner_id"] == instance.owner_id,
        "写入返工检查点必须属于当前工作",
    )?;
    let head = state
        .heads
        .iter()
        .find(|head| head.project_id == instance.project_id && head.id == instance.owner_id)
        .ok_or_else(|| err("configuration_required", "返工工作区代码头未登记"))?;
    if previous["revision"] == head.revision {
        return Ok(None);
    }
    let output = state.governance.iter().rev().find(|output| {
        output.kind == "workflow_output"
            && output.scope.project() == Some(instance.project_id.as_str())
            && output.data["value"]["workspace_id"] == previous["workspace_id"]
            && output.data["value"]["owner_id"] == instance.owner_id
            && output.data["value"]["revision"] == head.revision
            && output.data["provenance"]["kind"] == "platform"
            && output.data["provenance"]["tool"] == "workspace.snapshot"
            && output.data["provenance"]["pointer"] == ""
            && state.governance.iter().any(|call| {
                call.id == output.data["provenance"]["call_id"].as_str().unwrap_or("")
                    && call.kind == "platform_call"
                    && call.data["status"] == "succeeded"
                    && call.data["request"]["instance_id"] == instance_id
                    && call.data["receipt"] == output.data["value"]
            })
    });
    let output = output.ok_or_else(|| {
        err(
            "needs_reconciliation",
            "当前代码头缺少本实例的真实平台快照，不能开始写入返工",
        )
    })?;
    Ok(Some(ReworkWorkspaceCheckpoint {
        previous_workspace: previous.clone(),
        output_id: output.id.clone(),
        workspace: output.data["value"].clone(),
    }))
}

fn workspace_view(
    state: &EngineeringState,
    instance: &ConfiguredWorkflowInstance,
    execution: &StepExecution,
    step: &StepDefinition,
) -> Result<Option<FrozenCode>> {
    let Some(input) = &step.access.workspace_input else {
        return Ok(None);
    };
    let grant = execution
        .inputs
        .get(input)
        .ok_or_else(|| err("input_unavailable", "工作空间输入不存在"))?;
    let workspace_id = text(grant, "workspace_id")?;
    let revision = text(grant, "revision")?;
    require(
        [40, 64].contains(&revision.len()) && revision.bytes().all(|b| b.is_ascii_hexdigit()),
        "工作空间需要确切代码版本",
    )?;
    let workspace = state
        .repository_worktrees
        .iter()
        .find(|w| w.id == workspace_id && w.project_id == instance.project_id)
        .ok_or_else(|| err("scope_violation", "工作空间不属于当前项目"))?;
    require(
        workspace.owner_id == instance.owner_id
            || workspace.owner_id == instance.project_id
            || state
                .tasks
                .iter()
                .any(|t| t.id == workspace.owner_id && t.feature_id == instance.owner_id),
        "工作空间不属于当前执行范围",
    )?;
    require(
        step.access.writable_paths.is_empty()
            || matches!(workspace.kind, WorktreeKind::Feature | WorktreeKind::Task),
        "代码写入需要受管理的工作副本",
    )?;
    let project = state
        .projects
        .iter()
        .find(|p| p.id == instance.project_id)
        .ok_or_else(|| err("not_found", "Project 不存在"))?;
    let target = state
        .repository_worktrees
        .iter()
        .find(|w| w.project_id == instance.project_id && w.owner_id == instance.project_id)
        .ok_or_else(|| err("configuration_required", "项目主干工作空间尚未就绪"))?;
    Ok(Some(FrozenCode {
        revision,
        context: GitContext {
            project: instance.project_id.clone(),
            workspace: project
                .workspace_definition
                .clone()
                .ok_or_else(|| err("configuration_required", "项目仓库未登记"))?,
            source: CodeLocation {
                owner: workspace.owner_id.clone(),
                branch: workspace.branch.clone(),
                worktree: workspace.path.clone(),
            },
            target: CodeLocation {
                owner: target.owner_id.clone(),
                branch: target.branch.clone(),
                worktree: target.path.clone(),
            },
            origin: Origin::External,
        },
    }))
}

fn instances_for_executions(
    state: &EngineeringState,
    executions: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut ids: BTreeSet<String> = state
        .governance
        .iter()
        .filter(|r| {
            r.kind == "workflow_instance"
                && r.data["parent_execution_id"]
                    .as_str()
                    .is_some_and(|id| executions.contains(id))
        })
        .map(|r| r.id.clone())
        .collect();
    let initial: Vec<_> = ids.iter().cloned().collect();
    for id in initial {
        ids.extend(descendant_instances(state, &id));
    }
    ids
}
pub(crate) fn active_execution(state: &EngineeringState, execution: &StepExecution) -> bool {
    if state
        .governance
        .iter()
        .filter(|r| r.kind == "workflow_instance" && r.data["parent_execution_id"] == execution.id)
        .filter_map(|r| r.decode::<ConfiguredWorkflowInstance>().ok())
        .any(|child| child.steps.iter().any(|e| active_execution(state, e)))
    {
        return true;
    }
    if state.governance.iter().any(|r| {
        r.kind == "platform_call"
            && r.definition_id == execution.id
            && !matches!(
                r.data["status"].as_str(),
                Some("succeeded" | "failed" | "cancelled")
            )
    }) {
        return true;
    }
    state
        .executions
        .iter()
        .any(|e| Some(&e.session_id) == execution.session_id.as_ref() && !e.writer_revoked)
        || state
            .turns
            .iter()
            .any(|t| Some(&t.session_id) == execution.session_id.as_ref() && t.status.active())
        || execution
            .tool_call_id
            .as_ref()
            .and_then(|id| state.configuration(id).ok())
            .is_some_and(|r| {
                !["succeeded", "failed", "cancelled"]
                    .contains(&r.data["status"].as_str().unwrap_or(""))
            })
}

pub(crate) fn descendant_instances(state: &EngineeringState, parent: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::from([parent.to_owned()]);
    loop {
        let executions: BTreeSet<_> = state
            .governance
            .iter()
            .filter(|r| ids.contains(&r.id))
            .flat_map(|r| r.data["steps"].as_array().into_iter().flatten())
            .filter_map(|e| e["id"].as_str())
            .collect();
        let added: Vec<_> = state
            .governance
            .iter()
            .filter(|r| {
                r.kind == "workflow_instance"
                    && r.data["parent_execution_id"]
                        .as_str()
                        .is_some_and(|id| executions.contains(id))
                    && !ids.contains(&r.id)
            })
            .map(|r| r.id.clone())
            .collect();
        if added.is_empty() {
            break;
        }
        ids.extend(added);
    }
    ids.remove(parent);
    ids
}

fn update_budget_wait(state: &EngineeringState, execution: &mut StepExecution) {
    let child_ids = instances_for_executions(state, &BTreeSet::from([execution.id.clone()]));
    let waiting = matches!(
        execution.status,
        StepExecutionStatus::WaitingInput
            | StepExecutionStatus::WaitingConfirmation
            | StepExecutionStatus::AwaitingAcceptance
    ) || state.workflow_instance_suspended(&execution.workflow_instance_id)
        || (!child_ids.is_empty() && !active_execution(state, execution))
        || execution.turn_id.as_ref().is_some_and(|id| {
            state
                .turns
                .iter()
                .any(|t| t.id == *id && t.status == TurnStatus::WaitingInteraction)
        })
        || state
            .governance
            .iter()
            .filter(|r| child_ids.contains(&r.id))
            .any(|r| {
                r.data["steps"].as_array().is_some_and(|steps| {
                    steps.iter().any(|step| {
                        matches!(
                            step["status"].as_str(),
                            Some("waiting_input" | "waiting_confirmation" | "awaiting_acceptance")
                        ) || step["turn_id"].as_str().is_some_and(|id| {
                            state
                                .turns
                                .iter()
                                .any(|t| t.id == id && t.status == TurnStatus::WaitingInteraction)
                        })
                    })
                })
            });
    if waiting {
        if execution.budget_wait_started.is_none() {
            execution.budget_wait_started = Some(now());
        }
    } else if let Some(started) = execution.budget_wait_started.take() {
        execution.budget_waited_ms = execution
            .budget_waited_ms
            .saturating_add(now().saturating_sub(started));
    }
}

fn execution_result(state: &EngineeringState, execution: &StepExecution) -> Result<Option<Value>> {
    let Some(turn_id) = &execution.turn_id else {
        return Ok(None);
    };
    let turn = state
        .turns
        .iter()
        .find(|t| &t.id == turn_id)
        .ok_or_else(|| err("not_found", "步骤轮次不存在"))?;
    match turn.status {
        TurnStatus::Completed => {
            let text = state
                .conversation_events
                .iter()
                .filter(|e| e.turn_id == *turn_id && e.kind == ConversationEventKind::Assistant)
                .fold(String::new(), |mut text, event| {
                    if event.mode == MessageMode::Full {
                        text = event.content.clone()
                    } else {
                        text.push_str(&event.content)
                    };
                    text
                });
            serde_json::from_str(&text).map(Some).map_err(|e| {
                err(
                    "output_invalid",
                    format!("Agent 输出需要符合声明的 JSON 封套: {e}"),
                )
            })
        }
        TurnStatus::Failed => Err(err("agent_failed", "Agent 执行失败")),
        TurnStatus::Cancelled => Err(err("cancelled", "Agent 执行已取消")),
        _ => Ok(None),
    }
}

pub(crate) fn matching_confirmation<'a>(
    state: &'a EngineeringState,
    execution_id: &str,
    input_digest: &str,
    outputs: &Value,
) -> Option<&'a ConfigurationRecord> {
    state.governance.iter().find(|r| {
        r.kind == "workflow_confirmation"
            && r.definition_id == execution_id
            && r.data["input_digest"] == input_digest
            && r.data["outputs"] == *outputs
    })
}

pub(crate) fn facts(
    state: &EngineeringState,
    instance: &ConfiguredWorkflowInstance,
) -> Result<WorkflowFacts> {
    let mut facts = WorkflowFacts::default();
    for (name, value) in &instance.inputs {
        facts
            .values
            .insert(("$input".into(), name.clone()), value.clone());
    }
    for execution in &instance.steps {
        for (name, id) in &execution.outputs {
            let output = state.configuration(id)?;
            require(
                output.kind == "workflow_output" && output.data["execution_id"] == execution.id,
                "步骤产物来源不匹配",
            )?;
            facts.values.insert(
                (execution.step.clone(), name.clone()),
                output.data["value"].clone(),
            );
        }
        if execution.status == StepExecutionStatus::Completed
            || execution.status == StepExecutionStatus::WaitingConfirmation
            || execution.status == StepExecutionStatus::AwaitingAcceptance
        {
            facts.executions.insert(
                execution.step.clone(),
                ExecutionFact {
                    operation_id: execution.id.clone(),
                    input_digest: execution.input_digest.clone(),
                    identity_root: execution
                        .agent_revision_id
                        .as_ref()
                        .map(|agent| state.agent_identity(agent))
                        .transpose()?,
                    started: execution.started.unwrap_or(0),
                    finished: execution.finished.unwrap_or(0),
                    outcome: "succeeded".into(),
                },
            );
        }
        if matching_confirmation(
            state,
            &execution.id,
            &execution.input_digest,
            &json!(execution.outputs),
        )
        .is_some()
        {
            facts.approvals.insert(execution.step.clone());
        }
    }
    for material in state.governance.iter().filter(|r| {
        r.kind == "typed_material"
            && r.data["project_id"] == instance.project_id
            && r.data["root_instance_id"] == instance.root_instance_id
    }) {
        facts.values.insert(
            ("$material".into(), material.id.clone()),
            material.data["value"].clone(),
        );
    }
    Ok(facts)
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
    let checks = step
        .spec_refs
        .iter()
        .filter_map(|id| definitions.get(id))
        .filter(|r| r.decode::<ConfiguredSpecification>().is_ok())
        .map(|r| r.decode::<ConfiguredSpecification>().map(|s| s.checks))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let port_schemas:BTreeMap<_,_>=step.outputs.iter().map(|(name,port)| {
        let definition:DeliveryTypeDefinition=definitions.get(&port.delivery_ref).ok_or_else(||err("configuration_required","交付类型不在配置闭包中"))?.decode()?;
        let mut constraints=vec![];
        if ["markdown","text","diff"].contains(&definition.format.as_str()) {constraints.push(json!({"type":"string","description":"Text content encoded as a JSON string; do not wrap it in a title/body object."}));}
        if let Some(schema)=definition.content_schema {constraints.push(schema);}
        if port.content_checks {
            constraints.push(json!({"type":"object","required":["approve","summary","checks"],"properties":{"approve":{"type":"boolean"},"summary":{"type":"string"},"checks":{"type":"array","minItems":checks.len(),"maxItems":checks.len(),"items":{"type":"object","required":["key","passed","evidence"],"properties":{"key":{"enum":checks.iter().map(|check|&check.key).collect::<Vec<_>>()},"passed":{"type":"boolean"},"evidence":{"type":"string","minLength":1}},"additionalProperties":false}}},"additionalProperties":false}));
        }
        Ok((name.clone(),match constraints.len(){0=>json!({}),1=>constraints.remove(0),_=>json!({"allOf":constraints})}))
    }).collect::<Result<_>>()?;
    let outputs = step.outputs.iter().map(|(name,port)|Ok((name.clone(),json!({"required":port.required,"schema":port_schemas[name],"content_checks":port.content_checks,
        "required_checks":if port.content_checks {json!(checks)}else{Value::Null},
        "check_report_contract":if port.content_checks {json!({"required":["approve","summary","checks"],"check_fields":["key","passed","evidence"],"approve":"true exactly when all declared checks pass"})}else{Value::Null},
        "definition":definitions.get(&port.delivery_ref).ok_or_else(||err("configuration_required","交付类型不在配置闭包中"))?}))))
        .collect::<Result<BTreeMap<_,_>>>()?;
    let mut input_sources:BTreeMap<_,_>=step.inputs.iter().map(|(name,binding)| {
        let source_output_id=match binding {InputBinding::Source{source} if source.step!="$input"=>instance.steps.iter().find(|step|step.step==source.step).and_then(|step|step.outputs.get(&source.port)),_=>None};
        (name.clone(),json!({"binding":match binding {InputBinding::Source{source}=>json!({"type":"source","source":source}),InputBinding::Constant{..}=>json!({"type":"constant"}),InputBinding::Material{material_ref}=>json!({"type":"material","material_ref":material_ref})},"source_output_id":source_output_id,"value_digest":execution.inputs.get(name).map(configuration_hash)}))
    }).collect();
    if let Some(checkpoint) = &execution.rework_workspace {
        if let Some(name) = &step.access.workspace_input {
            if execution.inputs.get(name) == Some(&checkpoint.workspace) {
                let source = input_sources.get_mut(name).unwrap();
                source["original_source_output_id"] = source["source_output_id"].clone();
                source["source_output_id"] = json!(checkpoint.output_id);
                source["rework_checkpoint"] = json!(checkpoint);
            }
        }
    }
    let mut rendered_inputs = execution.inputs.clone();
    for (name, value) in &execution.inputs {
        let bytes = serde_json::to_vec(value).map_err(|e| err("serialization", e.to_string()))?;
        if bytes.len() > 32768 {
            if let Some(id) = input_sources
                .get(name)
                .and_then(|source| source["source_output_id"].as_str())
            {
                let pointer = match &step.inputs[name] {
                    _ if input_sources[name].get("rework_checkpoint").is_some() => "",
                    InputBinding::Source { source } => source.pointer.as_str(),
                    _ => "",
                };
                let reader = instance.plan.definitions.values().find(|r| {
                    r.kind == "tool"
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
        .filter_map(|r| r.decode::<DeliveryTypeDefinition>().ok())
        .flat_map(|delivery| delivery.spec_refs)
        .filter(|reference| !step.spec_refs.contains(reference))
        .collect();
    let mut input = json!({"renderer_version":AGENT_TASK_RENDERER,"source":{"workflow_revision_id":instance.workflow_ref,"standard_ref":instance.plan.standard_ref,"work_configuration_ref":instance.plan.work_configuration_ref,"execution_id":execution.id,
        "workflow_instance_id":execution.workflow_instance_id,"plan_revision_id":execution.plan_revision_id,"node_id":execution.node_id,"contract_digest":execution.contract_digest,"path":execution.path,"step":step.key,"round":execution.round,"sequence":execution.sequence},
        "owner_id":instance.owner_id,
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
    let input = serde_json::to_string(&input).map_err(|e| err("serialization", e.to_string()))?;
    require(
        input.len() <= TASK_PAYLOAD_LIMIT,
        "步骤任务及输入超过 512 KiB",
    )?;
    Ok(input)
}

fn mapped_tool_outputs(
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

fn output_provenance(
    state: &EngineeringState,
    row: &ConfigurationRecord,
    step: &StepDefinition,
    execution: &StepExecution,
    name: &str,
    definition: &DeliveryTypeDefinition,
    value: &Value,
) -> Result<Value> {
    let mut provenance =
        json!({"value_digest":configuration_hash(value),"spec_refs":definition.spec_refs});
    match &step.executor {
        StepExecutor::Tool { output_mapping, .. } => {
            if let Some(call) = execution
                .tool_call_id
                .as_ref()
                .and_then(|id| state.configuration(id).ok())
                .filter(|r| r.kind == "platform_call")
            {
                let tool = call.data["request"]["definition"]["qualified_name"]
                    .as_str()
                    .unwrap_or("");
                require(
                    call.data["status"] == "succeeded"
                        && call.data["request"]["execution_id"] == execution.id
                        && call.data["request"]["instance_id"] == row.id,
                    "交付缺少当前步骤的真实平台回执",
                )?;
                let pointer = output_mapping.get(name).map(String::as_str).unwrap_or("");
                require(
                    call.data["receipt"].pointer(pointer) == Some(value),
                    "交付内容与真实工具回执不一致",
                )?;
                if let Some(DeliveryProducer::Platform { tools }) = &definition.producer {
                    require(tools.iter().any(|t| t == tool), "交付来源工具不匹配")?;
                }
                provenance["kind"] = json!("platform");
                provenance["call_id"] = json!(call.id);
                provenance["tool"] = json!(tool);
                provenance["pointer"] = json!(pointer);
                if definition.format == "diff" {
                    let receipt = &call.data["receipt"];
                    let comparison = pointer
                        .strip_suffix("/diff")
                        .and_then(|p| receipt.pointer(p));
                    let base = comparison
                        .and_then(|v| v.get("base_revision"))
                        .unwrap_or(&receipt["target_revision"]);
                    let head = comparison
                        .and_then(|v| v.get("revision"))
                        .unwrap_or(&receipt["revision"]);
                    require(
                        base.is_string() && head.is_string(),
                        "Diff 缺少确切比较端点",
                    )?;
                    provenance["base_revision"] = base.clone();
                    provenance["revision"] = head.clone();
                    provenance["comparison"] = json!(pointer);
                }
                return Ok(provenance);
            }
            require(
                !matches!(definition.producer, Some(DeliveryProducer::Platform { .. })),
                "平台交付不能由普通工具回执替代",
            )?;
            provenance["kind"] = json!("tool");
        }
        StepExecutor::Agent { .. } => {
            require(
                !matches!(
                    definition.producer,
                    Some(DeliveryProducer::Platform { .. } | DeliveryProducer::Workflow)
                ),
                "Agent 不能冒充平台或子流程产出",
            )?;
            provenance["kind"] = json!("agent");
            provenance["turn_id"] = json!(execution.turn_id);
        }
        StepExecutor::Workflow { foreach, .. } => {
            provenance["kind"] = json!("workflow");
            let group = state
                .governance
                .iter()
                .find(|r| r.kind == "workflow_children" && r.definition_id == execution.id)
                .ok_or_else(|| err("output_invalid", "缺少实际子流程交付来源"))?;
            let ids: Vec<String> = serde_json::from_value(group.data["children"].clone())
                .map_err(|e| err("output_invalid", e.to_string()))?;
            let children = ids
                .iter()
                .map(|id| {
                    state
                        .configuration(id)?
                        .decode::<ConfiguredWorkflowInstance>()
                })
                .collect::<Result<Vec<_>>>()?;
            require(
                children.iter().all(|child| {
                    child.status == StepExecutionStatus::Completed
                        && child.parent_execution_id.as_ref() == Some(&execution.id)
                }),
                "汇总必须来自本次实际完成的子流程",
            )?;
            if let Some(collection) = foreach {
                let parent: ConfiguredWorkflowInstance = row.decode()?;
                let current = facts(state, &parent)?;
                let members = current
                    .value(&collection.source)
                    .and_then(Value::as_array)
                    .ok_or_else(|| err("output_invalid", "汇总来源需要数组"))?;
                require(
                    children.len() == members.len()
                        && children.iter().zip(members).all(|(child, member)| {
                            child.inputs.get(&collection.item_input) == Some(member)
                        }),
                    "汇总任务成员遗漏、重复或顺序不符",
                )?;
            }
            provenance["children"] = json!(ids);
        }
        StepExecutor::Control => {
            provenance["kind"] = json!("control");
        }
    }
    Ok(provenance)
}

fn intent(
    state: &mut EngineeringState,
    execution: &StepExecution,
    scope: &Scope,
    method: &str,
    target: &str,
    payload: Value,
) -> Id {
    let operation = format!(
        "workflow:{}",
        configuration_hash(&(execution.id.as_str(), execution.sequence, method))
    );
    if let Some(old) = state
        .governance
        .iter()
        .find(|r| r.kind == "workflow_intent" && r.data["command"]["command_id"] == operation)
    {
        return old.id.clone();
    }
    let revision = revision(state, target).or_else(|| {
        state
            .sessions
            .iter()
            .find(|s| s.id == target)
            .map(|s| s.revision)
    });
    let row = record(
        "workflow_intent",
        execution.id.clone(),
        scope.clone(),
        method.into(),
        1,
        None,
        json!({"status":"prepared","configured":true,"method":method,"command":Command {command_id:operation,target:Some(target.into()),expected_revision:revision,payload}}),
        false,
    );
    let id = row.id.clone();
    state.governance.push(row);
    id
}

impl Engine {
    pub(crate) fn start_configured_feature(
        &self,
        state: &mut EngineeringState,
        feature: &str,
        configuration: &ConfigurationRecord,
        p: &Value,
    ) -> Result<()> {
        let owner = state.feature(feature)?.clone();
        let mut selection = configuration.data.clone();
        for key in [
            "standard_ref",
            "workflow_ref",
            "role_agent_bindings",
            "permission_context_ref",
        ] {
            if let Some(value) = p.get(key) {
                selection[key] = value.clone();
            }
        }
        selection["source_configuration_ref"] = json!(configuration.id);
        selection["inputs"] = p.get("workflow_inputs").cloned().unwrap_or(json!({}));
        self.configured_runtime_command(
            "workflow.start",
            &Command {
                command_id: format!("work:{feature}:workflow"),
                target: Some(feature.into()),
                expected_revision: Some(owner.revision),
                payload: selection,
            },
            state,
        )?
        .ok_or_else(|| err("configuration_required", "工作未选择当前 Workflow"))?;
        Ok(())
    }

    pub fn configured_agent_request(
        &self,
        command: &Command,
    ) -> Result<Option<ConfiguredAgentRequest>> {
        if command.payload["controller_id"].is_string() {
            return self.controller_agent_request(command);
        }
        let Some(id) = command.payload["configured_execution_id"].as_str() else {
            return Ok(None);
        };
        let state = self.store.snapshot()?;
        require(
            state.governance.iter().any(|r| {
                r.kind == "workflow_intent"
                    && r.definition_id == id
                    && r.data["method"] == "session.open"
                    && r.data["command"] == json!(command)
            }),
            "Agent 会话缺少确切工作流意图",
        )?;
        let row = state
            .governance
            .iter()
            .find(|r| {
                r.kind == "workflow_instance"
                    && r.data["steps"]
                        .as_array()
                        .is_some_and(|steps| steps.iter().any(|s| s["id"] == id))
            })
            .ok_or_else(|| err("not_found", "Agent 步骤不存在"))?;
        let instance: ConfiguredWorkflowInstance = row.decode()?;
        let execution = instance.steps.iter().find(|e| e.id == id).unwrap();
        require(
            !state.workflow_instance_suspended(&row.id)
                && !instance.cancel_requested
                && execution.status == StepExecutionStatus::Running
                && command.target.as_ref() == Some(&instance.owner_id)
                && command.payload["agent"].as_str() == execution.agent_revision_id.as_deref(),
            "Agent 步骤当前不可执行或归属不符",
        )?;
        let step = instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == execution.step)
            .unwrap();
        workspace_view(&state, &instance, execution, step)?;
        let mut integration_lock = instance
            .plan
            .integration_locks
            .get(execution.agent_revision_id.as_ref().unwrap())
            .ok_or_else(|| err("configuration_required", "Agent 接入未冻结"))?
            .clone();
        integration_lock
            .tools
            .retain(|tool| step.access.tool_refs.contains(&tool.revision_id));
        integration_lock
            .platform_tools
            .retain(|tool| step.access.tool_refs.contains(&tool.id));
        Ok(Some(ConfiguredAgentRequest {
            integration_lock,
            access: step.access.clone(),
            permissions: instance
                .plan
                .permission_locks
                .get(execution.agent_revision_id.as_ref().unwrap())
                .ok_or_else(|| err("permission_unconfigured", "Agent 权限未冻结"))?
                .clone(),
        }))
    }

    pub(crate) fn configured_session_code_view(
        &self,
        state: &EngineeringState,
        session: &Session,
    ) -> Result<Option<Option<FrozenCode>>> {
        let Some(source) = state
            .governance
            .iter()
            .find(|r| r.kind == "configured_session" && r.definition_id == session.id)
        else {
            return Ok(None);
        };
        let instance: ConfiguredWorkflowInstance = state
            .configuration(source.data["instance_id"].as_str().unwrap())?
            .decode()?;
        let execution = instance
            .steps
            .iter()
            .find(|e| Some(e.id.as_str()) == source.data["execution_id"].as_str())
            .ok_or_else(|| err("task_contract_mismatch", "步骤执行已失效"))?;
        let step = instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == execution.step)
            .unwrap();
        Ok(Some(workspace_view(state, &instance, execution, step)?))
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
        let state = self.store.snapshot()?;
        let execution_id = text(&command.payload, "execution_id")?;
        require(
            state.governance.iter().any(|r| {
                r.kind == "workflow_intent"
                    && r.definition_id == execution_id
                    && r.data["method"] == "workflow.tool.execute"
                    && r.data["command"] == json!(command)
            }),
            "工具执行缺少确切工作流意图",
        )?;
        let row = state
            .governance
            .iter()
            .find(|r| {
                r.kind == "workflow_instance"
                    && r.data["steps"]
                        .as_array()
                        .is_some_and(|steps| steps.iter().any(|s| s["id"] == execution_id))
            })
            .ok_or_else(|| err("not_found", "工具步骤不存在"))?;
        let instance: ConfiguredWorkflowInstance = row.decode()?;
        require(
            !state.workflow_instance_suspended(&row.id) && !instance.cancel_requested,
            "工作流已暂停或正在取消",
        )?;
        let execution = instance
            .steps
            .iter()
            .find(|e| e.id == execution_id)
            .unwrap();
        require(
            execution.status == StepExecutionStatus::Running,
            "工具步骤当前不可执行",
        )?;
        let step = instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == execution.step)
            .unwrap();
        let StepExecutor::Tool {
            tool_ref,
            arguments,
            ..
        } = &step.executor
        else {
            return Err(err("invalid_workflow", "此步骤不调用工具"));
        };
        let facts = facts(&state, &instance)?;
        let arguments: serde_json::Map<String, Value> = arguments
            .iter()
            .map(|(name, value)| {
                let value = match value {
                    InputBinding::Constant { value } => value.clone(),
                    InputBinding::Material { material_ref } => facts
                        .values
                        .get(&("$material".into(), material_ref.clone()))
                        .cloned()
                        .ok_or_else(|| err("missing_material", "材料不存在"))?,
                    InputBinding::Source { source } => facts
                        .value(source)
                        .cloned()
                        .ok_or_else(|| err("input_unavailable", "工具参数来源尚未就绪"))?,
                };
                let value = if name == "workspace" {
                    checkpoint_workspace(execution, &value)
                } else {
                    value
                };
                Ok((name.clone(), value))
            })
            .collect::<Result<_>>()?;
        Ok(ConfiguredToolRequest {
            execution_id,
            owner_id: instance.owner_id,
            project_id: instance.project_id,
            tool: instance
                .plan
                .tool_bindings
                .get(tool_ref)
                .ok_or_else(|| err("integration_binding_required", "工具绑定不在执行闭包中"))?
                .clone(),
            permissions: instance
                .plan
                .permission_locks
                .get(tool_ref)
                .ok_or_else(|| err("permission_unconfigured", "工具权限未冻结"))?
                .clone(),
            arguments: Value::Object(arguments),
        })
    }
    pub(crate) fn configured_session_opened(
        &self,
        state: &mut EngineeringState,
        command: &Command,
        result: &Value,
    ) -> Result<()> {
        let Some(execution_id) = command.payload["configured_execution_id"].as_str() else {
            return Ok(());
        };
        let row = state
            .governance
            .iter()
            .find(|r| {
                r.kind == "workflow_instance"
                    && r.data["steps"]
                        .as_array()
                        .is_some_and(|steps| steps.iter().any(|s| s["id"] == execution_id))
            })
            .ok_or_else(|| err("invalid_workflow", "执行来源不存在"))?
            .clone();
        let instance: ConfiguredWorkflowInstance = row.decode()?;
        let execution = instance
            .steps
            .iter()
            .find(|e| e.id == execution_id)
            .unwrap();
        let session_id = text(result, "id")?;
        let session = state
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .ok_or_else(|| err("not_found", "Session 不存在"))?;
        require(
            !instance.cancel_requested
                && execution.status == StepExecutionStatus::Running
                && session.owner_id == instance.owner_id
                && Some(&session.agent) == execution.agent_revision_id.as_ref(),
            "Agent 会话与步骤执行归属或状态不符",
        )?;
        state.governance.push(record("configured_session",session_id,row.scope,"步骤任务来源".into(),1,None,
            json!({"execution_id":execution_id,"instance_id":row.id,"workflow_revision_id":instance.workflow_ref,"step":execution.step}),true));
        Ok(())
    }

    pub(crate) fn configured_turn_task(
        &self,
        state: &mut EngineeringState,
        turn_id: &str,
    ) -> Result<()> {
        let turn = state
            .turns
            .iter()
            .find(|t| t.id == turn_id)
            .ok_or_else(|| err("not_found", "Turn 不存在"))?;
        let Some(source) = state
            .governance
            .iter()
            .find(|r| r.kind == "configured_session" && r.definition_id == turn.session_id)
            .cloned()
        else {
            return Ok(());
        };
        let input: Value = serde_json::from_str(&turn.input)
            .map_err(|e| err("task_contract_mismatch", e.to_string()))?;
        require(
            input["renderer_version"].is_string()
                && input["source"]["execution_id"] == source.data["execution_id"],
            "步骤输入缺少可信任务来源，请通过工作流步骤提交补充输入",
        )?;
        let instance: ConfiguredWorkflowInstance = state
            .configuration(
                source.data["instance_id"]
                    .as_str()
                    .ok_or_else(|| err("task_contract_mismatch", "任务实例来源缺失"))?,
            )?
            .decode()?;
        let execution = instance
            .steps
            .iter()
            .find(|e| Some(e.id.as_str()) == source.data["execution_id"].as_str())
            .ok_or_else(|| err("task_contract_mismatch", "原步骤执行已经失效"))?;
        let step = instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == execution.step)
            .ok_or_else(|| err("task_contract_mismatch", "原步骤定义不存在"))?;
        let canonical: Value = serde_json::from_str(&task_input(
            &instance,
            step,
            execution,
            input["supplemental_input"].as_str(),
        )?)
        .map_err(|e| err("serialization", e.to_string()))?;
        require(
            input == canonical,
            "输入改变了 Workflow 任务、输出合同或授权来源",
        )?;
        let body = turn.input.clone();
        let digest = configuration_hash(&input);
        let agent = state
            .sessions
            .iter()
            .find(|s| s.id == turn.session_id)
            .ok_or_else(|| err("not_found", "Session 不存在"))?
            .agent
            .clone();
        let task = ConfiguredAgentTask {
            renderer_version: AGENT_TASK_RENDERER.into(),
            turn_id: turn_id.into(),
            agent_revision_id: agent,
            source: source.data,
            task: input,
            rendered_input: body,
            digest,
        };
        task.validate()?;
        state.governance.push(record(
            "configured_execution_task",
            turn_id.into(),
            source.scope,
            "实际步骤任务".into(),
            1,
            None,
            json!(task),
            true,
        ));
        Ok(())
    }

    pub(crate) fn configured_task(
        &self,
        state: &EngineeringState,
        turn: &str,
    ) -> Result<Option<ConfiguredAgentTask>> {
        state
            .governance
            .iter()
            .find(|r| r.kind == "configured_execution_task" && r.definition_id == turn)
            .map(|r| {
                let task: ConfiguredAgentTask = r.decode()?;
                task.validate()?;
                Ok(task)
            })
            .transpose()
    }

    pub(crate) fn configured_conversation_input(
        &self,
        state: &mut EngineeringState,
        session: &Session,
        command: &Command,
        input: &str,
    ) -> Result<Option<String>> {
        let Some(source) = state
            .governance
            .iter()
            .find(|r| r.kind == "configured_session" && r.definition_id == session.id)
            .cloned()
        else {
            return Ok(None);
        };
        if state.governance.iter().any(|r| {
            r.kind == "workflow_intent"
                && r.data["configured"] == true
                && r.data["method"] == "conversation.send"
                && r.data["command"] == json!(command)
        }) {
            return Ok(Some(input.into()));
        }
        let instance_id = text(&source.data, "instance_id")?;
        let row = state.configuration(&instance_id)?.clone();
        let mut instance: ConfiguredWorkflowInstance = row.decode()?;
        require(
            !matches!(
                instance.status,
                StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
            ),
            "流程终态不可重新控制，请建立后继实例",
        )?;
        require(
            !state.workflow_instance_suspended(&row.id) && !instance.cancel_requested,
            "工作流已暂停或正在取消",
        )?;
        let index = instance
            .steps
            .iter()
            .position(|s| Some(s.id.as_str()) == source.data["execution_id"].as_str())
            .ok_or_else(|| err("task_contract_mismatch", "步骤来源已失效"))?;
        let step = instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == instance.steps[index].step)
            .unwrap()
            .clone();
        require(
            instance.steps[index].status == StepExecutionStatus::WaitingInput,
            "此步骤不在等待输入；修改已完成工作请先返工",
        )?;
        require(
            instance.steps[index].sequence < step.budget.max_turns,
            "步骤已达到轮次上限",
        )?;
        instance.steps[index].sequence += 1;
        instance.steps[index].turn_id = None;
        instance.steps[index].status = StepExecutionStatus::Running;
        let rendered = task_input(&instance, &step, &instance.steps[index], Some(input))?;
        update_record(
            state
                .governance
                .iter_mut()
                .find(|r| r.id == instance_id)
                .unwrap(),
            json!(instance),
        );
        Ok(Some(rendered))
    }

    pub(crate) fn configured_runtime_command(
        &self,
        method: &str,
        command: &Command,
        state: &mut EngineeringState,
    ) -> Result<Option<Value>> {
        let target = command.target.as_deref().unwrap_or("");
        let p = &command.payload;
        if method == "workflow.start" {
            require(
                !state
                    .features
                    .iter()
                    .any(|f| f.id == target && f.status == Status::Completed)
                    && !state
                        .tasks
                        .iter()
                        .any(|t| t.id == target && t.status == Status::Completed),
                "已完成工作保持终态，请建立后继工作范围",
            )?;
            let actual =
                revision(state, target).ok_or_else(|| err("not_found", "工作流所属对象不存在"))?;
            if command.expected_revision != Some(actual) {
                return Err(err("revision_conflict", "工作流所属对象已变化，请刷新"));
            }
            let scope = state
                .projects
                .iter()
                .find(|r| r.id == target)
                .map(|p| Scope::Project {
                    project_id: p.id.clone(),
                })
                .or_else(|| {
                    state
                        .features
                        .iter()
                        .find(|f| f.id == target)
                        .map(|f| Scope::Feature {
                            project_id: f.project_id.clone(),
                            feature_id: f.id.clone(),
                        })
                })
                .or_else(|| {
                    state
                        .tasks
                        .iter()
                        .find(|t| t.id == target)
                        .and_then(|t| state.feature(&t.feature_id).ok())
                        .map(|f| Scope::Feature {
                            project_id: f.project_id.clone(),
                            feature_id: f.id.clone(),
                        })
                })
                .ok_or_else(|| err("not_found", "工作流所属对象不存在"))?;
            let inherited = p["work_configuration_ref"]
                .as_str()
                .and_then(|id| state.configuration(id).ok())
                .or_else(|| crate::standards::work_configuration(state, target))
                .cloned();
            if let Some(config) = &inherited {
                require(
                    config.kind == "work_configuration" && config.definition_id == target,
                    "工作配置不属于此工作",
                )?;
            }
            let configuration =
                crate::standards::select_work_configuration(state, target, p, inherited.as_ref())?;
            let workflow = reference(
                state,
                &text(&configuration.data, "workflow_ref")?,
                "workflow",
                &scope,
            )?
            .clone();
            let bindings: BTreeMap<Id, Id> =
                serde_json::from_value(configuration.data["role_agent_bindings"].clone())
                    .map_err(|e| err("invalid_params", e.to_string()))?;
            let selected = crate::standards::selection_state(state, &configuration.data);
            let mut plan =
                self.compile_configured_workflow(&selected, &workflow, &scope, Some(&bindings))?;
            plan.standard_ref = Some(text(&configuration.data, "standard_ref")?);
            plan.work_configuration_ref = Some(configuration.id.clone());
            require(
                !state.governance.iter().any(|r| {
                    r.kind == "workflow_instance"
                        && r.data["owner_id"] == target
                        && r.data["parent_execution_id"].is_null()
                        && !matches!(r.data["status"].as_str(), Some("completed" | "cancelled"))
                }),
                "同一归属已有未结束流程",
            )?;
            let inputs: BTreeMap<String, Value> =
                serde_json::from_value(p.get("inputs").cloned().unwrap_or(json!({})))
                    .map_err(|e| err("invalid_params", e.to_string()))?;
            for (name, port) in &plan.document.inputs {
                if port.required {
                    require(inputs.contains_key(name), format!("缺少流程输入 {name}"))?;
                }
                if let Some(value) = inputs.get(name) {
                    self.validate_configured_value(
                        &plan
                            .definitions
                            .get(&port.delivery_ref)
                            .ok_or_else(|| err("configuration_required", "输入类型不在闭包中"))?
                            .decode()?,
                        value,
                    )?;
                }
            }
            require(
                inputs
                    .keys()
                    .all(|key| plan.document.inputs.contains_key(key)),
                "流程包含未声明输入",
            )?;
            if let Some(feature) = state.features.iter_mut().find(|f| f.id == target) {
                if feature.status != Status::Running {
                    feature.status = Status::Running;
                    feature.revision += 1;
                }
            }
            let instance_id = id();
            let steps = plan
                .document
                .steps
                .iter()
                .map(|step| StepExecution {
                    plan_revision_id: String::new(),
                    node_id: step.node_id.clone(),
                    contract_digest: configuration_hash(&step),
                    id: id(),
                    workflow_instance_id: instance_id.clone(),
                    workflow_revision_id: workflow.id.clone(),
                    path: format!("/{}", step.key),
                    step: step.key.clone(),
                    round: 1,
                    sequence: 1,
                    status: StepExecutionStatus::Queued,
                    input_digest: String::new(),
                    budget_wait_started: None,
                    budget_waited_ms: 0,
                    rework_workspace: None,
                    rework: None,
                    inputs: BTreeMap::new(),
                    outputs: BTreeMap::new(),
                    agent_revision_id: None,
                    session_id: None,
                    turn_id: None,
                    tool_call_id: None,
                    started: None,
                    finished: None,
                    reason: String::new(),
                })
                .collect();
            let mut instance = ConfiguredWorkflowInstance {
                active_plan_revision_id: String::new(),
                control_version: 1,
                schema_version: WORKFLOW_INSTANCE_SCHEMA,
                owner_id: target.into(),
                workflow_ref: workflow.id.clone(),
                project_id: scope.project().unwrap().into(),
                plan,
                role_agent_bindings: bindings,
                inputs,
                steps,
                status: StepExecutionStatus::Queued,
                paused: false,
                parent_execution_id: None,
                root_instance_id: instance_id.clone(),
                cancel_requested: false,
            };
            crate::orchestration::freeze_plan(state, &mut instance, &scope, "initial")?;
            let mut row = record(
                "workflow_instance",
                target.into(),
                scope,
                workflow.name.clone(),
                1,
                None,
                json!(instance),
                false,
            );
            row.id = instance_id;
            let result = json!(row);
            state.governance.push(row);
            return Ok(Some(result));
        }
        if ![
            "workflow.pause",
            "workflow.resume",
            "workflow.cancel",
            "workflow.step.confirm",
            "workflow.step.approve",
            "workflow.step.input",
            "workflow.step.retry",
            "workflow.step.rework",
            "workflow.step.reconcile",
        ]
        .contains(&method)
        {
            return Ok(None);
        }
        let Some(row) = state
            .governance
            .iter()
            .find(|r| {
                r.id == target && r.kind == "workflow_instance" && r.data["steps"].is_array()
            })
            .cloned()
        else {
            return Ok(None);
        };
        let mut instance: ConfiguredWorkflowInstance = row.decode()?;
        if let Some(plan) = p["plan_revision_id"].as_str() {
            require(plan == instance.active_plan_revision_id, "活动计划已变化")?;
        }
        require(
            command.expected_revision == Some(row.revision)
                || (command
                    .expected_revision
                    .is_some_and(|v| v > 0 && v <= row.revision)
                    && p["control_version"].as_u64() == Some(instance.control_version)
                    && p["plan_revision_id"] == instance.active_plan_revision_id)
                || (method == "workflow.step.approve"
                    && command
                        .expected_revision
                        .is_some_and(|revision| revision > 0 && revision <= row.revision)),
            "工作流状态已变化，请刷新",
        )?;
        require(
            !matches!(
                instance.status,
                StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
            ),
            "流程终态不可重新控制，请建立后继实例",
        )?;
        if crate::workflow_changes::change_pending(state, &row.id)
            && ![
                "workflow.pause",
                "workflow.cancel",
                "workflow.step.reconcile",
            ]
            .contains(&method)
        {
            return Err(err("change_pending", "运行计划正在变更"));
        }
        match method {
            "workflow.pause" => {
                instance.paused = true;
                for execution in &mut instance.steps {
                    if execution.started.is_some() && execution.budget_wait_started.is_none() {
                        execution.budget_wait_started = Some(now());
                    }
                }
                let descendants = descendant_instances(state, &row.id);
                for child in state
                    .governance
                    .iter_mut()
                    .filter(|r| descendants.contains(&r.id))
                {
                    let mut child_instance: ConfiguredWorkflowInstance = child.decode()?;
                    for execution in &mut child_instance.steps {
                        if execution.started.is_some() && execution.budget_wait_started.is_none() {
                            execution.budget_wait_started = Some(now());
                        }
                    }
                    update_record(child, json!(child_instance));
                }
            }
            "workflow.resume" => instance.paused = false,
            "workflow.cancel" => {
                instance.cancel_requested = true;
                instance.paused = false;
                let descendants = descendant_instances(state, &row.id);
                for child in state
                    .governance
                    .iter_mut()
                    .filter(|r| descendants.contains(&r.id))
                {
                    let mut data = child.data.clone();
                    data["cancel_requested"] = json!(true);
                    data["paused"] = json!(false);
                    update_record(child, data);
                }
            }
            method => {
                let key = text(p, "step")?;
                let index = instance
                    .steps
                    .iter()
                    .position(|s| s.step == key)
                    .ok_or_else(|| err("not_found", "步骤不存在"))?;
                let step = instance
                    .plan
                    .document
                    .steps
                    .iter()
                    .find(|s| s.key == key)
                    .unwrap()
                    .clone();
                let execution = instance.steps[index].clone();
                match method {
                    "workflow.step.confirm" | "workflow.step.approve" => {
                        let manual = method == "workflow.step.approve";
                        require(
                            matches!(
                                execution.status,
                                StepExecutionStatus::WaitingConfirmation
                                    | StepExecutionStatus::AwaitingAcceptance
                            ),
                            "步骤不在等待确认",
                        )?;
                        require(!instance.cancel_requested, "流程已请求停止，不能批准结果")?;
                        if manual {
                            // Other steps may advance concurrently; this decision is bound to
                            // the exact execution, inputs and outputs below, not their revisions.
                            require(p["execution_id"] == execution.id, "步骤执行版本不匹配")?;
                            text(p, "reason")?;
                        }
                        require(
                            p["input_digest"] == execution.input_digest
                                && p["outputs"] == json!(execution.outputs),
                            "确认内容版本不匹配",
                        )?;
                        let valid = step.completion.evaluate(&facts(state, &instance)?);
                        if !valid && !manual {
                            require(
                                step.allow_completion_override && p["override_completion"] == true,
                                "完成条件尚未满足，当前配置不允许直接通过",
                            )?;
                            text(p, "reason")?;
                        }
                        state.governance.push(record("workflow_confirmation",execution.id.clone(),row.scope.clone(),"人工确认".into(),1,None,
                            json!({"execution_id":execution.id,"actor":{"kind":"human"},"input_digest":execution.input_digest,"outputs":execution.outputs,"reason":p["reason"],"override_completion":!valid,"manual_approval":manual}),true));
                    }
                    "workflow.step.reconcile" => {
                        require(
                            execution.status == StepExecutionStatus::NeedsReconciliation,
                            "步骤无需核对",
                        )?;
                        if let Some(call) = execution.tool_call_id.as_ref().and_then(|id| {
                            state
                                .governance
                                .iter_mut()
                                .find(|r| r.id == *id && r.kind == "platform_call")
                        }) {
                            require(
                                call.data["status"] == "needs_reconciliation",
                                "平台操作状态已变化",
                            )?;
                            let mut data = call.data.clone();
                            data["status"] = json!("running");
                            update_record(call, data);
                        }
                        instance.steps[index].status = StepExecutionStatus::Running;
                    }
                    "workflow.step.input" => {
                        require(
                            execution.status == StepExecutionStatus::WaitingInput,
                            "步骤不在等待输入",
                        )?;
                        require(
                            execution.sequence < step.budget.max_turns,
                            "步骤已达到轮次上限",
                        )?;
                        let supplemental = text(p, "input")?;
                        let current = &mut instance.steps[index];
                        current.sequence += 1;
                        current.turn_id = None;
                        current.status = StepExecutionStatus::Running;
                        let input = task_input(
                            &instance,
                            &step,
                            &instance.steps[index],
                            Some(&supplemental),
                        )?;
                        intent(
                            state,
                            &instance.steps[index],
                            &row.scope,
                            "conversation.send",
                            execution.session_id.as_deref().unwrap(),
                            json!({"client_message_id":format!("{}:{}",execution.id,instance.steps[index].sequence),"input":input}),
                        );
                    }
                    _ => {
                        require(
                            !state.executions.iter().any(|e| {
                                Some(&e.session_id) == execution.session_id.as_ref()
                                    && !e.writer_revoked
                            }),
                            "请先停止原执行者",
                        )?;
                        require(
                            !state.turns.iter().any(|t| {
                                Some(&t.session_id) == execution.session_id.as_ref()
                                    && t.status.active()
                            }),
                            "原执行结果需要先核对",
                        )?;
                        let rework = method == "workflow.step.rework";
                        require(
                            rework
                                || matches!(
                                    execution.status,
                                    StepExecutionStatus::Failed | StepExecutionStatus::Cancelled
                                ),
                            "此步骤不能技术重试",
                        )?;
                        require(
                            rework || execution.sequence <= step.budget.retry_limit,
                            "已达到重试上限",
                        )?;
                        let invalidated = instance
                            .plan
                            .document
                            .impacted_nodes(&BTreeSet::from([key.clone()]));
                        let affected_ids = instance
                            .steps
                            .iter()
                            .filter(|e| e.step == key || (rework && invalidated.contains(&e.step)))
                            .map(|e| e.id.clone())
                            .collect();
                        let checkpoint = if rework {
                            rework_workspace(state, &row.id, &instance, &execution, &step)?
                        } else {
                            execution.rework_workspace.clone()
                        };
                        let child_ids = instances_for_executions(state, &affected_ids);
                        for child in state
                            .governance
                            .iter()
                            .filter(|r| child_ids.contains(&r.id))
                        {
                            let child: ConfiguredWorkflowInstance = child.decode()?;
                            require(
                                !child.steps.iter().any(|e| active_execution(state, e)),
                                "子流程仍有活动执行者或未核对的副作用，请先停止并核对",
                            )?;
                        }
                        for child in state
                            .governance
                            .iter_mut()
                            .filter(|r| child_ids.contains(&r.id))
                        {
                            let mut child_data = child.data.clone();
                            child_data["cancel_requested"] = json!(true);
                            child_data["paused"] = json!(false);
                            update_record(child, child_data);
                        }
                        for current in &mut instance.steps {
                            if current.step == key
                                || (rework && invalidated.contains(&current.step))
                            {
                                require(
                                    !active_execution(state, current),
                                    "后继步骤的执行者或工具操作尚未停止并核对",
                                )?;
                                let previous_execution_id = current.id.clone();
                                state.governance.push(record(
                                    "workflow_execution_history",
                                    current.id.clone(),
                                    row.scope.clone(),
                                    step.name.clone(),
                                    1,
                                    None,
                                    json!(current),
                                    true,
                                ));
                                current.id = id();
                                let definition = instance
                                    .plan
                                    .document
                                    .steps
                                    .iter()
                                    .find(|s| s.key == current.step)
                                    .unwrap();
                                current.plan_revision_id = instance.active_plan_revision_id.clone();
                                current.workflow_revision_id = instance.workflow_ref.clone();
                                current.node_id = definition.node_id.clone();
                                current.contract_digest = configuration_hash(definition);
                                current.inputs.clear();
                                current.rework_workspace = checkpoint.clone();
                                if rework {
                                    let outputs = if current.outputs.is_empty() {
                                        state
                                            .governance
                                            .iter()
                                            .rev()
                                            .filter(|r| {
                                                r.kind == "workflow_execution_history"
                                                    && r.data["workflow_instance_id"]
                                                        == current.workflow_instance_id
                                                    && r.data["step"] == current.step
                                            })
                                            .filter_map(|r| r.decode::<StepExecution>().ok())
                                            .filter(|old| !old.outputs.is_empty())
                                            .max_by_key(|old| {
                                                (old.round, old.sequence, old.finished)
                                            })
                                            .map(|old| old.outputs)
                                            .unwrap_or_default()
                                    } else {
                                        current.outputs.clone()
                                    };
                                    current.rework = Some(StepReworkContext {
                                        previous_execution_id,
                                        reason: p["reason"]
                                            .as_str()
                                            .unwrap_or("workflow.step.rework")
                                            .into(),
                                        outputs,
                                    });
                                }

                                current.input_digest.clear();
                                current.agent_revision_id = None;
                                current.round += u64::from(rework);
                                current.sequence = if rework { 1 } else { current.sequence + 1 };
                                current.status = StepExecutionStatus::Queued;
                                current.session_id = None;
                                current.turn_id = None;
                                current.tool_call_id = None;
                                current.outputs.clear();
                                current.started = None;
                                current.finished = None;
                                current.budget_wait_started = None;
                                current.budget_waited_ms = 0;
                                current.reason.clear();
                            }
                        }
                        instance.status = StepExecutionStatus::Running;
                    }
                }
            }
        }
        instance.control_version += 1;
        update_record(
            state
                .governance
                .iter_mut()
                .find(|r| r.id == row.id)
                .unwrap(),
            json!(instance),
        );
        if [
            "workflow.step.retry",
            "workflow.step.rework",
            "workflow.step.reconcile",
        ]
        .contains(&method)
        {
            let mut parent = instance.parent_execution_id.clone();
            while let Some(execution_id) = parent {
                let Some(parent_row) = state
                    .governance
                    .iter()
                    .find(|r| {
                        r.kind == "workflow_instance"
                            && r.data["steps"].as_array().is_some_and(|steps| {
                                steps.iter().any(|step| step["id"] == execution_id)
                            })
                    })
                    .cloned()
                else {
                    break;
                };
                let mut data: ConfiguredWorkflowInstance = parent_row.decode()?;
                if data.cancel_requested || data.status == StepExecutionStatus::Cancelled {
                    break;
                }
                let execution = data
                    .steps
                    .iter_mut()
                    .find(|step| step.id == execution_id)
                    .unwrap();
                if matches!(
                    execution.status,
                    StepExecutionStatus::Failed | StepExecutionStatus::NeedsReconciliation
                ) {
                    execution.status = StepExecutionStatus::Running;
                    execution.reason.clear();
                    execution.finished = None;
                    data.status = StepExecutionStatus::Running;
                    update_record(
                        state
                            .governance
                            .iter_mut()
                            .find(|r| r.id == parent_row.id)
                            .unwrap(),
                        json!(data),
                    );
                }
                parent = data.parent_execution_id;
            }
        }
        Ok(Some(json!({"id":row.id,"accepted":true})))
    }

    pub fn evaluate_configured_workflows(&self) -> Result<()> {
        self.store.transact(None, &mut |state| {
            let rows: Vec<_> = state
                .governance
                .iter()
                .filter(|r| r.kind == "workflow_instance" && r.data["steps"].is_array())
                .cloned()
                .collect();
            for row in rows {
                let mut instance: ConfiguredWorkflowInstance = row.decode()?;
                if matches!(
                    instance.status,
                    StepExecutionStatus::Completed | StepExecutionStatus::Cancelled
                ) {
                    continue;
                }
                if instance.cancel_requested {
                    for execution in &mut instance.steps {
                        if matches!(
                            execution.status,
                            StepExecutionStatus::Completed
                                | StepExecutionStatus::Skipped
                                | StepExecutionStatus::Cancelled
                        ) {
                            continue;
                        }
                        if let Some(session) = &execution.session_id {
                            for turn in state
                                .turns
                                .iter_mut()
                                .filter(|t| t.session_id == *session && t.status.active())
                            {
                                turn.cancel_requested = true;
                                turn.revision += 1;
                                if turn.status == TurnStatus::Queued {
                                    turn.status = TurnStatus::Cancelled;
                                    turn.finished = Some(now());
                                }
                            }
                        }
                        if let Some(call) = execution
                            .tool_call_id
                            .as_ref()
                            .and_then(|id| state.governance.iter_mut().find(|r| r.id == *id))
                        {
                            if !["succeeded", "failed", "cancelled"]
                                .contains(&call.data["status"].as_str().unwrap_or(""))
                            {
                                let mut data = call.data.clone();
                                data["cancel_requested"] = json!(true);
                                update_record(call, data);
                            }
                        }
                        let writer = state.executions.iter().any(|e| {
                            Some(&e.session_id) == execution.session_id.as_ref()
                                && !e.writer_revoked
                        });
                        let active = state.turns.iter().any(|t| {
                            Some(&t.session_id) == execution.session_id.as_ref()
                                && t.status.active()
                        });
                        let tool_active = execution
                            .tool_call_id
                            .as_ref()
                            .and_then(|id| state.configuration(id).ok())
                            .is_some_and(|r| {
                                !["succeeded", "failed", "cancelled"]
                                    .contains(&r.data["status"].as_str().unwrap_or(""))
                            });
                        if !writer && !active && !tool_active {
                            execution.status = StepExecutionStatus::Cancelled;
                            execution.finished = Some(now());
                        }
                    }
                    if instance.steps.iter().all(|e| {
                        matches!(
                            e.status,
                            StepExecutionStatus::Cancelled
                                | StepExecutionStatus::Completed
                                | StepExecutionStatus::Skipped
                        )
                    }) {
                        instance.status = StepExecutionStatus::Cancelled;
                    }
                    update_record(
                        state
                            .governance
                            .iter_mut()
                            .find(|r| r.id == row.id)
                            .unwrap(),
                        json!(instance),
                    );
                    continue;
                }
                if let Some(parent) = &instance.parent_execution_id {
                    let group = state
                        .governance
                        .iter()
                        .find(|r| r.kind == "workflow_children" && r.definition_id == *parent)
                        .ok_or_else(|| err("invalid_workflow", "子流程集合来源缺失"))?;
                    let _parent_row = state
                        .governance
                        .iter()
                        .find(|r| {
                            r.kind == "workflow_instance"
                                && r.data["steps"]
                                    .as_array()
                                    .is_some_and(|steps| steps.iter().any(|s| s["id"] == *parent))
                        })
                        .ok_or_else(|| err("invalid_workflow", "子流程父节点不存在"))?;
                    if instance.status == StepExecutionStatus::Queued {
                        let active = group.data["children"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|id| id.as_str())
                            .filter_map(|id| state.configuration(id).ok())
                            .filter(|r| r.data["status"] == "running")
                            .count();
                        if active as u64 >= group.data["max_concurrency"].as_u64().unwrap_or(1) {
                            continue;
                        }
                    }
                }
                let steps = instance.plan.document.steps.clone();
                for step in &steps {
                    let checkpoint = state.clone();
                    let previous = instance.clone();
                    if let Err(error) =
                        self.advance_configured_step(state, &row, &mut instance, step)
                    {
                        *state = checkpoint;
                        instance = previous;
                        let execution = instance
                            .steps
                            .iter_mut()
                            .find(|e| e.step == step.key)
                            .unwrap();
                        execution.status = StepExecutionStatus::Failed;
                        execution.reason = format!("{}: {}", error.code, error.message);
                        execution.finished = Some(now());
                    }
                }
                let current_facts = facts(state, &instance)?;
                instance.status =
                    if instance.steps.iter().all(|e| {
                        matches!(
                            e.status,
                            StepExecutionStatus::Completed | StepExecutionStatus::Skipped
                        )
                    }) && instance.plan.document.completion.evaluate(&current_facts)
                        && state.governance.iter().any(|r| {
                            r.kind == "workflow_completion"
                                && r.definition_id == row.id
                                && r.data["plan_revision_id"] == instance.active_plan_revision_id
                        })
                    {
                        StepExecutionStatus::Completed
                    } else if instance
                        .steps
                        .iter()
                        .any(|s| s.status == StepExecutionStatus::NeedsReconciliation)
                    {
                        StepExecutionStatus::NeedsReconciliation
                    } else if instance
                        .steps
                        .iter()
                        .any(|s| s.status == StepExecutionStatus::Failed)
                    {
                        StepExecutionStatus::Failed
                    } else if instance.steps.iter().any(|s| s.started.is_some()) {
                        StepExecutionStatus::Running
                    } else {
                        StepExecutionStatus::Queued
                    };
                let data = json!(instance);
                if instance.parent_execution_id.is_none() {
                    if let Some(feature) = state
                        .features
                        .iter_mut()
                        .find(|f| f.id == instance.owner_id)
                    {
                        let status = if instance.status == StepExecutionStatus::Completed {
                            Status::Completed
                        } else {
                            Status::Running
                        };
                        if feature.status != status {
                            feature.status = status;
                            feature.revision += 1;
                        }
                    }
                }
                if row.data != data {
                    update_record(
                        state
                            .governance
                            .iter_mut()
                            .find(|r| r.id == row.id)
                            .unwrap(),
                        data,
                    );
                    event(
                        state,
                        &row.id,
                        "workflow.updated",
                        &format!("{:?}", instance.status),
                    );
                }
            }
            Ok(Value::Null)
        })?;
        Ok(())
    }

    pub(crate) fn advance_configured_step(
        &self,
        state: &mut EngineeringState,
        row: &ConfigurationRecord,
        instance: &mut ConfiguredWorkflowInstance,
        step: &StepDefinition,
    ) -> Result<()> {
        let index = instance
            .steps
            .iter()
            .position(|e| e.step == step.key)
            .unwrap();
        let mut execution = instance.steps[index].clone();
        if state
            .governance
            .iter()
            .any(|r| r.kind == "workflow_stop" && r.definition_id == execution.id)
        {
            if !active_execution(state, &execution)
                && execution.status != StepExecutionStatus::NeedsReconciliation
            {
                execution.status = StepExecutionStatus::Cancelled;
                execution.finished = Some(now());
                instance.steps[index] = execution;
            }
            return Ok(());
        }
        update_budget_wait(state, &mut execution);
        instance.steps[index] = execution.clone();
        if matches!(
            execution.status,
            StepExecutionStatus::Completed
                | StepExecutionStatus::Failed
                | StepExecutionStatus::Cancelled
                | StepExecutionStatus::Skipped
                | StepExecutionStatus::WaitingInput
                | StepExecutionStatus::NeedsReconciliation
        ) {
            return Ok(());
        }
        let current_facts = facts(state, &instance)?;
        if matches!(
            execution.status,
            StepExecutionStatus::WaitingConfirmation | StepExecutionStatus::AwaitingAcceptance
        ) {
            return Ok(());
        }
        if execution.status == StepExecutionStatus::Queued {
            if state.workflow_instance_suspended(&row.id)
                || !state.governance.iter().any(|r| {
                    r.kind == "workflow_dispatch"
                        && r.definition_id == execution.id
                        && r.data["plan_revision_id"] == instance.active_plan_revision_id
                })
            {
                return Ok(());
            }
            if !step.dependencies.iter().all(|key| {
                instance.steps.iter().any(|e| {
                    &e.step == key
                        && matches!(
                            e.status,
                            StepExecutionStatus::Completed | StepExecutionStatus::Skipped
                        )
                })
            }) {
                return Ok(());
            }
            if !step.condition.evaluate(&current_facts) {
                execution.status = StepExecutionStatus::Skipped;
                instance.steps[index] = execution;
                return Ok(());
            }
            execution.inputs = material_inputs(step, &current_facts)?;
            if let Some(name) = &step.access.workspace_input {
                if let Some(value) = execution.inputs.get(name).cloned() {
                    execution
                        .inputs
                        .insert(name.clone(), checkpoint_workspace(&execution, &value));
                }
            }
            execution.input_digest = configuration_hash(&execution.inputs);
            execution.started = Some(now());
            match &step.executor {
                StepExecutor::Agent { .. } => {
                    let role = crate::configuration::role_record(
                        state,
                        instance.plan.document.role(step).unwrap(),
                    )?;
                    let agent = instance
                        .role_agent_bindings
                        .get(&role.id)
                        .ok_or_else(|| err("configuration_required", "缺少 Agent 绑定"))?;
                    execution.agent_revision_id = Some(agent.clone());
                    execution.status = StepExecutionStatus::Running;
                    intent(
                        state,
                        &execution,
                        &row.scope,
                        "session.open",
                        &instance.owner_id,
                        json!({"agent":agent,"work_type":"workflow","force_new":true,"configured_execution_id":execution.id}),
                    );
                }
                StepExecutor::Control => {
                    execution.finished = Some(now());
                    execution.status = if step.confirmation == ConfirmationPolicy::Human {
                        StepExecutionStatus::WaitingConfirmation
                    } else {
                        StepExecutionStatus::AwaitingAcceptance
                    };
                }
                StepExecutor::Tool { .. } => {
                    execution.status = StepExecutionStatus::Running;
                    intent(
                        state,
                        &execution,
                        &row.scope,
                        "workflow.tool.execute",
                        &instance.owner_id,
                        json!({"execution_id":execution.id}),
                    );
                }
                StepExecutor::Workflow {
                    workflow_ref,
                    foreach,
                    work_binding,
                } => {
                    let child = instance
                        .plan
                        .definitions
                        .get(workflow_ref)
                        .ok_or_else(|| err("configuration_required", "子流程不在冻结闭包中"))?
                        .clone();
                    let mut child_doc = state.resolve_workflow_source(&child)?;
                    child_doc.assign_node_ids(&child.id);
                    let members = if let Some(collection) = foreach {
                        let values = current_facts
                            .value(&collection.source)
                            .and_then(Value::as_array)
                            .ok_or_else(|| err("invalid_workflow", "集合输入需要数组"))?;
                        require(
                            values.len() as u64 >= collection.min_items
                                && values.len() <= WORKFLOW_STEP_LIMIT,
                            "集合成员数量不满足配置",
                        )?;
                        values.iter().cloned().map(Some).collect::<Vec<_>>()
                    } else {
                        vec![None]
                    };
                    let existing: usize = state
                        .governance
                        .iter()
                        .filter(|r| {
                            r.kind == "workflow_instance"
                                && r.data["root_instance_id"] == instance.root_instance_id
                        })
                        .map(|r| r.data["steps"].as_array().map_or(0, Vec::len))
                        .sum();
                    require(
                        work_binding.is_some()
                            || existing + members.len() * child_doc.steps.len()
                                <= WORKFLOW_STEP_LIMIT,
                        "集合展开超过 1000 个步骤",
                    )?;
                    let mut children = vec![];
                    for (item, member) in members.into_iter().enumerate() {
                        let child_id = id();
                        let mut inputs = execution.inputs.clone();
                        if let (Some(collection), Some(member)) = (foreach, member) {
                            inputs.insert(collection.item_input.clone(), member);
                        }
                        let mut child_owner = instance.owner_id.clone();
                        let mut child_bindings = instance.role_agent_bindings.clone();
                        let mut child_plan = instance.plan.clone();
                        let mut effective_child = child.clone();
                        let mut effective_doc = child_doc.clone();
                        if let Some(binding) = work_binding {
                            let value = inputs
                                .get(&binding.input)
                                .and_then(|v| v.pointer(&binding.pointer))
                                .ok_or_else(|| {
                                    err("work_configuration_required", "调用缺少类型化工作引用")
                                })?;
                            let work: WorkReference = serde_json::from_value(value.clone())
                                .map_err(|e| err("invalid_params", format!("工作引用无效: {e}")))?;
                            let work_scope = work.scope(state)?;
                            require(
                                work_scope.project() == Some(instance.project_id.as_str()),
                                "子工作跨项目",
                            )?;
                            if let WorkReference::Task { id } = &work {
                                require(
                                    state.task(id)?.feature_id == instance.owner_id
                                        || instance.owner_id == *id,
                                    "Task 不属于当前父工作",
                                )?;
                            }
                            child_owner = work.id().into();
                            require(
                                !state.governance.iter().any(|r| {
                                    r.kind == "workflow_instance"
                                        && r.data["owner_id"] == child_owner
                                        && !matches!(
                                            r.data["status"].as_str(),
                                            Some("completed" | "cancelled")
                                        )
                                }),
                                "该工作已有活动流程，不能重复或改挂调用",
                            )?;
                            let inherited =
                                crate::standards::work_configuration(state, &child_owner).cloned();
                            let fallback = json!({"standard_ref":instance.plan.standard_ref,"workflow_ref":child.id,"role_agent_bindings":instance.role_agent_bindings,"permission_context_ref":instance.plan.permission_context_ref});
                            let selection = if let Some(selection) = inherited {
                                selection
                            } else {
                                crate::standards::select_work_configuration(
                                    state,
                                    &child_owner,
                                    &fallback,
                                    None,
                                )?
                            };
                            let selected =
                                crate::standards::selection_state(state, &selection.data);
                            effective_child = reference(
                                state,
                                &text(&selection.data, "workflow_ref")?,
                                "workflow",
                                &work_scope,
                            )?
                            .clone();
                            standard_entry(
                                state,
                                &text(&selection.data, "standard_ref")?,
                                &effective_child.id,
                                &work_scope,
                            )?;
                            child_bindings = serde_json::from_value(
                                selection.data["role_agent_bindings"].clone(),
                            )
                            .map_err(|e| err("invalid_configuration", e.to_string()))?;
                            child_plan = self.compile_configured_workflow(
                                &selected,
                                &effective_child,
                                &work_scope,
                                Some(&child_bindings),
                            )?;
                            child_plan.standard_ref = Some(text(&selection.data, "standard_ref")?);
                            child_plan.work_configuration_ref = Some(selection.id.clone());
                            effective_doc = child_plan.document.clone();
                            crate::standards::check_invocation_contract(
                                &child_doc,
                                &effective_doc,
                            )?;
                        }
                        let active_count: usize = state
                            .governance
                            .iter()
                            .filter(|r| {
                                r.kind == "workflow_instance"
                                    && r.data["root_instance_id"] == instance.root_instance_id
                            })
                            .map(|r| r.data["steps"].as_array().map_or(0, Vec::len))
                            .sum();
                        require(
                            active_count + effective_doc.steps.len() <= WORKFLOW_STEP_LIMIT,
                            "实际子流程展开超过 1000 个步骤",
                        )?;
                        let mut depth = 1;
                        let mut ancestor = instance.parent_execution_id.clone();
                        while let Some(parent) = ancestor {
                            depth += 1;
                            require(depth <= WORKFLOW_DEPTH_LIMIT, "实际调用深度超过 8")?;
                            ancestor = state
                                .governance
                                .iter()
                                .find(|r| {
                                    r.kind == "workflow_instance"
                                        && r.data["steps"].as_array().is_some_and(|steps| {
                                            steps.iter().any(|e| e["id"] == parent)
                                        })
                                })
                                .and_then(|r| {
                                    r.data["parent_execution_id"].as_str().map(str::to_owned)
                                });
                        }
                        let child_doc = &effective_doc;
                        let child = &effective_child;
                        for (name, port) in &child_doc.inputs {
                            if port.required {
                                require(
                                    inputs.contains_key(name),
                                    format!("子流程缺少输入 {name}"),
                                )?;
                            }
                            if let Some(value) = inputs.get(name) {
                                self.validate_configured_value(
                                    &child_plan
                                        .definitions
                                        .get(&port.delivery_ref)
                                        .ok_or_else(|| {
                                            err(
                                                "configuration_required",
                                                "子流程输入类型不在闭包中",
                                            )
                                        })?
                                        .decode()?,
                                    value,
                                )?;
                            }
                        }
                        let child_steps = child_doc
                            .steps
                            .iter()
                            .map(|step| StepExecution {
                                plan_revision_id: String::new(),
                                node_id: step.node_id.clone(),
                                contract_digest: configuration_hash(&step),
                                id: id(),
                                workflow_instance_id: child_id.clone(),
                                workflow_revision_id: child.id.clone(),
                                path: format!("{}/{item}/{}", execution.path, step.key),
                                step: step.key.clone(),
                                round: execution.round,
                                sequence: 1,
                                status: StepExecutionStatus::Queued,
                                input_digest: String::new(),
                                budget_wait_started: None,
                                budget_waited_ms: 0,
                                rework_workspace: None,
                                rework: None,
                                inputs: BTreeMap::new(),
                                outputs: BTreeMap::new(),
                                agent_revision_id: None,
                                session_id: None,
                                turn_id: None,
                                tool_call_id: None,
                                started: None,
                                finished: None,
                                reason: String::new(),
                            })
                            .collect();
                        let mut plan = child_plan;
                        plan.workflow_revision_id = child.id.clone();
                        plan.permission_workflow_ref = Some(child.id.clone());
                        plan.document = (*child_doc).clone();
                        let mut child_instance = ConfiguredWorkflowInstance {
                            active_plan_revision_id: String::new(),
                            control_version: 1,
                            schema_version: WORKFLOW_INSTANCE_SCHEMA,
                            owner_id: child_owner.clone(),
                            workflow_ref: child.id.clone(),
                            project_id: instance.project_id.clone(),
                            plan,
                            role_agent_bindings: child_bindings,
                            inputs,
                            steps: child_steps,
                            status: StepExecutionStatus::Queued,
                            paused: false,
                            parent_execution_id: Some(execution.id.clone()),
                            root_instance_id: instance.root_instance_id.clone(),
                            cancel_requested: false,
                        };
                        crate::orchestration::freeze_plan(
                            state,
                            &mut child_instance,
                            &row.scope,
                            "child",
                        )?;
                        let mut child_row = record(
                            "workflow_instance",
                            child_owner,
                            row.scope.clone(),
                            format!("{} · {}", step.name, item + 1),
                            1,
                            None,
                            json!(child_instance),
                            false,
                        );
                        child_row.id = child_id.clone();
                        state.governance.push(record("workflow_invocation",execution.id.clone(),row.scope.clone(),step.name.clone(),1,None,
                            json!({"parent_execution_id":execution.id,"parent_plan_revision_id":execution.plan_revision_id,"child_instance_id":child_id,"initial_plan_revision_id":child_instance.active_plan_revision_id,"owner_id":child_instance.owner_id,"member_identity":if work_binding.is_some(){child_instance.owner_id.clone()}else{item.to_string()},"standard_ref":child_instance.plan.standard_ref,"workflow_ref":child_instance.workflow_ref,"work_configuration_ref":child_instance.plan.work_configuration_ref,"input_digest":configuration_hash(&child_instance.inputs)}),true));
                        state.governance.push(child_row);
                        children.push(child_id);
                    }
                    state.governance.push(record("workflow_children",execution.id.clone(),row.scope.clone(),step.name.clone(),1,None,
                                    json!({"children":children,"foreach":foreach.is_some(),"max_concurrency":foreach.as_ref().map_or(1,|c|c.max_concurrency)}),true));
                    execution.status = StepExecutionStatus::Running;
                }
            }
        } else if matches!(step.executor, StepExecutor::Workflow { .. }) {
            let group = state
                .governance
                .iter()
                .find(|r| r.kind == "workflow_children" && r.definition_id == execution.id)
                .ok_or_else(|| err("invalid_workflow", "子流程成员来源缺失"))?;
            let children = group.data["children"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|id| id.as_str())
                .map(|id| {
                    state
                        .configuration(id)
                        .and_then(|row| row.decode::<ConfiguredWorkflowInstance>())
                })
                .collect::<Result<Vec<_>>>()?;
            if children.iter().any(|child| {
                child.steps.iter().any(|s| {
                    matches!(
                        s.status,
                        StepExecutionStatus::Failed | StepExecutionStatus::Cancelled
                    )
                })
            }) {
                execution.status = StepExecutionStatus::Failed;
                execution.reason = "子流程执行失败".into();
            } else if children
                .iter()
                .all(|child| child.status == StepExecutionStatus::Completed)
            {
                let is_collection = group.data["foreach"] == true;
                let mut outputs = serde_json::Map::new();
                for name in step.outputs.keys() {
                    let values =
                        children
                            .iter()
                            .map(|child| {
                                let child_facts = facts(state, child)?;
                                let source =
                                    child.plan.document.outputs.get(name).ok_or_else(|| {
                                        err("invalid_workflow", "子流程未声明输出")
                                    })?;
                                child_facts
                                    .value(source)
                                    .cloned()
                                    .ok_or_else(|| err("output_invalid", "子流程输出尚未就绪"))
                            })
                            .collect::<Result<Vec<_>>>()?;
                    outputs.insert(
                        name.clone(),
                        if is_collection {
                            json!(values)
                        } else {
                            values
                                .into_iter()
                                .next()
                                .ok_or_else(|| err("output_invalid", "子流程没有执行成员"))?
                        },
                    );
                }
                if let Err(error) = self.accept_configured_outputs(
                    state,
                    &row,
                    step,
                    &mut execution,
                    &json!({"status":"completed","summary":"子流程实际完成","outputs":outputs}),
                ) {
                    execution.status = StepExecutionStatus::Failed;
                    execution.reason = error.message;
                }
            }
        } else if matches!(step.executor, StepExecutor::Tool { .. }) {
            let request = state.governance.iter().find(|r| {
                r.kind == "workflow_intent"
                    && r.definition_id == execution.id
                    && r.data["method"] == "workflow.tool.execute"
            });
            if let Some(request) = request {
                if request.data["status"] == "failed" {
                    execution.status = StepExecutionStatus::Failed;
                    execution.reason = request.data["error"]
                        .as_str()
                        .unwrap_or("工具准备失败")
                        .into();
                } else if let Some(call_id) = request.data["result"]["id"].as_str() {
                    execution.tool_call_id = Some(call_id.into());
                    let call = state.configuration(call_id)?;
                    if call.kind == "platform_call" {
                        if call.data["status"] == "succeeded" {
                            let outputs = mapped_tool_outputs(step, &call.data["receipt"])?;
                            if let Err(error)=self.accept_configured_outputs(state,&row,step,&mut execution,&json!({"status":"completed","summary":"平台工具实际完成","outputs":outputs})) {execution.status=StepExecutionStatus::Failed;execution.reason=error.message;}
                        } else if call.data["status"] == "needs_reconciliation" {
                            execution.status = StepExecutionStatus::NeedsReconciliation;
                            execution.reason = "平台操作副作用需要核对".into();
                        } else if call.data["status"] == "failed" {
                            execution.status = StepExecutionStatus::Failed;
                            execution.reason = call.data["error"]["message"]
                                .as_str()
                                .unwrap_or("平台工具执行失败")
                                .into();
                        }
                        instance.steps[index] = execution;
                        return Ok(());
                    }

                    if call.data["status"] == "needs_reconciliation" {
                        execution.status = StepExecutionStatus::NeedsReconciliation;
                        execution.reason = "工具副作用需要核对".into();
                    } else if ["succeeded", "failed", "cancelled"]
                        .contains(&call.data["status"].as_str().unwrap_or(""))
                    {
                        let receipt: ToolReceipt =
                            serde_json::from_value(call.data["receipt"].clone())
                                .map_err(|e| err("adapter_contract", e.to_string()))?;
                        require(
                            receipt.invocation_id == call_id
                                && call.data["invocation"]["operation_id"] == execution.id,
                            "工具回执与步骤归属不匹配",
                        )?;
                        if receipt.success {
                            let value =
                                receipt.structured_content.unwrap_or(json!(receipt.content));
                            let outputs = mapped_tool_outputs(step, &value)?;
                            if let Err(error)=self.accept_configured_outputs(state,&row,step,&mut execution,&json!({"status":"completed","summary":"工具执行完成","outputs":outputs})) {
                                            execution.status=StepExecutionStatus::Failed;execution.reason=error.message;
                                        }
                        } else {
                            execution.status = StepExecutionStatus::Failed;
                            execution.reason = "工具执行失败".into();
                        }
                    }
                }
            }
        } else if matches!(step.executor, StepExecutor::Agent { .. }) {
            if execution.session_id.is_none() {
                let result = state
                    .governance
                    .iter()
                    .find(|r| {
                        r.kind == "workflow_intent"
                            && r.definition_id == execution.id
                            && r.data["method"] == "session.open"
                    })
                    .cloned();
                if let Some(result) = result {
                    if result.data["status"] == "failed" {
                        execution.status = StepExecutionStatus::Failed;
                        execution.reason = result.data["error"]
                            .as_str()
                            .unwrap_or("会话建立失败")
                            .into();
                    } else if let Some(session) = result.data["result"]["id"].as_str() {
                        execution.session_id = Some(session.into());
                        let input = task_input(&instance, step, &execution, None)?;
                        intent(
                            state,
                            &execution,
                            &row.scope,
                            "conversation.send",
                            session,
                            json!({"client_message_id":format!("{}:{}",execution.id,execution.sequence),"input":input}),
                        );
                    }
                }
            } else {
                if let Some(failed) = state.governance.iter().find(|r| {
                    r.kind == "workflow_intent"
                        && r.definition_id == execution.id
                        && r.data["method"] == "conversation.send"
                        && r.data["status"] == "failed"
                        && r.data["command"]["payload"]["client_message_id"]
                            == format!("{}:{}", execution.id, execution.sequence)
                }) {
                    execution.status = StepExecutionStatus::Failed;
                    execution.reason = failed.data["error"]
                        .as_str()
                        .unwrap_or("轮次派发失败")
                        .into();
                    execution.finished = Some(now());
                    instance.steps[index] = execution;
                    return Ok(());
                }
                if execution.turn_id.is_none() {
                    execution.turn_id = state
                        .governance
                        .iter()
                        .find(|r| {
                            r.kind == "workflow_intent"
                                && r.definition_id == execution.id
                                && r.data["method"] == "conversation.send"
                                && r.data["status"] == "completed"
                                && r.data["command"]["payload"]["client_message_id"]
                                    == format!("{}:{}", execution.id, execution.sequence)
                        })
                        .and_then(|r| r.data["result"]["id"].as_str())
                        .map(str::to_owned);
                }
                match execution_result(state, &execution) {
                    Ok(Some(result)) => match self.accept_configured_outputs(
                        state,
                        &row,
                        step,
                        &mut execution,
                        &result,
                    ) {
                        Ok(()) => {}
                        Err(error) => {
                            execution.status = StepExecutionStatus::Failed;
                            execution.reason = error.message;
                        }
                    },
                    Err(error) => {
                        execution.status = StepExecutionStatus::Failed;
                        execution.reason = error.message;
                    }
                    _ => {}
                }
                if execution.turn_id.as_ref().is_some_and(|id| {
                    state
                        .turns
                        .iter()
                        .any(|t| t.id == *id && t.status == TurnStatus::NeedsReconciliation)
                }) {
                    execution.status = StepExecutionStatus::NeedsReconciliation;
                    execution.reason = state
                        .conversation_events
                        .iter()
                        .rev()
                        .find(|event| {
                            Some(&event.turn_id) == execution.turn_id.as_ref()
                                && event.kind == ConversationEventKind::Status
                                && event.content.starts_with("needs_reconciliation:")
                        })
                        .map(|event| event.content.clone())
                        .unwrap_or_else(|| {
                            "原生执行结果需要核对，请先停止原执行者并处理会话恢复".into()
                        });
                }
            }
        }
        update_budget_wait(state, &mut execution);
        instance.steps[index] = execution;
        if instance.steps[index].status == StepExecutionStatus::Running
            && instance.steps[index].started.is_some_and(|started| {
                now()
                    .saturating_sub(started)
                    .saturating_sub(instance.steps[index].budget_waited_ms)
                    .saturating_sub(
                        instance.steps[index]
                            .budget_wait_started
                            .map_or(0, |wait| now().saturating_sub(wait)),
                    )
                    > step.budget.timeout_seconds * 1000
            })
        {
            if let Some(session) = &instance.steps[index].session_id {
                for turn in state
                    .turns
                    .iter_mut()
                    .filter(|t| t.session_id == *session && t.status.active())
                {
                    turn.cancel_requested = true;
                    turn.revision += 1;
                }
            }
            if let Some(call) = instance.steps[index]
                .tool_call_id
                .as_ref()
                .and_then(|id| state.governance.iter_mut().find(|r| r.id == *id))
            {
                let mut data = call.data.clone();
                data["cancel_requested"] = json!(true);
                update_record(call, data);
            }
            let child_ids = instances_for_executions(
                state,
                &BTreeSet::from([instance.steps[index].id.clone()]),
            );
            for child in state
                .governance
                .iter_mut()
                .filter(|r| child_ids.contains(&r.id))
            {
                let mut data = child.data.clone();
                data["cancel_requested"] = json!(true);
                data["paused"] = json!(false);
                update_record(child, data);
            }
            instance.steps[index].status = StepExecutionStatus::Failed;
            instance.steps[index].reason =
                "超过配置的执行时限，已请求停止；重试前须核对原执行者".into();
        }
        Ok(())
    }

    fn accept_configured_outputs(
        &self,
        state: &mut EngineeringState,
        row: &ConfigurationRecord,
        step: &StepDefinition,
        execution: &mut StepExecution,
        result: &Value,
    ) -> Result<()> {
        require(
            result["summary"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty()),
            "执行输出缺少摘要",
        )?;
        match result["status"].as_str() {
            Some("needs_input") => {
                execution.status = StepExecutionStatus::WaitingInput;
                execution.reason = result["summary"].as_str().unwrap().into();
                return Ok(());
            }
            Some("blocked") => {
                execution.status = StepExecutionStatus::Failed;
                execution.reason = result["summary"].as_str().unwrap().into();
                return Ok(());
            }
            Some("completed") => {}
            _ => return Err(err("output_invalid", "无效的 Agent 工作结论")),
        }
        let outputs = result["outputs"]
            .as_object()
            .ok_or_else(|| err("output_invalid", "outputs 需要对象"))?;
        require(
            outputs.keys().all(|name| step.outputs.contains_key(name)),
            "Agent 提交了未声明输出",
        )?;
        for (name, port) in &step.outputs {
            if port.required {
                require(outputs.contains_key(name), format!("缺少输出 {name}"))?;
            }
            if let Some(value) = outputs.get(name) {
                let instance: ConfiguredWorkflowInstance = row.decode()?;
                let definition: DeliveryTypeDefinition = instance
                    .plan
                    .definitions
                    .get(&port.delivery_ref)
                    .ok_or_else(|| err("configuration_required", "输出类型不在闭包中"))?
                    .decode()?;
                self.validate_configured_value(&definition, value)?;
                output_provenance(state, row, step, execution, name, &definition, value)?;
                if port.content_checks {
                    let expected = step
                        .spec_refs
                        .iter()
                        .map(|id| {
                            instance.plan.definitions[id]
                                .decode::<ConfiguredSpecification>()
                                .map(|s| s.checks)
                        })
                        .collect::<Result<Vec<_>>>()?
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>();
                    serde_json::from_value::<ContentCheckReport>(value.clone())
                        .map_err(|e| err("output_invalid", e.to_string()))?
                        .validate(&expected)?;
                }
            }
        }
        let owner = MaterialOwner::Project {
            id: row
                .scope
                .project()
                .ok_or_else(|| err("invalid_scope", "步骤需要项目归属"))?
                .into(),
        };
        for (name, value) in outputs {
            let instance: ConfiguredWorkflowInstance = row.decode()?;
            let definition: DeliveryTypeDefinition =
                instance.plan.definitions[&step.outputs[name].delivery_ref].decode()?;
            let provenance =
                output_provenance(state, row, step, execution, name, &definition, value)?;
            let body =
                serde_json::to_string(value).map_err(|e| err("serialization", e.to_string()))?;
            let artifact = self
                .artifacts
                .seal_scoped(&owner, "workflow-outputs", &body)?;
            let output = record(
                "workflow_output",
                execution.id.clone(),
                row.scope.clone(),
                name.clone(),
                1,
                None,
                json!({"execution_id":execution.id,"port":name,"delivery_ref":step.outputs[name].delivery_ref,"value":value,"artifact_id":artifact.id,"input_digest":execution.input_digest,"provenance":provenance}),
                true,
            );
            execution.outputs.insert(name.clone(), output.id.clone());
            state.artifacts.push(artifact);
            state.governance.push(output);
        }
        execution.finished = Some(now());
        execution.status = if step.confirmation == ConfirmationPolicy::Human {
            StepExecutionStatus::WaitingConfirmation
        } else {
            StepExecutionStatus::AwaitingAcceptance
        };
        execution.reason = result["summary"].as_str().unwrap().into();
        Ok(())
    }

    pub(crate) fn configured_runtime_query(
        &self,
        method: &str,
        p: &Value,
    ) -> Option<Result<Value>> {
        if !["query.workflow_instance", "query.execution_task"].contains(&method) {
            return None;
        }
        let state = match self.store.snapshot() {
            Ok(s) => s,
            Err(e) => return Some(Err(e)),
        };
        if method == "query.execution_task" {
            let row = state.governance.iter().find(|r| {
                [
                    "configured_execution_task",
                    "archived_configured_execution_task",
                ]
                .contains(&r.kind.as_str())
                    && p["turn_id"] == r.definition_id
            })?;
            return Some(Ok(
                json!({"configured":true,"archived":row.kind=="archived_configured_execution_task","task":row.data,"turn_id":row.definition_id}),
            ));
        }
        let row = if let Some(reference) = p["id"].as_str() {
            state
                .governance
                .iter()
                .find(|r| r.kind == "workflow_instance" && r.id == reference)
        } else {
            p["owner_id"]
                .as_str()
                .and_then(|owner| crate::standards::current_work_instance(&state, owner))
        }?;
        if let Some(project) = p["project_id"].as_str() {
            if row.scope.project() != Some(project) {
                return Some(Err(err("scope_violation", "工作流不属于当前项目")));
            }
        }
        let output_ids: BTreeSet<_> = row.data["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|s| {
                s["outputs"]
                    .as_object()
                    .into_iter()
                    .flat_map(|o| o.values())
            })
            .filter_map(Value::as_str)
            .collect();
        let outputs = state
            .governance
            .iter()
            .filter(|r| output_ids.contains(r.id.as_str()))
            .map(|r| (&r.id, &r.data))
            .collect::<BTreeMap<_, _>>();
        let executions: BTreeSet<_> = row.data["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s["id"].as_str())
            .collect();
        let children = state
            .governance
            .iter()
            .filter(|r| {
                r.kind == "workflow_instance"
                    && r.data["parent_execution_id"]
                        .as_str()
                        .is_some_and(|id| executions.contains(id))
            })
            .collect::<Vec<_>>();
        let instance: ConfiguredWorkflowInstance = match row.decode() {
            Ok(instance) => instance,
            Err(error) => return Some(Err(error)),
        };
        let current_facts = match facts(&state, &instance) {
            Ok(facts) => facts,
            Err(error) => return Some(Err(error)),
        };
        let completion_status = instance.steps.iter().map(|execution| {
            let step = instance.plan.document.steps.iter().find(|step| step.key == execution.step).unwrap();
            let confirmation = state.governance.iter().rev().find(|r| {
                r.kind == "workflow_confirmation" && r.definition_id == execution.id
                    && r.data["input_digest"] == execution.input_digest
                    && r.data["outputs"] == json!(execution.outputs)
            });
            (&execution.id, json!({"satisfied":step.completion.evaluate(&current_facts),"confirmation":confirmation.map(|r| &r.data)}))
        }).collect::<BTreeMap<_, _>>();
        let history=state.governance.iter().filter(|r|(r.kind=="workflow_plan_revision"&&(r.data["instance_id"]==row.id||r.id==instance.active_plan_revision_id||instance.steps.iter().any(|e|e.plan_revision_id==r.id)))||(r.kind=="workflow_execution_history"&&r.data["workflow_instance_id"]==row.id)||(r.kind=="workflow_change"&&r.definition_id==row.id)).map(|r| {
            let data=if r.kind=="workflow_plan_revision" {json!({"parent_revision_id":r.data["parent_revision_id"],"reason":r.data["reason"]})} else if r.kind=="workflow_change" {json!({"status":r.data["status"],"new_plan_revision_id":r.data["new_plan_revision_id"],"affected":r.data["preview"]["affected"],"bypassed":r.data["preview"]["bypassed"]})} else {r.data.clone()};
            let frozen_step=if r.kind=="workflow_execution_history" {state.configuration(r.data["plan_revision_id"].as_str().unwrap_or("")).ok().and_then(|p|p.data["plan"]["document"]["steps"].as_array()).and_then(|steps|steps.iter().find(|s|s["node_id"]==r.data["node_id"]))} else {None};
            json!({"id":r.id,"kind":r.kind,"name":r.name,"data":data,"frozen_step":frozen_step})
        }).collect::<Vec<_>>();
        Some(Ok(
            json!({"record":row,"runtime":row.data,"plan":row.data["plan"]["document"],"outputs":outputs,"children":children,"completion_status":completion_status,"history":history}),
        ))
    }
}
