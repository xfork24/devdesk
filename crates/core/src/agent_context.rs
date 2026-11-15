//! Durable context calls share Agent identity without requiring a Workflow caller.
use crate::governance::{record, update_record};
use crate::*;

pub(crate) fn experience_read_history(state: &EngineeringState, session: &str) -> Vec<Value> {
    state.governance.iter().rev().filter(|row|row.r#type=="agent_context_call" && row.data["invocation"]["session_id"]==session && row.name.starts_with("agent.experience.")).take(50)
        .map(|row|json!({"id":row.id,"method":row.name,"status":row.data["status"],"source":row.data["result"]["entry"],"content_digest":row.data["result"]["content_digest"],"turn_id":row.data["invocation"]["turn_id"],"error":row.data["error"]})).collect()
}

fn active(state: &EngineeringState, call: &ConfigurationRecord) -> bool {
    state.turns.iter().any(|turn| {
        call.data["invocation"]["turn_id"] == turn.id
            && turn.status == TurnStatus::Running
            && !turn.cancel_requested
    }) && state.executions.iter().any(|worker| {
        call.data["invocation"]["session_id"] == worker.session_id
            && call.data["invocation"]["operation_id"] == worker.operation_id
            && !worker.writer_revoked
    })
}

impl Engine {
    pub(crate) fn project_context_call(
        &self,
        state: &mut EngineeringState,
        session: &Session,
        tid: &str,
        observation: &ports::ConversationObservation,
        tool: (&str, ToolDefinition),
        rejection: Option<Error>,
    ) -> Result<()> {
        let (tool_ref, definition) = tool;
        let ports::ConversationObservation::ToolCall {
            request,
            call,
            arguments,
            ..
        } = observation
        else {
            return Ok(());
        };
        let turn = state
            .turns
            .iter()
            .find(|turn| turn.id == tid)
            .ok_or_else(|| err("not_found", "Turn 不存在"))?;
        let binding = state.turn_runtime_binding(turn)?;
        let configuration = binding
            .agent_configuration
            .as_ref()
            .ok_or_else(|| err("invalid_configuration", "缺少 Agent 身份"))?;
        let instance = state.configuration(&configuration.agent_instance_id)?;
        let origin_project = instance.scope.project_id().map(str::to_owned);
        let scope = instance.scope.clone();
        let worker = state
            .executions
            .iter()
            .find(|worker| worker.session_id == session.id && !worker.writer_revoked)
            .ok_or_else(|| err("tool_not_authorized", "调用 worker 已停止"))?;
        let identity = format!(
            "agent-context-call-{}",
            configuration_hash(&(session.id.as_str(), tid, worker.operation_id.as_str(), call))
        );
        if let Some(previous) = state.governance.iter().find(|r| r.id == identity) {
            require(
                previous.data["invocation"]["arguments"] == *arguments
                    && previous.data["definition"] == json!(definition),
                "工具调用 ID 被不同输入复用",
            )?;
            return Ok(());
        }
        let mut row = record(
            "agent_context_call",
            identity.clone(),
            scope,
            definition.qualified_name.clone(),
            1,
            None,
            json!({"status":if rejection.is_some(){"failed"}else{"prepared"},"definition":definition,
                "origin_project":origin_project,"request_id":request,"call_id":call,"error":rejection,"reply_sent":false,
                "invocation":{"id":identity,"agent_instance_id":configuration.agent_instance_id,
                    "agent_revision_id":configuration.agent_revision_id,"session_id":session.id,"turn_id":tid,
                    "operation_id":worker.operation_id,"project_id":session.project_id,
                    "feature_id":session.feature_id,"task_id":session.task_id,
                    "tool":{"revision_id":tool_ref,"definition":definition},"arguments":arguments}}),
            false,
        );
        row.id = identity;
        state.governance.push(row);
        Ok(())
    }

    fn execute_context_call(
        &self,
        state: &EngineeringState,
        call: &ConfigurationRecord,
    ) -> Result<Value> {
        if !active(state, call) {
            return Err(err("cancelled", "调用者已停止"));
        }
        let invocation = &call.data["invocation"];
        let definition: ToolDefinition = serde_json::from_value(call.data["definition"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let arguments = &invocation["arguments"];
        self.tool_executor
            .as_ref()
            .ok_or_else(|| err("unsupported", "缺少工具结构校验器"))?
            .validate_arguments(&definition.input_schema, arguments)?;
        let turn = state
            .turns
            .iter()
            .find(|turn| invocation["turn_id"] == turn.id)
            .ok_or_else(|| err("not_found", "Turn 不存在"))?;
        let binding = state.turn_runtime_binding(turn)?;
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
            "agent.experience.list" | "agent.experience.read" => self
                .context_reader
                .as_ref()
                .ok_or_else(|| err("unsupported", "未装配 Agent 经验读取服务"))?
                .experience(instance, origin, arguments),
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

    pub(crate) fn drive_agent_context_calls(&self, snapshot: &EngineeringState) -> Result<()> {
        for call in snapshot
            .governance
            .iter()
            .filter(|r| r.r#type == "agent_context_call" && r.data["status"] == "prepared")
        {
            let result = self.execute_context_call(snapshot, call);
            self.store.transact(None, &mut |state| {
                let current = state
                    .governance
                    .iter_mut()
                    .find(|r| r.id == call.id)
                    .ok_or_else(|| err("storage", "上下文调用丢失"))?;
                if current.data["status"] != "prepared" {
                    return Ok(Value::Null);
                }
                let mut data = current.data.clone();
                data["status"] = json!(if result.is_ok() {
                    "succeeded"
                } else {
                    "failed"
                });
                data["result"] = result.as_ref().ok().cloned().unwrap_or(Value::Null);
                data["error"] = json!(result.as_ref().err());
                data["finished_at"] = json!(now());
                update_record(current, data);
                Ok(Value::Null)
            })?;
        }
        let snapshot = self.store.snapshot()?;
        for call in snapshot.governance.iter().filter(|r| {
            r.r#type == "agent_context_call"
                && r.data["reply_sent"] != true
                && r.data["status"] != "prepared"
        }) {
            let session = snapshot
                .sessions
                .iter()
                .find(|s| call.data["invocation"]["session_id"] == s.id)
                .ok_or_else(|| err("storage", "上下文 Session 丢失"))?;
            let success = call.data["status"] == "succeeded";
            let receipt = ToolReceipt {
                invocation_id: call.id.clone(),
                success,
                content: vec![
                    json!({"type":"text","text":if success{call.data["result"].to_string()}else{call.data["error"].to_string()}}),
                ],
                structured_content: success.then(|| call.data["result"].clone()),
                remote_effect: "not_applied".into(),
                error: (!success).then(|| call.data["error"].clone()),
                origin: Origin::External,
            };
            if active(&snapshot, call)
                && self
                    .harness
                    .answer_tool_call(session, &call.data["request_id"], &receipt)
                    .is_err()
            {
                continue;
            }
            self.store.transact(None, &mut |state| {
                let row = state
                    .governance
                    .iter_mut()
                    .find(|r| r.id == call.id)
                    .unwrap();
                let mut data = row.data.clone();
                data["reply_sent"] = json!(true);
                data["receipt"] = json!(receipt);
                update_record(row, data);
                Ok(Value::Null)
            })?;
        }
        Ok(())
    }
}
