//! Durable manager sessions and native workflow tool calls.
use crate::governance::{record, update_record};
use crate::*;

pub const CONTROL_TOOLS: &[&str] = &[
    "workflow.start",
    "workflow.inspect",
    "workflow.events.read",
    "workflow.outputs.read",
    "workflow.execution.inspect",
    "workflow.step.dispatch",
    "workflow.step.accept",
    "workflow.step.input",
    "workflow.step.retry",
    "workflow.step.rework",
    "workflow.execution.stop",
    "workflow.pause",
    "workflow.resume",
    "workflow.cancel",
    "workflow.change.preview",
    "workflow.change.apply",
    "workflow.materials.import",
    "workflow.decision.request",
    "workflow.execution.reconcile",
    "workflow.complete",
    "workflow.segment.dispatch",
];

fn control_query(method: &str) -> bool {
    matches!(
        method,
        "workflow.inspect"
            | "workflow.events.read"
            | "workflow.outputs.read"
            | "workflow.execution.inspect"
            | "workflow.change.preview"
    )
}

fn native_reply_value(method: &str, call_id: &str, value: Value) -> Value {
    if !control_query(method) && value.to_string().len() > 128 * 1024 {
        json!({"id":value["id"],"status":value["status"],"receipt_ref":call_id,"result_digest":configuration_hash(&value),"accepted":true,"next":"Use workflow.inspect to read the current bounded projection."})
    } else {
        value
    }
}

fn operation(method: &str) -> &str {
    match method {
        "workflow.events.read" | "workflow.outputs.read" | "workflow.execution.inspect" => {
            "inspect"
        }
        "workflow.materials.import" => "import",
        "workflow.decision.request" => "request_decision",
        "workflow.segment.dispatch" => "segment",
        _ => method.rsplit('.').next().unwrap_or(""),
    }
}

fn instance_summary(state: &EngineeringState, row: &ConfigurationRecord) -> Value {
    let steps = row.data["steps"].as_array().into_iter().flatten().map(|execution| {
        let definition = row.data["plan"]["document"]["steps"].as_array().into_iter().flatten().find(|step| step["key"] == execution["step"]);
        let policy = definition.and_then(|step| step["confirmation"].as_str()).unwrap_or("automatic");
        let confirmation = crate::configured_runtime::matching_confirmation(state, execution["id"].as_str().unwrap_or(""), execution["input_digest"].as_str().unwrap_or(""), &execution["outputs"]);
        let waiver = state.governance.iter().find(|record| record.r#type == "workflow_waiver" && record.data["plan_revision_id"] == row.data["active_plan_revision_id"] && record.data["node_id"] == execution["node_id"] && record.data["requirement"] == "confirmation");
        let disposition = if confirmation.is_some() { "confirmed" } else if waiver.is_some() { "waived" } else if policy == "human" { "pending" } else { "not_required" };
        json!({"step":execution["step"],"id":execution["id"],"status":execution["status"],"outputs":execution["outputs"],"input_digest":execution["input_digest"],"reason":execution["reason"],"confirmation_policy":policy,"human_satisfied":policy!="human"||confirmation.is_some()||waiver.is_some(),"confirmation_status":disposition,"confirmation_ref":confirmation.map(|record|&record.id),"confirmation_waiver_ref":waiver.map(|record|&record.id)})
    }).collect::<Vec<_>>();
    json!({"id":row.id,"owner_id":row.data["owner_id"],"standard_ref":row.data["plan"]["standard_ref"],"workflow_ref":row.data["workflow_ref"],"plan":row.data["active_plan_revision_id"],"status":row.data["status"],"control_version":row.data["control_version"],"paused":row.data["paused"],"steps":steps})
}

fn step_attention(step: &Value) -> u8 {
    match step["status"].as_str().unwrap_or("") {
        "failed"
        | "needs_reconciliation"
        | "waiting_input"
        | "waiting_confirmation"
        | "awaiting_acceptance"
        | "cancelled" => 0,
        "running" => 1,
        "queued" => 2,
        _ => 3,
    }
}

pub(crate) fn binding<'a>(s: &'a EngineeringState, sid: &str) -> Option<&'a ConfigurationRecord> {
    s.governance
        .iter()
        .find(|r| r.r#type == "controller_session" && r.definition_id == sid)
}
pub(crate) fn authorized_send(s: &EngineeringState, c: &Command) -> bool {
    let Some(target) = c.target.as_deref() else {
        return false;
    };
    let Some(binding) = binding(s, target) else {
        return false;
    };
    let Ok(row) = s.configuration(binding.data["controller_id"].as_str().unwrap_or("")) else {
        return false;
    };
    row.r#type == "workflow_controller"
        && row.data["status"] == "active"
        && row.data["session_id"] == target
        && row.data["epoch"] == binding.data["epoch"]
        && c.payload["controller_epoch"] == row.data["epoch"]
        && s.governance.iter().any(|r| {
            r.r#type == "workflow_intent"
                && r.definition_id == row.id
                && r.data["method"] == "conversation.send"
                && r.data["command"] == json!(c)
        })
}

fn intent(
    s: &mut EngineeringState,
    row: &ConfigurationRecord,
    epoch: u64,
    method: &str,
    target: &str,
    payload: Value,
) {
    let command_id = format!(
        "controller:{}:{}:{}:{}",
        row.id,
        epoch,
        method,
        configuration_hash(&payload)
    );
    if s.governance
        .iter()
        .any(|r| r.r#type == "workflow_intent" && r.data["command"]["command_id"] == command_id)
    {
        return;
    }
    let expected = revision(s, target).or_else(|| {
        s.sessions
            .iter()
            .find(|r| r.id == target)
            .map(|r| r.revision)
    });
    s.governance.push(record("workflow_intent",row.id.clone(),row.scope.clone(),method.into(),1,None,json!({"status":"prepared","controller":true,"method":method,"command":Command{command_id,target:Some(target.into()),expected_revision:expected,payload}}),false));
}

impl Engine {
    pub(crate) fn controller_command(
        &self,
        method: &str,
        c: &Command,
        s: &mut EngineeringState,
    ) -> Result<Option<Value>> {
        if ![
            "orchestration.open",
            "manager.open",
            "orchestration.input",
            "orchestration.takeover",
            "orchestration.resume",
        ]
        .contains(&method)
        {
            return Ok(None);
        }
        let target = c.target.as_deref().unwrap_or("");
        if method == "orchestration.open" || method == "manager.open" {
            let workspace_only = method == "manager.open";
            let defaults = current_template_defaults(s).unwrap_or(Value::Null);
            require(
                !workspace_only || s.projects.iter().any(|p| p.id == target),
                "经理工作台必须绑定项目",
            )?;
            let root = s
                .governance
                .iter()
                .find(|r| r.id == target && r.r#type == "workflow_instance")
                .cloned();
            let owner = root
                .as_ref()
                .and_then(|r| r.data["owner_id"].as_str())
                .unwrap_or(target)
                .to_owned();
            let project = s
                .projects
                .iter()
                .find(|p| p.id == owner)
                .map(|p| p.id.clone())
                .or_else(|| {
                    s.features
                        .iter()
                        .find(|f| f.id == owner)
                        .map(|f| f.project_id.clone())
                })
                .ok_or_else(|| err("not_found", "编排归属不存在"))?;
            let mut predecessor = None;
            let mut snapshot = None;
            let mut replaced = None;
            if let Some(existing) = s
                .governance
                .iter()
                .rev()
                .find(|r| {
                    r.r#type == "workflow_controller"
                        && r.data["owner_id"] == owner
                        && r.data["workspace_only"].as_bool().unwrap_or(false) == workspace_only
                        && (r.data["status"] != "closed"
                            || root
                                .as_ref()
                                .is_some_and(|root| r.data["root_instance_id"] == root.id))
                })
                .cloned()
            {
                let replacing = c.payload["agent_ref"]
                    .as_str()
                    .is_some_and(|id| existing.data["agent_revision_id"] != id)
                    || c.payload["permission_context_ref"]
                        .as_str()
                        .is_some_and(|id| existing.data["permission_context_ref"] != id);
                if !replacing {
                    return Ok(Some(
                        json!({"id":existing.id,"reused":true,"controller":existing.data}),
                    ));
                }
                let sid = existing.data["session_id"].as_str();
                require(
                    existing.data["status"] != "active"
                        && !s
                            .turns
                            .iter()
                            .any(|t| Some(t.session_id.as_str()) == sid && t.status.active())
                        && !s
                            .executions
                            .iter()
                            .any(|e| Some(e.session_id.as_str()) == sid && !e.writer_revoked),
                    "请先暂停并核对原经理 worker，再采用后继配置",
                )?;
                if let Some(session) = sid
                    .and_then(|id| s.sessions.iter().find(|v| v.id == id))
                    .cloned()
                {
                    snapshot = Some(self.capture_conversation(
                        s,
                        &session,
                        "orchestration_successor",
                        None,
                    )?);
                    predecessor = Some(session.id);
                }
                replaced = Some(existing.id.clone());
                let mut data = existing.data.clone();
                data["status"] = json!("closed");
                data["epoch"] = json!(data["epoch"].as_u64().unwrap_or(0) + 1);
                update_record(
                    s.governance
                        .iter_mut()
                        .find(|r| r.id == existing.id)
                        .unwrap(),
                    data,
                );
            }
            let config = s
                .governance
                .iter()
                .filter(|r| r.r#type == "project_configuration" && r.definition_id == project)
                .max_by_key(|r| r.version);
            let role = c.payload["role_ref"]
                .as_str()
                .or_else(|| {
                    workspace_only
                        .then(|| {
                            c.payload["agent_ref"]
                                .as_str()
                                .and_then(|id| s.configuration(id).ok())
                                .and_then(|r| r.refs["role_ref"].as_str())
                        })
                        .flatten()
                })
                .or_else(|| config.and_then(|r| r.data["orchestrator_role_ref"].as_str()))
                .or_else(|| {
                    workspace_only
                        .then(|| defaults["orchestrator_role_ref"].as_str())
                        .flatten()
                })
                .ok_or_else(|| err("configuration_required", "项目需要明确编排 Role"))?;
            let role = crate::configuration::role_record(s, role)?;
            let agent_id = c.payload["agent_ref"]
                .as_str()
                .or_else(|| config.and_then(|r| r.data["role_agent_bindings"][&role.id].as_str()))
                .or_else(|| {
                    workspace_only
                        .then(|| defaults["role_agent_bindings"][&role.id].as_str())
                        .flatten()
                })
                .ok_or_else(|| err("configuration_required", "编排 Role 未绑定 Agent"))?;
            let agent = s.configuration(agent_id)?;
            let composition = s.resolve_agent(&agent.id)?;
            require(
                composition.role == role.definition_id,
                "编排 Agent 的 Role 不匹配",
            )?;
            require(
                driver_supports(
                    s.configuration(&composition.harness_ref)?.data["provider"]
                        .as_str()
                        .unwrap_or(""),
                    "orchestration",
                ),
                "此 Harness 尚不支持编排原生工具桥",
            )?;
            crate::governance::validate_composition(
                s,
                &composition,
                &Scope::project(project.clone()),
            )?;
            require(
                c.payload.get("policy").is_none(),
                "编排授权必须通过 Permission Context 源修订选择",
            )?;
            let context_ref = c.payload["permission_context_ref"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| permission_context(s).map(|r| r.id.clone()))
                .ok_or_else(|| err("permission_unconfigured", "请选择编排授权"))?;
            let context = crate::governance::reference(
                s,
                &context_ref,
                "permission_context",
                &Scope::project(project.clone()),
            )?;
            let policy: WorkflowControlPolicy =
                serde_json::from_value(context.data["control_policy"].clone())
                    .map_err(|e| err("invalid_source", e.to_string()))?;
            policy.validate()?;
            let controller = WorkflowController {
                workspace_only,
                project_id: project.clone(),
                owner_id: owner,
                root_instance_id: root.as_ref().map(|r| r.id.clone()),
                agent_revision_id: agent.id.clone(),
                role_ref: role.id.clone(),
                permission_context_ref: context_ref,
                policy_digest: configuration_hash(&policy),
                policy,
                session_id: None,
                predecessor_session_id: predecessor,
                snapshot_id: snapshot,
                epoch: 1,
                status: "active".into(),
                event_cursor: 0,
                wakeup_turns: 0,
                last_state_digest: String::new(),
                last_result_digest: String::new(),
                reason: String::new(),
            };
            let row = record(
                "workflow_controller",
                controller.owner_id.clone(),
                Scope::project(project),
                "项目经理".into(),
                1,
                None,
                json!(controller),
                false,
            );
            s.orchestration_leases.push(OrchestrationLease {
                id: row.id.clone(),
                controller_id: row.id.clone(),
                owner_id: controller.owner_id.clone(),
                root_instance_id: controller.root_instance_id.clone(),
                epoch: controller.epoch,
                expires_at: now() + controller.policy.timeout_seconds * 1000,
            });
            let out = json!({"id":row.id,"controller":controller});
            let successor = row.id.clone();
            s.governance.push(row);
            if let Some(previous) = replaced {
                // Relinquish the unique root slot before persisting the successor.
                // The expired row and closed controller preserve the old identity.
                for lease in s
                    .orchestration_leases
                    .iter_mut()
                    .filter(|l| l.controller_id == previous)
                {
                    lease.root_instance_id = None;
                    lease.expires_at = now();
                }
                for intent in s.governance.iter_mut().filter(|r| {
                    r.r#type == "workflow_intent"
                        && r.definition_id == previous
                        && r.data["status"] == "prepared"
                }) {
                    let mut data = intent.data.clone();
                    data["status"] = json!("revoked");
                    update_record(intent, data);
                }
                for segment in s.governance.iter_mut().filter(|r| {
                    r.r#type == "workflow_segment"
                        && r.data["actor"]["controller_id"] == previous
                        && r.data["status"] == "active"
                }) {
                    let mut data = segment.data.clone();
                    data["status"] = json!("stopped");
                    data["reason"] = json!("人类采用后继经理");
                    update_record(segment, data);
                }
                if !workspace_only {
                    if let Some(driver) = s.governance.iter_mut().find(|r| {
                        r.r#type == "workflow_driver"
                            && Some(r.definition_id.as_str())
                                == controller.root_instance_id.as_deref()
                            && r.data["mode"] == "agent"
                    }) {
                        require(
                            driver.data["controller_id"] == previous
                                || driver.data["controller_id"].is_null(),
                            "根控制权已经改变",
                        )?;
                        let mut data = driver.data.clone();
                        data["controller_id"] = json!(successor);
                        data["epoch"] = json!(data["epoch"].as_u64().unwrap_or(0) + 1);
                        data["expires_at"] = json!(now() + 60_000);
                        update_record(driver, data);
                    }
                }
                s.governance.push(record("controller_selection",successor.clone(),Scope::project(controller.project_id.clone()),"人类采用后继经理".into(),1,None,json!({"predecessor_controller_id":previous,"successor_controller_id":successor,"agent_ref":controller.agent_revision_id,"context_ref":controller.permission_context_ref}),true));
            }
            return Ok(Some(out));
        }
        let row = s.configuration(target)?.clone();
        require(row.r#type == "workflow_controller", "不是编排控制器")?;
        let mut controller: WorkflowController = row.decode()?;
        require(
            !s.governance.iter().any(|r| {
                r.r#type == "controller_selection" && r.data["predecessor_controller_id"] == row.id
            }),
            "已交接的控制器不能恢复，请使用当前经理",
        )?;
        if let Some(epoch) = c.payload["epoch"].as_u64() {
            require(epoch == controller.epoch, "编排代次已变化")?;
        }
        require(
            c.expected_revision == Some(row.revision)
                || (c
                    .expected_revision
                    .is_some_and(|v| v > 0 && v <= row.revision)
                    && c.payload["epoch"].as_u64() == Some(controller.epoch)),
            "编排状态已变化",
        )?;
        if method == "orchestration.input" {
            require(
                ["active", "budget_exhausted"].contains(&controller.status.as_str()),
                "请先恢复项目经理再发送指令",
            )?;
            let input = text(&c.payload, "input")?;
            require(input.len() <= 65536, "输入过长")?;
            s.governance.push(record(
                "controller_input",
                row.id.clone(),
                row.scope.clone(),
                "用户指令".into(),
                1,
                None,
                json!({"input":input,"consumed":false}),
                false,
            ));
            controller.status = "active".into();
            controller.wakeup_turns = 0;
        } else {
            controller.epoch += 1;
            controller.wakeup_turns = 0;
            controller.reason.clear();
            if method == "orchestration.takeover" {
                controller.status = "paused".into();
                for t in s.turns.iter_mut().filter(|t| {
                    Some(&t.session_id) == controller.session_id.as_ref() && t.status.active()
                }) {
                    t.cancel_requested = true;
                    t.revision += 1;
                }
            } else {
                require(
                    c.payload.get("policy").is_none(),
                    "编排授权必须通过 Permission Context 源修订选择",
                )?;
                if let Some(sid) = &controller.session_id {
                    require(
                        !s.turns
                            .iter()
                            .any(|t| t.session_id == *sid && t.status.active())
                            && !s
                                .executions
                                .iter()
                                .any(|e| e.session_id == *sid && !e.writer_revoked),
                        "请先停止并核对旧编排执行者",
                    )?;
                }
                for field in ["agent_ref", "permission_context_ref"] {
                    if let Some(value) = c.payload.get(field) {
                        require(value.is_string(), "经理配置引用必须是确切源修订字符串")?;
                    }
                }
                let agent_ref = c.payload["agent_ref"]
                    .as_str()
                    .unwrap_or(&controller.agent_revision_id)
                    .to_owned();
                let context_ref = c.payload["permission_context_ref"]
                    .as_str()
                    .unwrap_or(&controller.permission_context_ref)
                    .to_owned();
                if agent_ref != controller.agent_revision_id
                    || context_ref != controller.permission_context_ref
                {
                    require(
                        row.data["status"] != "active" || controller.session_id.is_none(),
                        "请先暂停旧经理再采用后继配置",
                    )?;
                    return self.controller_command(
                        if controller.workspace_only { "manager.open" } else { "orchestration.open" },
                        &Command { command_id:format!("{}:successor",c.command_id), target:Some(controller.root_instance_id.clone().unwrap_or_else(||controller.owner_id.clone())), expected_revision:None,
                            payload:json!({"role_ref":controller.role_ref,"agent_ref":agent_ref,"permission_context_ref":context_ref}) }, s,
                    );
                }
                if let Some(session) = controller
                    .session_id
                    .as_ref()
                    .and_then(|id| s.sessions.iter().find(|session| session.id == *id))
                    .cloned()
                {
                    controller.snapshot_id = Some(self.capture_conversation(
                        s,
                        &session,
                        "orchestration_handoff",
                        None,
                    )?);
                    controller.predecessor_session_id = Some(session.id);
                }
                controller.session_id = None;
                controller.status = "active".into();
                controller.last_state_digest.clear();
            }
        }
        if let Some(lease) = s
            .orchestration_leases
            .iter_mut()
            .find(|l| l.controller_id == row.id)
        {
            lease.epoch = controller.epoch;
            lease.expires_at = now() + controller.policy.timeout_seconds * 1000;
        }
        update_record(
            s.governance.iter_mut().find(|r| r.id == row.id).unwrap(),
            json!(controller),
        );
        Ok(Some(json!({"id":row.id,"controller":controller})))
    }

    pub(crate) fn controller_agent_request(
        &self,
        c: &Command,
    ) -> Result<Option<ConfiguredAgentRequest>> {
        let Some(id) = c.payload["controller_id"].as_str() else {
            return Ok(None);
        };
        let s = self.store.snapshot()?;
        let row = s.configuration(id)?;
        let controller: WorkflowController = row.decode()?;
        require(
            row.r#type == "workflow_controller"
                && controller.status == "active"
                && c.payload["agent"] == controller.agent_revision_id
                && c.payload["controller_epoch"] == controller.epoch
                && c.target.as_deref() == Some(&controller.owner_id)
                && s.governance.iter().any(|r| {
                    r.r#type == "workflow_intent"
                        && r.definition_id == id
                        && r.data["command"] == json!(c)
                }),
            "编排会话缺少确切执行意图",
        )?;
        let agent = s.resolve_agent(&controller.agent_revision_id)?;
        let permissions = resolve_permission_lock(
            &s,
            &agent.harness_ref,
            &permission_selection_from_context(&s, &controller.permission_context_ref)?,
        )?;
        let mut lock = freeze_integrations(&s, &agent, Some(&controller.project_id))?;
        lock.platform_tools.retain(|r| {
            r.data["qualified_name"].as_str().is_some_and(|n| {
                if controller.workspace_only {
                    MANAGER_TOOLS.contains(&n)
                } else {
                    CONTROL_TOOLS.contains(&n) || MANAGER_TOOLS.contains(&n)
                }
            })
        });
        require(
            !lock.platform_tools.is_empty(),
            "编排 Agent 的 Capability 缺少控制工具",
        )?;
        Ok(Some(ConfiguredAgentRequest {
            integration_lock: lock,
            access: StepAccess::default(),
            permissions,
        }))
    }

    pub(crate) fn project_controller_tool(
        &self,
        s: &mut EngineeringState,
        session: &Session,
        tid: &str,
        observation: &ports::ConversationObservation,
        definition: ConfigurationRecord,
    ) -> Result<()> {
        let ports::ConversationObservation::ToolCall {
            request,
            call,
            arguments,
            ..
        } = observation
        else {
            return Err(err("invalid_params", "不是原生工具调用"));
        };
        let bound =
            binding(s, &session.id).ok_or_else(|| err("tool_not_authorized", "无编排会话绑定"))?;
        let row = s
            .configuration(bound.data["controller_id"].as_str().unwrap_or(""))?
            .clone();
        let controller: WorkflowController = row.decode()?;
        require(
            crate::manager_workspace::session_instance_current(s, &controller),
            "经理实例已改变，需要后继会话",
        )?;
        let name = text(&definition.data, "qualified_name")?;
        let worker = s
            .executions
            .iter()
            .find(|e| e.session_id == session.id && !e.writer_revoked)
            .ok_or_else(|| err("tool_not_authorized", "编排 worker 已停止"))?;
        require(
            (if controller.workspace_only {
                MANAGER_TOOLS.contains(&name.as_str())
            } else {
                CONTROL_TOOLS.contains(&name.as_str()) || MANAGER_TOOLS.contains(&name.as_str())
            }) && controller.session_id.as_deref() == Some(&session.id)
                && controller.status == "active"
                && bound.data["epoch"] == controller.epoch,
            "编排身份或代次已失效",
        )?;
        let turn = s
            .turns
            .iter()
            .find(|t| t.id == tid && t.session_id == session.id && t.status == TurnStatus::Running)
            .ok_or_else(|| err("tool_not_authorized", "编排 Turn 无效"))?;
        let b = s.turn_runtime_binding(turn)?;
        let permissions = b
            .permissions
            .as_ref()
            .ok_or_else(|| err("permission_unconfigured", "编排权限缺失"))?;
        for policy in [&permissions.harness.policy, &permissions.sandbox.policy] {
            if let ToolPermission::Restricted { allow, deny } = &policy.tools {
                require(
                    !deny.contains(&definition.id)
                        && (allow.is_empty() || allow.contains(&definition.id)),
                    "Permission 未允许该工具",
                )?;
            }
        }
        let identity = format!(
            "controller-call-{}",
            configuration_hash(&(
                session.id.as_str(),
                tid,
                worker.operation_id.as_str(),
                call.as_str()
            ))
        );
        if let Ok(old) = s.configuration(&identity) {
            require(
                old.data["arguments"] == *arguments,
                "原生工具调用 ID 被不同参数复用",
            )?;
            return Ok(());
        }
        self.tool_executor
            .as_ref()
            .ok_or_else(|| err("unsupported", "缺少参数校验器"))?
            .validate_arguments(&definition.data["input_schema"], arguments)?;
        require(
            s.orchestration_leases.iter().any(|l| {
                l.controller_id == row.id && l.epoch == controller.epoch && l.expires_at >= now()
            }),
            "编排租约已过期",
        )?;
        let count = s
            .governance
            .iter()
            .filter(|r| r.r#type == "controller_tool_call" && r.data["turn_id"] == tid)
            .count();
        require(
            count < (controller.policy.max_tool_calls_per_turn as usize),
            "编排工具预算耗尽",
        )?;
        let mut source = record(
            "controller_tool_call",
            row.id,
            row.scope,
            "编排工具调用".into(),
            1,
            None,
            json!({"status":"prepared","method":name,"arguments":arguments,"session_id":session.id,"turn_id":tid,"worker_operation_id":worker.operation_id,"request_id":request,"call_id":call,"tool_ref":definition.id,"epoch":controller.epoch}),
            false,
        );
        source.id = identity;
        s.governance.push(source);
        Ok(())
    }

    pub fn drive_controllers(&self) -> Result<()> {
        self.drive_controller_calls()?;
        self.store.transact(None,&mut|s|{
            let rows:Vec<_>=s.governance.iter().filter(|r|r.r#type=="workflow_controller").cloned().collect();
            for row in rows{
                let mut controller:WorkflowController=row.decode()?;
                if controller.status!="active"{continue;}
                if !crate::manager_workspace::session_instance_current(s,&controller) {
                    if s.governance.iter().any(|call|call.r#type=="controller_tool_call"&&call.definition_id==row.id&&!s.governance.iter().any(|reply|reply.r#type=="controller_tool_reply"&&reply.definition_id==call.id)){continue;}
                    for turn in s.turns.iter_mut().filter(|t|Some(&t.session_id)==controller.session_id.as_ref()&&t.status.active()){turn.cancel_requested=true;turn.revision+=1;}
                    controller.epoch+=1;controller.status="paused".into();controller.reason="经理实例已换绑/移除，当前冻结会话已撤权；需要人类建立后继会话".into();
                    update_record(s.governance.iter_mut().find(|r|r.id==row.id).unwrap(),json!(controller));continue;
                }
                if controller.session_id.is_none(){
                    let opened=s.governance.iter().rev().find(|r|r.r#type=="workflow_intent"&&r.definition_id==row.id&&r.data["method"]=="session.open"&&r.data["command"]["payload"]["controller_epoch"]==controller.epoch).cloned();
                    if let Some(opened)=opened{
                        if let Some(sid)=opened.data["result"]["id"].as_str(){
                            controller.session_id=Some(sid.into());
                            if binding(s,sid).is_none(){s.governance.push(record("controller_session",sid.into(),row.scope.clone(),"项目经理会话".into(),1,None,json!({"controller_id":row.id,"epoch":controller.epoch}),true));}
                        }else if opened.data["status"]=="failed"{controller.status="failed".into();controller.reason=opened.data["error"].to_string();}
                    }else{intent(s,&row,controller.epoch,"session.open",&controller.owner_id,json!({"agent":controller.agent_revision_id,"work_type":"orchestration","controller_id":row.id,"controller_epoch":controller.epoch,"force_new":true,"snapshot_id":controller.snapshot_id,"predecessor":controller.predecessor_session_id}));}
                    update_record(s.governance.iter_mut().find(|r|r.id==row.id).unwrap(),json!(controller));continue;
                }
                let sid=controller.session_id.clone().unwrap();
                if let Some(turn)=s.turns.iter().find(|t|t.session_id==sid&&t.status.active()).cloned(){
                    if let Some(lease)=s.orchestration_leases.iter_mut().find(|l|l.controller_id==row.id&&l.epoch==controller.epoch){if lease.expires_at<now()+controller.policy.timeout_seconds*500 {lease.expires_at=now()+controller.policy.timeout_seconds*1000;}}

                    if turn.status==TurnStatus::NeedsReconciliation {controller.status="needs_reconciliation".into();controller.reason="编排执行结果未知，请停止并核对后恢复".into();}
                    else if turn.started.is_some_and(|started|now().saturating_sub(started)>controller.policy.timeout_seconds*1000){let t=s.turns.iter_mut().find(|t|t.id==turn.id).unwrap();t.cancel_requested=true;t.revision+=1;controller.status="budget_exhausted".into();controller.reason="编排 Turn 超时，已请求停止".into();}
                    update_record(s.governance.iter_mut().find(|r|r.id==row.id).unwrap(),json!(controller));continue;
                }
                // A durable send can be awaiting Host dispatch before a Turn exists.
                // Coalesce new events until that exact send has been admitted.
                if s.governance.iter().any(|r|r.r#type=="workflow_intent"&&r.definition_id==row.id&&r.data["method"]=="conversation.send"&&r.data["status"]=="prepared"&&r.data["command"]["payload"]["controller_epoch"]==controller.epoch){continue;}
                let root=controller.root_instance_id.as_ref().and_then(|id|s.configuration(id).ok());
                if root.is_some_and(|r|["completed","cancelled"].contains(&r.data["status"].as_str().unwrap_or(""))){controller.status="closed".into();update_record(s.governance.iter_mut().find(|r|r.id==row.id).unwrap(),json!(controller));continue;}
                let instances:Vec<_>=s.governance.iter().filter(|r|r.r#type=="workflow_instance"&&Some(r.data["root_instance_id"].as_str().unwrap_or(""))==controller.root_instance_id.as_deref()).map(|row|instance_summary(s,row)).collect();
                let mut input_bytes=0;
                let user:Vec<_>=s.governance.iter().filter(|r|r.r#type=="controller_input"&&r.definition_id==row.id&&r.data["consumed"]==false).take(8).take_while(|r|{let size=r.data["input"].as_str().unwrap_or("").len();if input_bytes+size>65536{false}else{input_bytes+=size;true}}).cloned().collect();
                let decisions:Vec<_>=s.governance.iter().filter(|r|r.r#type=="workflow_pending_decision"&&s.configuration(&r.definition_id).is_ok_and(|i|Some(i.data["root_instance_id"].as_str().unwrap_or(""))==controller.root_instance_id.as_deref())).map(|r|json!({"id":r.id,"instance_id":r.definition_id,"status":r.data["status"],"decision_ref":r.data["decision_ref"],"request":r.data["request"]})).collect();
                if let Some(failed)=s.governance.iter().rev().find(|r|r.r#type=="workflow_intent"&&r.definition_id==row.id&&r.data["method"]=="conversation.send"&&r.data["command"]["payload"]["controller_epoch"]==controller.epoch&&r.data["status"]=="failed").cloned() {
                    let refs=failed.data["command"]["payload"]["controller_input_refs"].as_array().cloned().unwrap_or_default();
                    for input in s.governance.iter_mut().filter(|r|r.r#type=="controller_input"&&r.definition_id==row.id&&refs.contains(&json!(r.id))) {let mut data=input.data.clone();data["consumed"]=json!(false);update_record(input,data);}
                    controller.status="failed".into();controller.reason=failed.data["error"].to_string();
                    update_record(s.governance.iter_mut().find(|r|r.id==row.id).unwrap(),json!(controller));continue;
                }
                if controller.workspace_only && user.is_empty(){continue;}
                let digest=configuration_hash(&json!({"instances":instances,"decisions":decisions,"inputs":user.iter().map(|r|&r.id).collect::<Vec<_>>() }));
                if digest==controller.last_state_digest{continue;}
                let results=configuration_hash(&instances.iter().flat_map(|i|i["steps"].as_array().into_iter().flatten()).map(|e|json!({"outputs":e["outputs"],"confirmation_ref":e["confirmation_ref"],"confirmation_waiver_ref":e["confirmation_waiver_ref"]})).collect::<Vec<_>>());
                if results!=controller.last_result_digest||!user.is_empty(){controller.wakeup_turns=0;controller.last_result_digest=results;}
                if controller.wakeup_turns>=controller.policy.max_turns_per_wakeup{controller.status="budget_exhausted".into();controller.reason="编排唤醒预算已耗尽，请恢复或补充指令".into();update_record(s.governance.iter_mut().find(|r|r.id==row.id).unwrap(),json!(controller));continue;}
                let mut visible=instances.clone();visible.sort_by_key(|i| (i["id"]!=json!(controller.root_instance_id),["completed","cancelled"].contains(&i["status"].as_str().unwrap_or(""))));visible.truncate(16);
                for instance in &mut visible {if let Some(steps)=instance["steps"].as_array(){let total=steps.len();let mut shown=steps.clone();shown.sort_by_key(step_attention);shown.truncate(16);for step in &mut shown {if let Some(reason)=step["reason"].as_str(){step["reason"]=json!(reason.chars().take(160).collect::<String>());}}instance["steps"]=json!(shown);instance["step_total"]=json!(total);instance["step_selection"]=json!("attention_then_pending");}}
                let visible_decisions=decisions.iter().rev().take(32).map(|d|json!({"id":d["id"],"instance_id":d["instance_id"],"status":d["status"],"decision_ref":d["decision_ref"],"operation":d["request"]["operation"]})).collect::<Vec<_>>();
                let scope_snapshot=json!({"renderer_version":ORCHESTRATION_RENDERER,"controller_id":row.id,"epoch":controller.epoch,"owner_id":controller.owner_id,"root_instance_id":controller.root_instance_id,"instances":visible,"instance_total":instances.len(),"event_hint":s.events.last().map(|e|e.id),
                    "workspace_only":controller.workspace_only,"project_id":controller.project_id,"project_tools":MANAGER_TOOLS,"project_overview_only":true,"instructions":if controller.workspace_only {"You are a project-bound manager workspace. Use project.overview and paginated project.records.list/read to obtain facts. Use configuration catalog/read/usage, then change.preview and change.apply with the exact preview hash. Source edits publish new immutable revisions; adoption is explicit. You have no root scheduling lease. Never claim a write succeeded without its tool receipt. Do not change your own current permissions. Shared configuration writes require configuration.global.write; otherwise create project-owned successors. Finish and wait after answering the user's request."}else{"Use workflow tools to inspect the current plan and outputs, dispatch and accept explicit steps, and complete workflows. Every child also requires explicit dispatch and completion. Projected steps prioritize unresolved and pending work, not authoring order; inspect omitted steps when needed. human_satisfied and confirmation_ref reflect matching recorded decisions; execution.reason is an earlier message. waiting_confirmation does not mean a human decision is still missing: when human_satisfied is true, inspect the current result and accept it if all conditions hold. Do not simulate tool receipts. Submit changes through preview and apply. When only workers or humans can act, finish this turn and wait. Never approve as a human. Use tools for every actual operation."},
                    "root":root.map(|r|json!({"id":r.id,"plan_revision_id":r.data["active_plan_revision_id"],"status":r.data["status"]})),"policy":controller.policy,"decisions":visible_decisions,"user_messages":user.iter().map(|r|&r.data["input"]).collect::<Vec<_>>()});
                controller.event_cursor+=1;controller.wakeup_turns+=1;controller.last_state_digest=digest;
                let payload=json!({"client_message_id":format!("controller:{}:{}:{}",row.id,controller.epoch,controller.event_cursor),"input":scope_snapshot.to_string(),"controller_epoch":controller.epoch,"controller_input_refs":user.iter().map(|r|&r.id).collect::<Vec<_>>()});
                let wakeup_id=payload["client_message_id"].as_str().unwrap().to_owned();
                if !s.orchestration_wakeups.iter().any(|w|w.id==wakeup_id){s.orchestration_wakeups.push(OrchestrationWakeup{id:wakeup_id,controller_id:row.id.clone(),event_cursor:controller.event_cursor,status:"prepared".into(),turn_id:None,created_at:now()});}
                if let Some(lease)=s.orchestration_leases.iter_mut().find(|l|l.controller_id==row.id){lease.epoch=controller.epoch;lease.root_instance_id=controller.root_instance_id.clone();lease.expires_at=now()+controller.policy.timeout_seconds*1000;}
                intent(s,&row,controller.epoch,"conversation.send",&sid,payload);
                for input in user{let r=s.governance.iter_mut().find(|r|r.id==input.id).unwrap();let mut d=r.data.clone();d["consumed"]=json!(true);update_record(r,d);}
                update_record(s.governance.iter_mut().find(|r|r.id==row.id).unwrap(),json!(controller));
            }
            for wakeup in &mut s.orchestration_wakeups{
                if let Some(turn)=s.turns.iter().find(|t|t.client_message_id==wakeup.id){wakeup.turn_id=Some(turn.id.clone());wakeup.status=if turn.consumed{"consumed"}else{"dispatched"}.into();}
            }
            Ok(Value::Null)
        })?;
        Ok(())
    }

    fn drive_controller_calls(&self) -> Result<()> {
        let snapshot = self.store.snapshot()?;
        for call in snapshot
            .governance
            .iter()
            .filter(|r| r.r#type == "controller_tool_call")
        {
            let controller: WorkflowController =
                snapshot.configuration(&call.definition_id)?.decode()?;
            if call.data["status"] == "prepared" {
                let method = call.data["method"].as_str().unwrap_or("");
                let args = call.data["arguments"].clone();
                let result = (|| -> Result<Value> {
                    if MANAGER_TOOLS.contains(&method) {
                        return self.manager_tool(&controller, call, method, &args);
                    }
                    require(
                        controller.status == "active" && call.data["epoch"] == controller.epoch,
                        "编排租约已失效",
                    )?;
                    require(
                        snapshot.orchestration_leases.iter().any(|l| {
                            l.controller_id == call.definition_id
                                && l.epoch == controller.epoch
                                && l.expires_at >= now()
                        }),
                        "编排租约已过期",
                    )?;
                    controller.policy.permits(operation(method))?;
                    if controller
                        .policy
                        .human_operations
                        .contains(operation(method))
                    {
                        let mut authorized = args.clone();
                        authorized
                            .as_object_mut()
                            .ok_or_else(|| err("invalid_params", "参数必须是对象"))?
                            .remove("decision_ref");
                        let decision_ref = args["decision_ref"].as_str().unwrap_or("");
                        let approved = snapshot.configuration(decision_ref)?;
                        require(
                            approved.r#type == "workflow_human_decision"
                                && approved.data["approved"] == true
                                && approved.data["actor"]["kind"] == "human"
                                && approved.data["plan_revision_id"] == args["plan_revision_id"]
                                && approved.data["request"]["operation"] == method
                                && approved.data["request"]["arguments"] == authorized,
                            "该操作需要对应确切参数的人工批准",
                        )?;
                    }
                    let root = controller.root_instance_id.clone();
                    let target = args["instance_id"]
                        .as_str()
                        .or_else(|| args["id"].as_str())
                        .map(str::to_owned)
                        .or(root.clone());
                    if let Some(target) = &target {
                        let r = snapshot.configuration(target)?;
                        require(
                            r.r#type == "workflow_instance"
                                && r.data["root_instance_id"].as_str() == root.as_deref(),
                            "目标不属于编排根实例",
                        )?;
                    }
                    if method == "workflow.inspect" && root.is_none() {
                        return Ok(
                            json!({"owner_id":controller.owner_id,"configuration":snapshot.governance.iter().filter(|r|r.r#type=="project_configuration"&&r.definition_id==controller.project_id).max_by_key(|r|r.version),"allowed_actions":["start"]}),
                        );
                    }
                    if control_query(method) {
                        let mut p = args;
                        p["id"] = json!(target);
                        p["project_id"] = json!(controller.project_id);
                        let mut value = self
                            .workflow_control_query(method, &p)
                            .ok_or_else(|| err("method_not_found", "未知编排查询"))??;
                        if method == "workflow.inspect" && p["decision_ref"].is_null() {
                            // Input bodies can contain every earlier business document.
                            // The manager reads those through execution.inspect parts;
                            // status inspection must retain the actual gate conditions.
                            for execution in value["runtime"]["steps"]
                                .as_array_mut()
                                .into_iter()
                                .flatten()
                            {
                                execution.as_object_mut().unwrap().remove("inputs");
                            }
                            for step in value["steps"].as_array_mut().into_iter().flatten() {
                                step["execution"].as_object_mut().unwrap().remove("inputs");
                            }
                            value["input_parts"] = json!("workflow.execution.inspect: part=inputs");
                        }
                        if method == "workflow.change.preview" {
                            value.as_object_mut().unwrap().remove("document");
                            if value.to_string().len() > 128 * 1024 {
                                value.as_object_mut().unwrap().remove("changeset");
                                value["changeset_in_request"] = json!(true);
                            }
                        }
                        require(
                            value.to_string().len() <= 256 * 1024,
                            "查询结果过大，请缩小 limit，或用 execution.inspect 的 part=inputs/contract、outputs.read 分块读取",
                        )?;
                        return Ok(value);
                    }
                    let actor = WorkflowActor::Controller {
                        controller_id: call.definition_id.clone(),
                        session_id: text(&call.data, "session_id")?,
                        turn_id: text(&call.data, "turn_id")?,
                        epoch: controller.epoch,
                    };
                    let target = if method == "workflow.start" {
                        controller.owner_id.clone()
                    } else {
                        target.ok_or_else(|| err("not_found", "尚未启动流程"))?
                    };
                    let original = configuration_hash(&(method, &args, &actor));
                    self.store.transact(Some((&format!("{}:action",call.id),&original)),&mut|s|{
                        let current:WorkflowController=s.configuration(&call.definition_id)?.decode()?;
                        require(current.epoch==controller.epoch&&current.status=="active","编排控制器已失效")?;
                        let turn=s.turns.iter().find(|t|t.id==call.data["turn_id"]&&t.status==TurnStatus::Running).ok_or_else(||err("tool_not_authorized","原生调用轮次已结束"))?;
                        require(s.executions.iter().any(|e|e.session_id==turn.session_id&&e.operation_id==call.data["worker_operation_id"]&&!e.writer_revoked),"原生调用 worker 已停止")?;
                        let revision=s.configuration(&target).ok().map(|r|r.revision).or_else(||revision(s,&target));
                        if method!="workflow.start"{require(args["plan_revision_id"]==s.configuration(&target)?.data["active_plan_revision_id"],"活动计划已变化")?;}
                        let c=Command{command_id:format!("{}:action",call.id),target:Some(target.clone()),expected_revision:revision,payload:args.clone()};
                        self.authorize_driver_action(s,method,&c,&actor)?;
                        InvocationContext::workflow(actor.clone(),Some(controller.project_id.clone())).record(s,method,&c)?;
                        let result=if let Some(v)=self.workflow_control_command(method,&c,s,&actor)?{v}else{require(!["workflow.step.confirm","workflow.step.approve"].contains(&method),"Agent 无权签署人工决定")?;let result=self.configured_runtime_command(if method=="workflow.execution.reconcile"{"workflow.step.reconcile"}else{method},&c,s)?.ok_or_else(||err("method_not_found","未知编排命令"))?;crate::orchestration::record_auxiliary_decision(s,method,&c,&actor,&result)?;result};
                        if method=="workflow.start"{let mut next=current;next.root_instance_id=Some(text(&result,"id")?);if let Some(lease)=s.orchestration_leases.iter_mut().find(|l|l.controller_id==call.definition_id){lease.root_instance_id=next.root_instance_id.clone();}update_record(s.governance.iter_mut().find(|r|r.id==call.definition_id).unwrap(),json!(next));}
                        Ok(result)
                    })
                })();
                self.store.transact(None, &mut |s| {
                    let row = s.governance.iter_mut().find(|r| r.id == call.id).unwrap();
                    let mut d = row.data.clone();
                    match &result {
                        Ok(v) => {
                            d["status"] = json!("succeeded");
                            d["receipt"] = v.clone();
                        }
                        Err(e) => {
                            d["status"] = json!("failed");
                            d["error"] = json!(e);
                        }
                    }
                    update_record(row, d);
                    Ok(Value::Null)
                })?;
                continue;
            }
            if !["succeeded", "failed"].contains(&call.data["status"].as_str().unwrap_or(""))
                || snapshot
                    .governance
                    .iter()
                    .any(|r| r.r#type == "controller_tool_reply" && r.definition_id == call.id)
            {
                continue;
            }
            let session = snapshot
                .sessions
                .iter()
                .find(|s| s.id == call.data["session_id"])
                .ok_or_else(|| err("storage", "编排会话缺失"))?;
            let active = snapshot.executions.iter().any(|e| {
                e.session_id == session.id
                    && e.operation_id == call.data["worker_operation_id"]
                    && !e.writer_revoked
            });
            let success = call.data["status"] == "succeeded";
            let value = if success {
                call.data["receipt"].clone()
            } else {
                call.data["error"].clone()
            };
            let value = if success {
                native_reply_value(call.data["method"].as_str().unwrap_or(""), &call.id, value)
            } else {
                value
            };
            let receipt = ToolReceipt {
                invocation_id: call.id.clone(),
                success,
                // The Codex bridge renders structured_content as inputText.
                // Supplying the same text too doubles every inspection result.
                content: if success {
                    vec![]
                } else {
                    vec![json!({"type":"text","text":value.to_string()})]
                },
                structured_content: success.then_some(value.clone()),
                remote_effect: if success { "applied" } else { "not_applied" }.into(),
                error: (!success).then_some(value),
                origin: Origin::External,
            };
            if !active
                || self
                    .harness
                    .answer_tool_call(session, &call.data["request_id"], &receipt)
                    .is_ok()
            {
                self.store.transact(None,&mut|s|{if !s.governance.iter().any(|r|r.r#type=="controller_tool_reply"&&r.definition_id==call.id){s.governance.push(record("controller_tool_reply",call.id.clone(),call.scope.clone(),"编排原生回复".into(),1,None,json!({"status":if active{"delivered"}else{"worker_unavailable"},"receipt":receipt}),true));}Ok(Value::Null)})?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod reply_tests {
    use super::*;

    #[test]
    fn large_inspection_retains_real_gate_conditions_in_the_native_reply() {
        let result = json!({"steps":[{"step":"review","human_satisfied":true}],"contract":"x".repeat(160 * 1024)});
        assert_eq!(
            native_reply_value("workflow.inspect", "source", result.clone()),
            result
        );
        let mutation = native_reply_value("workflow.change.apply", "source", result);
        assert_eq!(mutation["receipt_ref"], "source");
        assert_eq!(mutation["accepted"], true);
    }
}
