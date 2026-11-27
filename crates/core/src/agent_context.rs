//! Durable context calls share Agent identity without requiring a Workflow caller.
use crate::governance::record;
use crate::*;

/// Rebuild an exact configuration reference or the declared kernel protocol.
/// A denied unbound tool retains its original source schema without publishing
/// a second mutable configuration body inside its runtime call.
pub fn context_tool_definition(
    state: &EngineeringState,
    reference: &str,
    name: &str,
    unbound: bool,
) -> Result<ToolDefinition> {
    if reference == AGENT_CONTEXT_PROTOCOL {
        require(!unbound, "内核上下文协议不能伪装为未绑定接入")?;
        return agent_context_tools()
            .into_iter()
            .find(|d| d.qualified_name == name)
            .ok_or_else(|| err("invalid_configuration", "上下文工具不属于原内核协议"));
    }
    let source = state.configuration(reference)?;
    if unbound {
        require(
            source.data["qualified_name"] == name && source.data["input_schema"].is_object(),
            "未绑定工具不属于原源修订",
        )?;
        return Ok(ToolDefinition {
            runtime: None,
            harness_provider: None,
            qualified_name: name.into(),
            implementation: "unbound".into(),
            handler: None,
            input_schema: source.data["input_schema"].clone(),
            output_schema: json!({"type":"object"}),
            capabilities: vec![],
        });
    }
    let definition: ToolDefinition = source.decode()?;
    require(
        definition.qualified_name == name,
        "上下文工具改变了原源身份",
    )?;
    Ok(definition)
}

pub(crate) struct ProviderContextDefinition {
    pub reference: Id,
    pub definition: ToolDefinition,
    pub rejection: Option<Error>,
}

impl Engine {
    pub(crate) fn provider_context_definition(
        &self,
        poll: &crate::provider_tools::ProviderPoll,
        observation: &ports::ConversationObservation,
    ) -> Result<Option<ProviderContextDefinition>> {
        let ports::ConversationObservation::ToolCall { tool, .. } = observation else {
            return Ok(None);
        };
        let configuration = poll
            .session
            .runtime_binding
            .as_ref()
            .and_then(|b| b.agent_configuration.as_ref())
            .ok_or_else(|| err("blocked", "缺少原 Agent 工具锁"))?;
        if configuration.context_protocol.as_deref() == Some(AGENT_CONTEXT_PROTOCOL)
            && let Some(definition) = agent_context_tools()
                .into_iter()
                .find(|d| d.call_name() == *tool)
        {
            return Ok(Some(ProviderContextDefinition {
                reference: AGENT_CONTEXT_PROTOCOL.into(),
                definition,
                rejection: None,
            }));
        }
        let Some(lock) = configuration.integration_lock.as_ref() else {
            return Ok(None);
        };
        if poll.authority.workflow.is_none() && poll.authority.controller.is_none() {
            // A domain tool is the package's; it is offered like a kernel tool, on its own.
            for record in lock.platform_tools.iter().chain(&lock.domain_tools) {
                let definition: ToolDefinition = record.decode()?;
                if definition.call_name() == *tool {
                    return Ok(Some(ProviderContextDefinition {
                        reference: record.id.clone(),
                        definition,
                        rejection: None,
                    }));
                }
            }
        }
        if let Some(record) = lock
            .unbound_tools
            .iter()
            .find(|r| r.data["qualified_name"] == *tool)
        {
            let definition = ToolDefinition {
                runtime: None,
                harness_provider: None,
                qualified_name: tool.clone(),
                implementation: "unbound".into(),
                handler: None,
                input_schema: record.data["input_schema"].clone(),
                output_schema: json!({"type":"object"}),
                capabilities: vec![],
            };
            return Ok(Some(ProviderContextDefinition {
                reference: record.id.clone(),
                definition,
                rejection: Some(err(
                    "integration_binding_required",
                    lock.unbound_reasons
                        .get(&record.id)
                        .cloned()
                        .unwrap_or_else(|| "请为此工具明确选择接入绑定，当前对话可以继续".into()),
                )),
            }));
        }
        Ok(None)
    }
    pub(crate) fn provider_context_record(
        &self,
        poll: &crate::provider_tools::ProviderPoll,
        observation: &ports::ConversationObservation,
        tool: ProviderContextDefinition,
        origin_scope: &Scope,
        turn_revision: u64,
    ) -> Result<ConfigurationRecord> {
        let ports::ConversationObservation::ToolCall {
            request,
            call,
            arguments,
            ..
        } = observation
        else {
            return Err(err("invalid_params", "缺少上下文调用观察"));
        };
        let configuration = poll
            .session
            .runtime_binding
            .as_ref()
            .and_then(|b| b.agent_configuration.as_ref())
            .ok_or_else(|| err("blocked", "缺少原 Agent 身份"))?;
        let identity = format!(
            "agent-context-call-{}",
            configuration_hash(&(
                poll.session.id.as_str(),
                poll.turn.id.as_str(),
                poll.worker.as_str(),
                call
            ))
        );
        let scope = if poll.session.scope == Some(SessionScope::Global) {
            Scope::global()
        } else {
            Scope::project(&poll.turn.project_id)
        };
        let mut basis = poll.basis();
        basis.turn_revision = turn_revision;
        let mut row = record(
            "agent_context_call",
            identity.clone(),
            scope,
            tool.definition.qualified_name.clone(),
            1,
            None,
            json!({"status":if tool.rejection.is_some(){"failed"}else{"prepared"},"definition":tool.definition,"origin_project":origin_scope.project_id(),"request_id":request,"call_id":call,"error":tool.rejection,"reply_sent":false,
                "provider_basis":basis,"conversation_authority":poll.authority,"provider_poll_id":poll.journal.as_ref().map(|j|&j.resource.id),"origin":{"kind":"native_worker","session_id":poll.session.id,"turn_id":poll.turn.id,"worker_operation_id":poll.worker},
                "invocation":{"id":identity,"agent_instance_id":configuration.agent_instance_id,"agent_revision_id":configuration.agent_revision_id,"session_id":poll.session.id,"turn_id":poll.turn.id,"operation_id":poll.worker,"project_id":poll.session.project_id,"feature_id":poll.session.feature_id,"task_id":poll.session.task_id,
                    "tool":{"revision_id":tool.reference,"definition":tool.definition},"arguments":arguments}}),
            false,
        );
        row.id = identity;
        Ok(row)
    }
}

impl Engine {
    pub(crate) fn execute_context_call(
        &self,
        catalog: &ConfigurationCatalogSnapshot,
        call: &ConfigurationRecord,
        session: &Session,
    ) -> Result<Value> {
        let state = catalog.state.as_ref();
        let invocation = &call.data["invocation"];
        let definition: ToolDefinition = serde_json::from_value(call.data["definition"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let arguments = &invocation["arguments"];
        self.tool_executor
            .as_ref()
            .ok_or_else(|| err("unsupported", "缺少工具结构校验器"))?
            .validate_arguments(&definition.input_schema, arguments)?;
        let binding = session
            .runtime_binding
            .as_ref()
            .ok_or_else(|| err("blocked", "原会话缺少运行锁"))?;
        let permissions = binding
            .permissions
            .as_ref()
            .ok_or_else(|| err("permission_unconfigured", "缺少冻结权限"))?;
        let tool_ref = invocation["tool"]["revision_id"].as_str().unwrap_or("");
        if let Some(lock) = &permissions.binding_lock {
            require(
                lock.harness
                    .profile
                    .policy
                    .tool_refs
                    .as_ref()
                    .is_none_or(|refs| refs.iter().any(|id| id == tool_ref)),
                "Profile 未允许该工具修订",
            )?;
        }
        for policy in [&permissions.harness.policy, &permissions.sandbox.policy] {
            let allowed = match &policy.tools {
                ToolPermission::AllAvailable => true,
                ToolPermission::Restricted { allow, deny } => {
                    !deny
                        .iter()
                        .any(|id| id == tool_ref || id == &definition.qualified_name)
                        && (allow.is_empty()
                            || allow
                                .iter()
                                .any(|id| id == tool_ref || id == &definition.qualified_name))
                }
            };
            if !allowed {
                return Err(err("permission_denied", "Permission 未允许该工具调用"));
            }
        }
        let instance = invocation["agent_instance_id"].as_str().unwrap_or("");
        let origin = call.data["origin_project"].as_str();
        let result = match definition.qualified_name.as_str() {
            "agent.experience.list" | "agent.experience.read" => {
                let (owner, _) = self.session_work_owner(session)?;
                let mut scoped = arguments.clone();
                if owner != session.owner_id {
                    scoped["entity_id"] = json!(owner);
                    scoped["source_project_id"] = json!(session.project_id);
                    scoped["project_id"] = json!(session.project_id);
                } else {
                    scoped["exclude_entity_scope"] = json!(true);
                }
                self.context_reader
                    .as_ref()
                    .ok_or_else(|| err("unsupported", "未装配 Agent 经验读取服务"))?
                    .experience(instance, origin, &scoped)
            }
            "workflow.outputs.read" => self
                .context_reader
                .as_ref()
                .ok_or_else(|| err("unsupported", "未装配历史产物读取服务"))?
                .workflow_outputs(instance, origin, arguments),
            "spec.lint" => {
                let scope = invocation["project_id"]
                    .as_str()
                    .filter(|id| *id != GLOBAL_DISCUSSION_NAMESPACE)
                    .or(origin)
                    .map(Scope::project)
                    .unwrap_or_default();
                let delivery = if let Some(reference) = arguments["delivery_ref"].as_str() {
                    let record = state.configuration(reference)?.clone();
                    require(
                        record.scope.visible_from(&scope),
                        "规范检查目标不在当前 Agent 可见范围",
                    )?;
                    Some(record)
                } else {
                    arguments
                        .get("delivery")
                        .map(|source| {
                            serde_json::from_value::<SourceDefinition>(source.clone())
                                .map_err(|e| err("invalid_params", e.to_string()))?
                                .into_record()
                        })
                        .transpose()?
                };
                if let Some(delivery) = delivery {
                    for reference in reference_list(&delivery.refs, "spec_refs")? {
                        require(
                            state.configuration(&reference)?.scope.visible_from(&scope),
                            "规范不在当前 Agent 可见范围",
                        )?;
                    }
                }
                self.specification_lint(arguments)
            }
            method @ ("project.overview"
            | "project.records.list"
            | "project.record.read"
            | "project.artifact.read"
            | "project.resources"
            | "configuration.catalog"
            | "configuration.read"
            | "configuration.usage") => self
                .context_reader
                .as_ref()
                .ok_or_else(|| err("unsupported", "未装配项目资料读取服务"))?
                .project_knowledge(
                    instance,
                    origin,
                    invocation["project_id"]
                        .as_str()
                        .filter(|id| *id != GLOBAL_DISCUSSION_NAMESPACE),
                    method,
                    arguments,
                ),
            _ => Err(err(
                "context_required",
                format!(
                    "{} 需要明确的工作目标与相应权限 Profile；Agent 能力保持可用",
                    definition.qualified_name
                ),
            )),
        }?;
        self.tool_executor
            .as_ref()
            .unwrap()
            .validate_arguments(&definition.output_schema, &result)?;
        Ok(result)
    }
}
