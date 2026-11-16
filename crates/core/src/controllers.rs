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
    "workflow.controller.wait",
    "workflow.controller.notify",
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

struct ControllerReply {
    call: ConfigurationRecord,
    session: Session,
    active: bool,
}
fn controller_reply(
    state: &EngineeringState,
    call: &ConfigurationRecord,
) -> Result<Option<ControllerReply>> {
    if !matches!(call.data["status"].as_str(), Some("succeeded" | "failed"))
        || state
            .governance
            .iter()
            .any(|r| r.r#type == "controller_tool_reply" && r.definition_id == call.id)
    {
        return Ok(None);
    }
    let session = state
        .sessions
        .iter()
        .find(|s| s.id == call.data["session_id"])
        .ok_or_else(|| err("storage", "编排会话缺失"))?;
    Ok(Some(ControllerReply {
        call: call.clone(),
        session: session.clone(),
        active: state.executions.iter().any(|e| {
            e.session_id == session.id
                && e.operation_id == call.data["worker_operation_id"]
                && !e.writer_revoked
        }),
    }))
}

fn operation(method: &str) -> &str {
    match method {
        "workflow.events.read" | "workflow.outputs.read" | "workflow.execution.inspect" => {
            "inspect"
        }
        "workflow.materials.import" => "import",
        "workflow.decision.request" => "request_decision",
        "workflow.segment.dispatch" => "segment",
        "workflow.controller.wait" | "workflow.controller.notify" => "inspect",
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
        json!({"step":execution["step"],"id":execution["id"],"status":execution["status"],"outputs":execution["outputs"],"input_digest":execution["input_digest"],"reason":execution["reason"],"name":definition.map(|step| &step["name"]),"session_id":execution["session_id"],"turn_id":execution["turn_id"],"confirmation_policy":policy,"human_satisfied":policy!="human"||confirmation.is_some()||waiver.is_some(),"confirmation_status":disposition,"confirmation_ref":confirmation.map(|record|&record.id),"confirmation_waiver_ref":waiver.map(|record|&record.id)})
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

fn turn_failure_reason(state: &EngineeringState, turn: &str, fallback: &str) -> String {
    state
        .conversation_events
        .iter()
        .find(|event| event.deduplication_key == format!("{turn}:execution-failure"))
        .and_then(|event| serde_json::from_str::<Error>(&event.content).ok())
        .map(|failure| failure.to_string())
        .unwrap_or_else(|| fallback.into())
}

// Union of durable native tool wait intervals; overlapping requests count once.
// This diagnoses the existing wall-clock limit without changing a frozen budget.
fn controller_platform_wait_ms(state: &EngineeringState, turn: &str, start: u64, end: u64) -> u64 {
    let mut intervals: Vec<_> = state
        .governance
        .iter()
        .filter(|row| row.r#type == "controller_tool_call" && row.data["turn_id"] == turn)
        .map(|call| {
            let reply = state
                .governance
                .iter()
                .find(|row| row.r#type == "controller_tool_reply" && row.definition_id == call.id);
            (
                call.created_at.max(start).min(end),
                reply.map_or(end, |row| row.created_at).max(start).min(end),
            )
        })
        .filter(|(a, b)| a < b)
        .collect();
    intervals.sort_unstable();
    let (mut total, mut last) = (0u64, start);
    for (a, b) in intervals {
        total = total.saturating_add(b.saturating_sub(a.max(last)));
        last = last.max(b);
    }
    total
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

fn update_controller(row: &mut ConfigurationRecord, data: Value) {
    if row.data != data {
        update_record(row, data);
    }
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
    pub(crate) fn cancel_bound_controller(
        &self,
        state: &mut EngineeringState,
        session: &str,
        command_id: &str,
    ) -> Result<()> {
        let Some(bound) = binding(state, session).cloned() else {
            return Ok(());
        };
        let Some(row) = state
            .governance
            .iter()
            .find(|row| {
                row.id == bound.data["controller_id"]
                    && row.data["session_id"] == session
                    && row.data["epoch"] == bound.data["epoch"]
                    && row.data["status"] == "active"
            })
            .cloned()
        else {
            return Ok(());
        };
        self.controller_command(
            "orchestration.takeover",
            &Command {
                command_id: format!("{command_id}:cancel-manager"),
                target: Some(row.id),
                expected_revision: Some(row.revision),
                payload: json!({"epoch":row.data["epoch"]}),
            },
            state,
        )?;
        Ok(())
    }

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
                    || c.payload["harness_permission_ref"]
                        .as_str()
                        .is_some_and(|id| existing.data["harness_permission_ref"] != id);
                if !replacing {
                    crate::configuration::project_manager_role(
                        s,
                        &text(&existing.data, "role_ref")?,
                    )?;
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
            let role = crate::configuration::project_manager_role(s, role)?;
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
                "编排授权必须通过 Permission Profile 源修订选择",
            )?;
            let context_ref = c.payload["harness_permission_ref"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| {
                    permission_selection(s)
                        .and_then(|choice| {
                            resolve_permission_lock(s, &composition.harness_ref, &choice)
                        })
                        .ok()
                        .map(|lock| lock.harness.profile.id)
                })
                .ok_or_else(|| err("permission_unconfigured", "请选择编排授权"))?;
            let context = crate::governance::reference(
                s,
                &context_ref,
                "permission_profile",
                &Scope::project(project.clone()),
            )?;
            let scheduling = root
                .as_ref()
                .and_then(|r| r.data["plan"]["document"]["scheduling"]["agent"].as_object())
                .map(|v| serde_json::from_value::<AgentSchedulingBudget>(json!(v)))
                .transpose()
                .map_err(|e| err("invalid_source", e.to_string()))?;
            let scheduling = if root.is_none() && c.payload["budget"].is_object() {
                Some(
                    serde_json::from_value::<AgentSchedulingBudget>(c.payload["budget"].clone())
                        .map_err(|e| err("invalid_params", e.to_string()))?,
                )
            } else {
                scheduling
            };
            let policy = profile_control_policy(s, &context.id, scheduling.as_ref())?;
            policy.validate()?;
            let controller = WorkflowController {
                workspace_only,
                project_id: project.clone(),
                owner_id: owner,
                root_instance_id: root.as_ref().map(|r| r.id.clone()),
                agent_revision_id: agent.id.clone(),
                role_ref: role.id.clone(),
                harness_permission_ref: context_ref,
                policy_digest: configuration_hash(&policy),
                policy,
                session_id: None,
                predecessor_session_id: predecessor,
                snapshot_id: snapshot,
                epoch: 1,
                status: "active".into(),
                event_cursor: 0,
                wakeup_turns: 0,
                source_cursor: 0,
                acknowledged_cursor: 0,
                delivery_id: String::new(),
                run_state: "runnable".into(),
                checkpoint: Value::Null,
                wait_deadline: None,
                wait_turn_id: None,
                bootstrap_epoch: 0,
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
                if !workspace_only
                    && let Some(driver) = s.governance.iter_mut().find(|r| {
                        r.r#type == "workflow_driver"
                            && Some(r.definition_id.as_str())
                                == controller.root_instance_id.as_deref()
                            && r.data["mode"] == "agent"
                    })
                {
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
                s.governance.push(record("controller_selection",successor.clone(),Scope::project(controller.project_id.clone()),"人类采用后继经理".into(),1,None,json!({"predecessor_controller_id":previous,"successor_controller_id":successor,"agent_ref":controller.agent_revision_id,"harness_permission_ref":controller.harness_permission_ref}),true));
            }
            return Ok(Some(out));
        }
        let row = s.configuration(target)?.clone();
        require(row.r#type == "workflow_controller", "不是编排控制器")?;
        let mut controller: WorkflowController = row.decode()?;
        if method != "orchestration.takeover" {
            crate::configuration::project_manager_role(s, &controller.role_ref)?;
        }
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
                    "编排授权必须通过 Permission Profile 源修订选择",
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
                for field in ["agent_ref", "harness_permission_ref"] {
                    if let Some(value) = c.payload.get(field) {
                        require(value.is_string(), "经理配置引用必须是确切源修订字符串")?;
                    }
                }
                let agent_ref = c.payload["agent_ref"]
                    .as_str()
                    .unwrap_or(&controller.agent_revision_id)
                    .to_owned();
                let context_ref = c.payload["harness_permission_ref"]
                    .as_str()
                    .unwrap_or(&controller.harness_permission_ref)
                    .to_owned();
                if agent_ref != controller.agent_revision_id
                    || context_ref != controller.harness_permission_ref
                {
                    require(
                        row.data["status"] != "active" || controller.session_id.is_none(),
                        "请先暂停旧经理再采用后继配置",
                    )?;
                    return self.controller_command(
                        if controller.workspace_only { "manager.open" } else { "orchestration.open" },
                        &Command { command_id:format!("{}:successor",c.command_id), target:Some(controller.root_instance_id.clone().unwrap_or_else(||controller.owner_id.clone())), expected_revision:None,
                            payload:json!({"role_ref":controller.role_ref,"agent_ref":agent_ref,"harness_permission_ref":context_ref}) }, s,
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
                controller.run_state = "runnable".into();
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
        let role = crate::configuration::project_manager_role(&s, &controller.role_ref)?;
        require(
            agent.role == role.definition_id,
            "编排 Agent 必须属于项目经理角色",
        )?;
        let permissions = resolve_permission_lock(
            &s,
            &agent.harness_ref,
            &permission_selection_with_profile(&s, &controller.harness_permission_ref)?,
        )?;
        let lock = freeze_integrations(&s, &agent, Some(&controller.project_id))?;
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
        crate::configuration::project_manager_role(s, &controller.role_ref)?;
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
        if !self
            .store
            .workflow_runtime_pending(ports::WorkflowRuntimeWork::Controllers)?
        {
            return Ok(());
        }
        let _span = self.store.observe_query("runtime.controllers");
        self.store.transact(None,&mut|s|{
            let rows:Vec<_>=s.governance.iter().filter(|r|r.r#type=="workflow_controller").cloned().collect();
            for row in rows{
                let mut controller:WorkflowController=row.decode()?;
                if controller.status!="active"{continue;}
                if !crate::manager_workspace::session_instance_current(s,&controller) {
                    if s.governance.iter().any(|call|call.r#type=="controller_tool_call"&&call.definition_id==row.id&&!s.governance.iter().any(|reply|reply.r#type=="controller_tool_reply"&&reply.definition_id==call.id)){continue;}
                    for turn in s.turns.iter_mut().filter(|t|Some(&t.session_id)==controller.session_id.as_ref()&&t.status.active()){turn.cancel_requested=true;turn.revision+=1;}
                    controller.epoch+=1;controller.status="paused".into();controller.reason="经理实例已换绑/移除，当前冻结会话已撤权；需要人类建立后继会话".into();
                    update_controller(s.governance.iter_mut().find(|r|r.id==row.id).unwrap(),json!(controller));continue;
                }
                if controller.session_id.is_none(){
                    let opened=s.governance.iter().rev().find(|r|r.r#type=="workflow_intent"&&r.definition_id==row.id&&r.data["method"]=="session.open"&&r.data["command"]["payload"]["controller_epoch"]==controller.epoch).cloned();
                    if let Some(opened)=opened{
                        if let Some(sid)=opened.data["result"]["id"].as_str(){
                            controller.session_id=Some(sid.into());
                            if binding(s,sid).is_none(){s.governance.push(record("controller_session",sid.into(),row.scope.clone(),"项目经理会话".into(),1,None,json!({"controller_id":row.id,"epoch":controller.epoch}),true));}
                        }else if opened.data["status"]=="failed"{controller.status="failed".into();controller.reason=opened.data["error"].to_string();}
                    }else{intent(s,&row,controller.epoch,"session.open",&controller.owner_id,json!({"agent":controller.agent_revision_id,"work_type":"orchestration","controller_id":row.id,"controller_epoch":controller.epoch,"force_new":true,"snapshot_id":controller.snapshot_id,"predecessor":controller.predecessor_session_id}));}
                    update_controller(s.governance.iter_mut().find(|r|r.id==row.id).unwrap(),json!(controller));continue;
                }
                let sid=controller.session_id.clone().unwrap();
                if let Some(turn)=s.turns.iter().find(|t|t.session_id==sid&&t.status.active()).cloned(){
                    if let Some(lease)=s.orchestration_leases.iter_mut().find(|l|l.controller_id==row.id&&l.epoch==controller.epoch)&& lease.expires_at<now()+controller.policy.timeout_seconds*500 {lease.expires_at=now()+controller.policy.timeout_seconds*1000;}

                    if turn.status==TurnStatus::NeedsReconciliation {controller.status="needs_reconciliation".into();controller.reason=turn_failure_reason(s,&turn.id,"编排执行结果未知，请停止并核对后恢复");}
                    else if turn.started.is_some_and(|started|now().saturating_sub(started)>controller.policy.timeout_seconds*1000){let t=s.turns.iter_mut().find(|t|t.id==turn.id).unwrap();t.cancel_requested=true;t.revision+=1;controller.status="budget_exhausted".into();let end=now();let start=turn.started.unwrap();let wall=end.saturating_sub(start);let wait=controller_platform_wait_ms(s,&turn.id,start,end);controller.reason=format!("编排 Turn 超时，已请求停止；墙钟 {} ms，其中已登记平台请求等待 {} ms，其余区间 {} ms（不等同于模型推理时间）",wall,wait,wall.saturating_sub(wait));}
                    update_controller(s.governance.iter_mut().find(|r|r.id==row.id).unwrap(),json!(controller));continue;
                }
                // A failed Turn is terminal too. Without this projection the
                // state digest can remain unchanged and leave an idle manager
                // marked active forever after a rejected worker startup.
                if let Some(turn)=s.turns.iter().rev().find(|turn|turn.session_id==sid).filter(|turn|matches!(turn.status,TurnStatus::Failed|TurnStatus::Cancelled)) {
                    controller.status=if turn.status==TurnStatus::Cancelled {"paused"} else {"failed"}.into();
                    controller.reason=turn_failure_reason(s,&turn.id,if turn.status==TurnStatus::Cancelled {"项目经理轮次已取消，请恢复后建立后继会话"} else {"项目经理会话执行失败，请恢复后建立后继会话"});
                    update_controller(s.governance.iter_mut().find(|r|r.id==row.id).unwrap(),json!(controller));continue;
                }
                // A durable send can be awaiting Host dispatch before a Turn exists.
                // Coalesce new events until that exact send has been admitted.
                if s.governance.iter().any(|r|r.r#type=="workflow_intent"&&r.definition_id==row.id&&r.data["method"]=="conversation.send"&&r.data["status"]=="prepared"&&r.data["command"]["payload"]["controller_epoch"]==controller.epoch){continue;}
                let root=controller.root_instance_id.as_ref().and_then(|id|s.configuration(id).ok());
                if root.is_some_and(|r|["completed","cancelled"].contains(&r.data["status"].as_str().unwrap_or(""))){controller.status="closed".into();update_controller(s.governance.iter_mut().find(|r|r.id==row.id).unwrap(),json!(controller));continue;}
                let mut input_bytes=0;
                let user:Vec<_>=s.governance.iter().filter(|r|r.r#type=="controller_input"&&r.definition_id==row.id&&r.data["consumed"]==false).take(8).take_while(|r|{let size=r.data["input"].as_str().unwrap_or("").len();if input_bytes+size>65536{false}else{input_bytes+=size;true}}).cloned().collect();
                if let Some(failed)=s.governance.iter().rev().find(|r|r.r#type=="workflow_intent"&&r.definition_id==row.id&&r.data["method"]=="conversation.send"&&r.data["command"]["payload"]["controller_epoch"]==controller.epoch&&r.data["status"]=="failed").cloned() {
                    let refs=failed.data["command"]["payload"]["controller_input_refs"].as_array().cloned().unwrap_or_default();
                    for input in s.governance.iter_mut().filter(|r|r.r#type=="controller_input"&&r.definition_id==row.id&&refs.contains(&json!(r.id))) {let mut data=input.data.clone();data["consumed"]=json!(false);update_record(input,data);}
                    controller.status="failed".into();controller.reason=failed.data["error"].to_string();
                    update_controller(s.governance.iter_mut().find(|r|r.id==row.id).unwrap(),json!(controller));continue;
                }
                if controller.workspace_only && user.is_empty(){controller.run_state="waiting".into();update_controller(s.governance.iter_mut().find(|r|r.id==row.id).unwrap(),json!(controller));continue;}
                let bootstrap=controller.bootstrap_epoch!=controller.epoch;
                let events=if bootstrap {Vec::new()} else {crate::controller_events::pending_events(s,&controller)};
                let deadline=controller.wait_deadline.is_some_and(|d|d<=now());
                if !bootstrap && events.is_empty() && user.is_empty() && !deadline {
                    if let Some(turn)=s.turns.iter().rev().find(|t|t.session_id==sid)
                        && turn.status==TurnStatus::Completed && controller.wait_turn_id.as_deref()!=Some(&turn.id) {
                        controller.run_state="needs_attention".into();
                        controller.reason="项目经理结束本轮但未登记等待；请核对经理工作记录".into();
                        update_controller(s.governance.iter_mut().find(|r|r.id==row.id).unwrap(),json!(controller));
                    }
                    continue;
                }
                let progress=events.iter().any(|e| {
                    let source=&e["source"];
                    source["origin_turn_id"].is_null() && (source["change"]["kind"]=="human.confirmed"
                        || (source["change"]["kind"]=="decision.changed" && source["change"]["status"]!="pending")
                        || source["change"]["has_outputs"]==true
                        || source["change"]["changes"].as_array().into_iter().flatten().any(|c|c["execution"]["outputs"].as_object().is_some_and(|v|!v.is_empty())))
                });
                if progress||!user.is_empty(){controller.wakeup_turns=0;}
                if controller.wakeup_turns>=controller.policy.max_turns_per_wakeup{controller.status="budget_exhausted".into();controller.reason="编排唤醒预算已耗尽".into();update_controller(s.governance.iter_mut().find(|r|r.id==row.id).unwrap(),json!(controller));continue;}
                controller.event_cursor+=1;
                controller.delivery_id=format!("controller:{}:{}:{}",row.id,controller.epoch,controller.event_cursor);
                let mut scope_snapshot=json!({"renderer_version":ORCHESTRATION_RENDERER,"delivery_id":controller.delivery_id,
                    "controller_id":row.id,"epoch":controller.epoch,"owner_id":controller.owner_id,
                    "root_instance_id":controller.root_instance_id,"project_id":controller.project_id,
                    "workspace_only":controller.workspace_only,"bootstrap":bootstrap,"events":events,
                    "deadline_reached":deadline,"user_messages":user.iter().map(|r|&r.data["input"]).collect::<Vec<_>>()});
                if bootstrap {
                    scope_snapshot["root"]=root.map(|r|json!({"id":r.id,"plan_revision_id":r.data["active_plan_revision_id"],"status":r.data["status"],"human_governance":r.data["human_governance"],"user_request":r.data["user_request"]})).unwrap_or(Value::Null);
                    scope_snapshot["policy"]=json!(controller.policy);
                    scope_snapshot["checkpoint"]=controller.checkpoint.clone();
                    let mut instances:Vec<_>=s.governance.iter().filter(|r|r.r#type=="workflow_instance"&&Some(r.data["root_instance_id"].as_str().unwrap_or(""))==controller.root_instance_id.as_deref()).take(16).map(|r|instance_summary(s,r)).collect();
                    for instance in &mut instances {if let Some(steps)=instance["steps"].as_array_mut(){let total=steps.len();steps.sort_by_key(step_attention);steps.truncate(16);instance["step_total"]=json!(total);}}
                    scope_snapshot["instances"]=json!(instances);
                    controller.bootstrap_epoch=controller.epoch;
                    controller.source_cursor=s.events.last().map_or(0,|e|e.id);
                }
                if let Some(cursor)=events.last().and_then(|e|e["id"].as_u64()){controller.source_cursor=cursor;}
                controller.wakeup_turns+=1;
                controller.run_state="running".into();
                controller.wait_deadline=None;
                controller.reason.clear();
                let payload=json!({"client_message_id":format!("controller:{}:{}:{}",row.id,controller.epoch,controller.event_cursor),"input":scope_snapshot.to_string(),"controller_epoch":controller.epoch,"controller_input_refs":user.iter().map(|r|&r.id).collect::<Vec<_>>()});
                let wakeup_id=payload["client_message_id"].as_str().unwrap().to_owned();
                if !s.orchestration_wakeups.iter().any(|w|w.id==wakeup_id){s.orchestration_wakeups.push(OrchestrationWakeup{id:wakeup_id,controller_id:row.id.clone(),event_cursor:controller.event_cursor,status:"prepared".into(),turn_id:None,created_at:now()});}
                if let Some(lease)=s.orchestration_leases.iter_mut().find(|l|l.controller_id==row.id){lease.epoch=controller.epoch;lease.root_instance_id=controller.root_instance_id.clone();lease.expires_at=now()+controller.policy.timeout_seconds*1000;}
                intent(s,&row,controller.epoch,"conversation.send",&sid,payload);
                for input in user{let r=s.governance.iter_mut().find(|r|r.id==input.id).unwrap();let mut d=r.data.clone();d["consumed"]=json!(true);update_record(r,d);}
                update_controller(s.governance.iter_mut().find(|r|r.id==row.id).unwrap(),json!(controller));
            }
            for wakeup in &mut s.orchestration_wakeups{
                if let Some(turn)=s.turns.iter().find(|t|t.client_message_id==wakeup.id){wakeup.turn_id=Some(turn.id.clone());wakeup.status=if turn.consumed{"consumed"}else{"dispatched"}.into();}
            }
            Ok(Value::Null)
        })?;
        Ok(())
    }

    fn drive_controller_calls(&self) -> Result<()> {
        let _span = self.store.observe_query("runtime.controller_calls");
        if !self
            .store
            .workflow_runtime_pending(ports::WorkflowRuntimeWork::ControllerCalls)?
        {
            return Ok(());
        }
        let snapshot = self.store.snapshot()?;
        let replied: std::collections::BTreeSet<_> = snapshot
            .governance
            .iter()
            .filter(|r| r.r#type == "controller_tool_reply")
            .map(|r| r.definition_id.as_str())
            .collect();
        for call in snapshot.governance.iter().filter(|r| {
            r.r#type == "controller_tool_call"
                && (r.data["status"] == "prepared"
                    || (!replied.contains(r.id.as_str())
                        && matches!(r.data["status"].as_str(), Some("succeeded" | "failed"))))
        }) {
            let _call_span = self
                .store
                .observe_native_call(&call.id, "native.call.total");
            let controller: WorkflowController =
                snapshot.configuration(&call.definition_id)?.decode()?;
            if call.data["status"] == "prepared" {
                let method = call.data["method"].as_str().unwrap_or("");
                let args = call.data["arguments"].clone();
                let result = (|| -> Result<Value> {
                    let _span = self
                        .store
                        .observe_native_call(&call.id, "native.call.action");
                    // A Host can exit after committing the action and before recording
                    // its native result. Recover that exact receipt even after takeover;
                    // never relabel a committed action as not applied or re-execute it.
                    if !control_query(method) && !MANAGER_TOOLS.contains(&method) {
                        let actor = WorkflowActor::Controller {
                            controller_id: call.definition_id.clone(),
                            session_id: text(&call.data, "session_id")?,
                            turn_id: text(&call.data, "turn_id")?,
                            epoch: call.data["epoch"]
                                .as_u64()
                                .ok_or_else(|| err("storage", "调用缺少经理代次"))?,
                        };
                        let fingerprint = configuration_hash(&(method, &args, &actor));
                        if let Some(receipt) = self
                            .store
                            .replay(&format!("{}:action", call.id), &fingerprint)?
                        {
                            return Ok(receipt);
                        }
                    }
                    require(
                        snapshot.turns.iter().any(|turn| {
                            turn.id == call.data["turn_id"]
                                && turn.status.active()
                                && !matches!(
                                    turn.status,
                                    TurnStatus::Queued | TurnStatus::NeedsReconciliation
                                )
                                && !turn.cancel_requested
                        }) && snapshot.executions.iter().any(|execution| {
                            execution.session_id == call.data["session_id"]
                                && execution.operation_id == call.data["worker_operation_id"]
                                && !execution.writer_revoked
                        }),
                        "原生调用者已停止或等待核对",
                    )?;
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
                        let result=if method=="workflow.controller.wait" {crate::controller_events::wait(s,&call.definition_id,&text(&call.data,"turn_id")?,&args)?}else if method=="workflow.controller.notify" {let interaction=crate::controller_interactions::validate(s,s.configuration(&target)?,&args["interaction"],&json!({}))?;let notice=record("controller_interaction",call.definition_id.clone(),call.scope.clone(),text(&args["interaction"],"title")?,1,None,json!({"instance_id":target,"plan_revision_id":args["plan_revision_id"],"turn_id":call.data["turn_id"],"interaction":interaction}),true);let id=notice.id.clone();s.governance.push(notice);json!({"id":id,"status":"published"})}else if let Some(v)=self.workflow_control_command(method,&c,s,&actor)?{v}else{require(!["workflow.step.confirm","workflow.step.approve"].contains(&method),"Agent 无权签署人工决定")?;let result=self.configured_runtime_command(if method=="workflow.execution.reconcile"{"workflow.step.reconcile"}else{method},&c,s)?.ok_or_else(||err("method_not_found","未知编排命令"))?;crate::orchestration::record_auxiliary_decision(s,method,&c,&actor,&result)?;result};
                        if method=="workflow.start"{let mut next=current;next.root_instance_id=Some(text(&result,"id")?);if let Some(lease)=s.orchestration_leases.iter_mut().find(|l|l.controller_id==call.definition_id){lease.root_instance_id=next.root_instance_id.clone();}update_record(s.governance.iter_mut().find(|r|r.id==call.definition_id).unwrap(),json!(next));}
                        Ok(result)
                    })
                })();
                let _commit = self
                    .store
                    .observe_native_call(&call.id, "native.call.result_commit");
                let mut delivery = None;
                self.store.transact(None, &mut |s| {
                    let row = s.governance.iter_mut().find(|r| r.id == call.id).unwrap();
                    let mut d = row.data.clone();
                    match &result {
                        Ok(v) => {
                            d["status"] = json!("succeeded");
                            d["finished_at"] = json!(now());
                            d["receipt"] = v.clone();
                        }
                        Err(e) => {
                            d["status"] = json!("failed");
                            d["finished_at"] = json!(now());
                            d["error"] = json!(e);
                        }
                    }
                    update_record(row, d);
                    // Derive this small delivery context from the same complete,
                    // authoritative transaction that records the result. It is
                    // usable only after commit succeeds, never as validator input.
                    delivery = controller_reply(s, s.configuration(&call.id)?)?;
                    Ok(Value::Null)
                })?;
                drop(_commit);
                // The durable result is authoritative before any native reply. Deliver it
                // in this drive, rather than waiting behind the remaining scheduler stages.
                if let Some(delivery) = delivery {
                    self.deliver_controller_reply(delivery)?;
                }
                continue;
            }
            self.deliver_controller_call(&snapshot, call)?;
        }
        Ok(())
    }
    fn deliver_controller_call(
        &self,
        snapshot: &EngineeringState,
        call: &ConfigurationRecord,
    ) -> Result<()> {
        if let Some(delivery) = controller_reply(snapshot, call)? {
            self.deliver_controller_reply(delivery)?;
        }
        Ok(())
    }
    fn deliver_controller_reply(&self, delivery: ControllerReply) -> Result<()> {
        let ControllerReply {
            call,
            session,
            active,
        } = delivery;
        let _span = self.store.observe_query("runtime.controller_reply");
        let _delivery = self
            .store
            .observe_native_call(&call.id, "native.call.delivery");
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
                .answer_tool_call(&session, &call.data["request_id"], &receipt)
                .is_ok()
        {
            self.store.transact(None,&mut|s|{if !s.governance.iter().any(|r|r.r#type=="controller_tool_reply"&&r.definition_id==call.id){s.governance.push(record("controller_tool_reply",call.id.clone(),call.scope.clone(),"编排原生回复".into(),1,None,json!({"status":if active{"delivered"}else{"worker_unavailable"},"receipt":receipt}),true));}Ok(Value::Null)})?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod reply_tests {
    use super::*;

    #[test]
    fn platform_wait_counts_overlaps_once_and_clamps_to_the_turn() {
        let mut state = EngineeringState::default();
        for (id, start, finish) in [("a", 10, Some(50)), ("b", 40, Some(70)), ("c", 80, None)] {
            let mut call = record(
                "controller_tool_call",
                "manager".into(),
                Scope::global(),
                id.into(),
                1,
                None,
                json!({"turn_id":"turn"}),
                false,
            );
            call.id = id.into();
            call.created_at = start;
            state.governance.push(call);
            if let Some(finish) = finish {
                let mut reply = record(
                    "controller_tool_reply",
                    id.into(),
                    Scope::global(),
                    "reply".into(),
                    1,
                    None,
                    json!({}),
                    true,
                );
                reply.created_at = finish;
                state.governance.push(reply);
            }
        }
        assert_eq!(controller_platform_wait_ms(&state, "turn", 20, 100), 70);
        assert_eq!(controller_platform_wait_ms(&state, "turn", 50, 60), 10);
        assert_eq!(controller_platform_wait_ms(&state, "another", 20, 100), 0);
    }

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
