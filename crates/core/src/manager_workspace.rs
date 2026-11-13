//! Project-bound manager tools. No tool accepts an arbitrary RPC method or database path.
use crate::governance::record;
use crate::*;
use std::sync::Arc;
pub const MANAGER_TOOLS: &[&str] = &[
    "spec.lint",
    "project.overview",
    "project.records.list",
    "project.record.read",
    "project.artifact.read",
    "project.resources",
    "configuration.catalog",
    "configuration.read",
    "configuration.usage",
    "configuration.change.preview",
    "configuration.change.apply",
    "configuration.transfer.stage",
    "configuration.transfer.apply",
    "configuration.transfer.cancel",
    "configuration.transfer.status",
];
pub struct ManagerServices {
    pub shared: Arc<Engine>,
    pub inventory: Arc<dyn ports::ProjectInventory>,
    pub gate: Arc<std::sync::Mutex<()>>,
}
pub(crate) fn manager_operation(method: &str) -> &str {
    match method {
        "spec.lint" | "configuration.change.preview" | "configuration.transfer.stage" => {
            "configuration.preview"
        }
        "configuration.change.apply"
        | "configuration.transfer.apply"
        | "configuration.transfer.cancel" => "configuration.apply",
        m if m.starts_with("configuration.") => "configuration.read",
        _ => "project.read",
    }
}
pub(crate) fn session_instance_current(
    s: &EngineeringState,
    controller: &WorkflowController,
) -> bool {
    let instance = controller
        .session_id
        .as_deref()
        .and_then(|id| s.sessions.iter().find(|session| session.id == id))
        .and_then(|session| session.runtime_binding.as_ref())
        .and_then(|binding| binding.agent_configuration.as_ref())
        .and_then(|a| s.configuration(&a.agent_instance_id).ok());
    instance.is_none_or(|instance| {
        !configuration_retired(s, instance)
            && !s.governance.iter().any(|r| {
                r.r#type == "agent_instance"
                    && r.definition_id == instance.definition_id
                    && r.version > instance.version
            })
    })
}
fn sanitize(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if [
                    "credential_ref",
                    "credential_reference",
                    "access_token",
                    "refresh_token",
                    "api_key",
                    "password",
                    "authorization",
                    "secret",
                ]
                .contains(&key.to_ascii_lowercase().as_str())
                {
                    *value = json!("[redacted]");
                } else {
                    sanitize(value);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                sanitize(item)
            }
        }
        _ => {}
    }
}
fn chunk(value: &Value, p: &Value) -> Result<Value> {
    let digest = configuration_hash(value);
    require(
        p["value_digest"].as_str().is_none_or(|d| d == digest),
        "读取来源已变化，请从起点重新读取",
    )?;
    let body = value.to_string();
    let offset = p["offset"].as_u64().unwrap_or(0) as usize;
    require(
        offset <= body.len() && body.is_char_boundary(offset),
        "分块游标越界",
    )?;
    let mut end = (offset + p["max_bytes"].as_u64().unwrap_or(16384).clamp(1024, 65536) as usize)
        .min(body.len());
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    Ok(
        json!({"redaction":"known_credential_fields","value_digest":digest,"offset":offset,"value_chunk":&body[offset..end],"total_bytes":body.len(),"next_offset":if end<body.len(){Some(end)}else{None},"complete":end==body.len()}),
    )
}
fn record_kinds() -> &'static [&'static str] {
    &[
        "projects",
        "features",
        "tasks",
        "documents",
        "sessions",
        "contexts",
        "snapshots",
        "heads",
        "artifacts",
        "events",
        "turns",
        "conversation_events",
        "interactions",
        "proposals",
        "executions",
        "repository_operations",
        "repository_worktrees",
        "governance",
        "orchestration_leases",
        "orchestration_wakeups",
    ]
}
fn rows(s: &EngineeringState, kind: &str, project: &str) -> Result<Vec<Value>> {
    require(record_kinds().contains(&kind), "未知项目资料类型")?;
    let value = serde_json::to_value(s).map_err(|e| err("serialization", e.to_string()))?;
    let mut rows = value[kind].as_array().cloned().unwrap_or_default();
    if kind == "governance" {
        rows.retain(|r| {
            serde_json::from_value::<ConfigurationRecord>(r.clone()).is_ok_and(|r| {
                if r.is_definition() {
                    configuration_visible(s, &r, Some(project))
                } else {
                    r.scope.visible_from(&Scope::project(project))
                }
            })
        });
    }
    for row in &mut rows {
        sanitize(row);
    }
    Ok(rows)
}
fn summary(row: &Value) -> Value {
    json!({"id":row["id"],"type":row["type"],"name":row.get("name").or_else(||row.get("title")),"revision":row.get("revision").or_else(||row.get("version")),"status":row.get("status").or_else(||row["data"].get("status")),"content_hash":row["content_hash"],"value_digest":configuration_hash(row)})
}
impl Engine {
    fn manager_project(&self, s: &EngineeringState, project: &str, p: &Value) -> Result<()> {
        require(
            self.store.project_id().as_deref() == Some(project)
                && s.projects.len() == 1
                && s.projects[0].id == project,
            "经理工作台需要绑定的单项目服务",
        )?;
        require(
            p["project_id"].as_str().is_none_or(|id| id == project),
            "工具参数不能改变经理项目范围",
        )
    }
    pub fn manager_knowledge(&self, method: &str, project: &str, p: &Value) -> Result<Value> {
        let s = self.store.snapshot()?;
        self.manager_project(&s, project, p)?;
        match method {
            "project.overview" => {
                let counts = record_kinds()
                    .iter()
                    .map(|kind| Ok((kind.to_string(), json!(rows(&s, kind, project)?.len()))))
                    .collect::<Result<serde_json::Map<String, Value>>>()?;
                Ok(
                    json!({"project":s.projects[0],"counts":counts,"record_kinds":record_kinds(),"overview_only":true,
                    "continuation":{"list":"project.records.list","read":"project.record.read","artifact":"project.artifact.read"},"snapshot_digest":configuration_hash(&counts)}),
                )
            }
            "project.records.list" | "configuration.catalog" => {
                let kind = if method == "configuration.catalog" {
                    "governance"
                } else {
                    p["kind"].as_str().unwrap_or("")
                };
                let mut records = rows(&s, kind, project)?;
                if method == "configuration.catalog" {
                    records.retain(|v| {
                        serde_json::from_value::<ConfigurationRecord>(v.clone())
                            .is_ok_and(|r| r.is_definition() || r.r#type == "agent_instance")
                    });
                }
                if let Some(family) = p["type"].as_str() {
                    records.retain(|r| r["type"] == family);
                }
                if let Some(search) = p["search"].as_str() {
                    require(search.len() <= 1024, "搜索过长")?;
                    records.retain(|r| r.to_string().contains(search));
                }
                records.sort_by_key(|r| r["id"].to_string());
                let digest = configuration_hash(&records);
                require(
                    p["snapshot_digest"].as_str().is_none_or(|d| d == digest),
                    "列表已变化，请重新分页",
                )?;
                let offset = p["offset"].as_u64().unwrap_or(0) as usize;
                let limit = p["limit"].as_u64().unwrap_or(32).clamp(1, 100) as usize;
                Ok(
                    json!({"kind":kind,"records":records.iter().skip(offset).take(limit).map(summary).collect::<Vec<_>>(),"total":records.len(),"offset":offset,"next_offset":if offset+limit<records.len(){Some(offset+limit)}else{None},"snapshot_digest":digest,"complete":offset+limit>=records.len()}),
                )
            }
            "project.record.read" | "configuration.read" => {
                let kind = if method == "configuration.read" {
                    "governance"
                } else {
                    p["kind"].as_str().unwrap_or("")
                };
                let id = text(p, "id")?;
                let records = rows(&s, kind, project)?;
                let record = records
                    .iter()
                    .find(|r| {
                        r["id"]
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| r["id"].to_string())
                            == id
                    })
                    .ok_or_else(|| err("not_found", "项目记录不存在或不可见"))?;
                let pointer = p["pointer"].as_str().unwrap_or("");
                let value = record
                    .pointer(pointer)
                    .ok_or_else(|| err("not_found", "记录字段不存在"))?;
                let mut result = chunk(value, p)?;
                result["record_ref"] = json!({"kind":kind,"id":id,"revision":record["revision"],"content_hash":record["content_hash"],"pointer":pointer});
                Ok(result)
            }
            "project.artifact.read" => {
                let id = text(p, "id")?;
                let artifact = s
                    .artifacts
                    .iter()
                    .find(|a| a.id == id)
                    .ok_or_else(|| err("not_found", "项目没有此产物引用"))?;
                let body = self.artifacts.read(artifact)?;
                require(body.len() <= 8 * 1024 * 1024, "产物超过经理分块读取上限")?;
                let text = String::from_utf8(body)
                    .map_err(|_| err("unsupported", "二进制产物需要专用查看器"))?;
                let mut result = chunk(&json!(text), p)?;
                result["artifact_ref"] = json!({"id":artifact.id,"sha256":artifact.sha256});
                Ok(result)
            }
            "project.resources" => {
                let actual = self
                    .manager_services
                    .as_ref()
                    .map(|v| v.inventory.runtime_facts())
                    .transpose()?
                    .unwrap_or(json!({"available":false}));
                chunk(
                    &json!({"drivers":driver_capabilities(),"runtime_facts":actual,"configurations":s.governance.iter().filter(|r|["harness","sandbox","model","model_provider","runtime_resource","permission_binding","permission_context"].contains(&r.r#type.as_str())).map(|r|json!({"reference":r.id,"type":r.r#type,"content_hash":r.content_hash})).collect::<Vec<_>>()}),
                    p,
                )
            }
            "configuration.usage" => Ok(
                json!({"reference":text(p,"id")?,"users":configuration_usage(&s,&text(p,"id")?)?,"identity_users":configuration_identity_usage(&s,&text(p,"id")?)?}),
            ),
            _ => Err(err("method_not_found", "未知经理查询")),
        }
    }
    fn manager_live(
        &self,
        s: &EngineeringState,
        controller: &WorkflowController,
        call: &ConfigurationRecord,
    ) -> Result<()> {
        let current: WorkflowController = s.configuration(&call.definition_id)?.decode()?;
        require(
            session_instance_current(s, &current),
            "经理实例已换绑/移除，需要后继会话",
        )?;
        require(
            current.status == "active"
                && current.epoch == controller.epoch
                && call.data["epoch"] == current.epoch,
            "经理会话已失效",
        )?;
        require(
            s.orchestration_leases.iter().any(|l| {
                l.controller_id == call.definition_id
                    && l.epoch == current.epoch
                    && l.expires_at >= now()
            }),
            "经理租约已过期",
        )?;
        let turn = s
            .turns
            .iter()
            .find(|t| {
                t.id == call.data["turn_id"]
                    && t.session_id == call.data["session_id"]
                    && t.status == TurnStatus::Running
            })
            .ok_or_else(|| err("tool_not_authorized", "经理原生调用轮次已结束"))?;
        require(
            s.executions.iter().any(|e| {
                e.session_id == turn.session_id
                    && e.operation_id == call.data["worker_operation_id"]
                    && !e.writer_revoked
            }),
            "经理 worker 已停止",
        )
    }
    fn manager_change_scope(
        &self,
        s: &EngineeringState,
        controller: &WorkflowController,
        change: &ConfigurationChangeSet,
        global: bool,
    ) -> Result<()> {
        if global {
            controller.policy.permits("configuration.global.write")?;
        }
        for operation in &change.operations {
            match operation {
                ConfigurationOperation::Create { source }
                | ConfigurationOperation::Revise { source, .. } => {
                    require(
                        source.scope
                            == if global {
                                Scope::global()
                            } else {
                                Scope::project(&controller.project_id)
                            },
                        "经理不能写入其他项目或隐式扩大范围",
                    )?;
                    if !global && source.r#type == "role" {
                        let role: RoleDefinition = serde_json::from_value(source.data.clone())
                            .map_err(|e| err("invalid_configuration", e.to_string()))?;
                        if let Some(visibility) = role.visibility {
                            require(
                                matches!(visibility,DefinitionVisibility::Projects{project_ids} if project_ids.iter().all(|id|id==&controller.project_id)),
                                "项目经理不能通过 Role 可见性扩展到其他项目",
                            )?;
                        }
                    }
                }
                ConfigurationOperation::SelectProject { project_id, .. } => require(
                    !global && *project_id == controller.project_id,
                    "经理不能采用其他项目配置",
                )?,
                ConfigurationOperation::SelectWork { owner_id, .. } => {
                    require(!global, "共享目录不能选择项目工作")?;
                    require(
                        work_reference(s, owner_id)?.scope(s)?.project_id()
                            == Some(controller.project_id.as_str()),
                        "工作不属于经理项目",
                    )?;
                }
                ConfigurationOperation::Retire { reference, .. } => require(
                    s.configuration(reference)?.scope
                        == if global {
                            Scope::global()
                        } else {
                            Scope::project(&controller.project_id)
                        },
                    "停用目标不属于授权配置库",
                )?,
                ConfigurationOperation::PermissionBindings { bindings } => {
                    let local = self.store.snapshot()?;
                    let agent = local.resolve_agent(&controller.agent_revision_id)?;
                    let own_sandbox = local
                        .configuration(&controller.permission_context_ref)?
                        .refs
                        .get("sandbox_ref")
                        .and_then(Value::as_str);
                    require(
                        !bindings.iter().any(|b| {
                            b["target"]["revision_id"] == agent.harness_ref
                                || b["target"]["revision_id"].as_str() == own_sandbox
                        }),
                        "经理不能换绑当前会话的权限目标；由人类执行后建立后继会话",
                    )?;
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub(crate) fn manager_tool(
        &self,
        controller: &WorkflowController,
        call: &ConfigurationRecord,
        method: &str,
        p: &Value,
    ) -> Result<Value> {
        let _gate = self
            .manager_services
            .as_ref()
            .map(|v| {
                v.gate
                    .lock()
                    .map_err(|_| err("storage", "配置写入锁不可用"))
            })
            .transpose()?;
        let snapshot = self.store.snapshot()?;
        self.manager_project(&snapshot, &controller.project_id, p)?;
        let actor = WorkflowActor::Controller {
            controller_id: call.definition_id.clone(),
            session_id: text(&call.data, "session_id")?,
            turn_id: text(&call.data, "turn_id")?,
            epoch: call.data["epoch"].as_u64().unwrap_or(0),
        };
        let action_id = format!("{}:manager-action", call.id);
        let global = p["scope"] == "global";
        let target = if global {
            self.manager_services
                .as_ref()
                .ok_or_else(|| err("unsupported", "未装配共享配置服务"))?
                .shared
                .as_ref()
        } else {
            self
        };
        let fingerprint = configuration_hash(&(method, p, &actor));
        // An already committed write is reconciled even if the initiating session has since ended.
        if method == "configuration.change.apply" {
            if let Some(result) = target.store.replay(&action_id, &fingerprint)? {
                return Ok(result);
            }
        }
        self.manager_live(&snapshot, controller, call)?;
        controller.policy.permits(manager_operation(method))?;
        require(
            !controller
                .policy
                .human_operations
                .contains(manager_operation(method)),
            "配置策略要求人类执行此操作",
        )?;
        if method == "spec.lint" {
            return self.specification_lint(p);
        }
        if method.starts_with("configuration.transfer.") {
            controller.policy.permits("configuration.global.write")?;
            let shared = &self
                .manager_services
                .as_ref()
                .ok_or_else(|| err("unsupported", "未装配共享配置服务"))?
                .shared;
            if method == "configuration.transfer.stage" {
                let global: ConfigurationChangeSet =
                    serde_json::from_value(p["global_changeset"].clone())
                        .map_err(|e| err("invalid_params", e.to_string()))?;
                let local: ConfigurationChangeSet =
                    serde_json::from_value(p["project_changeset"].clone())
                        .map_err(|e| err("invalid_params", e.to_string()))?;
                self.manager_change_scope(&shared.store.snapshot()?, controller, &global, true)?;
                self.manager_change_scope(&snapshot, controller, &local, false)?;
                return shared.stage_configuration_transfer_as(
                    self,
                    Command {
                        command_id: action_id,
                        target: None,
                        expected_revision: None,
                        payload: p.clone(),
                    },
                    &actor,
                );
            }
            let id = text(p, "id")?;
            let transfer = shared.store.snapshot()?.configuration(&id)?.clone();
            require(
                transfer.r#type == "configuration_transfer"
                    && transfer.data["project_id"] == controller.project_id,
                "跨库操作不属于经理项目",
            )?;
            let mut owner = transfer.data["actor"].clone();
            let mut principal = json!(actor);
            for value in [&mut owner, &mut principal] {
                if let Some(map) = value.as_object_mut() {
                    map.remove("turn_id");
                }
            }
            require(owner == principal, "经理只能继续本会话发起的跨库操作")?;
            return match method {
                "configuration.transfer.apply" => shared.apply_configuration_transfer(self, &id),
                "configuration.transfer.cancel" => shared.cancel_configuration_transfer(&id),
                _ => Ok(
                    json!({"id":id,"status":transfer.data["status"],"project_id":controller.project_id,"result":transfer.data["result"]}),
                ),
            };
        }
        if !["configuration.change.preview", "configuration.change.apply"].contains(&method) {
            return self.manager_knowledge(method, &controller.project_id, p);
        }
        let change: ConfigurationChangeSet = serde_json::from_value(p["changeset"].clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        let selected = target.store.snapshot()?;
        self.manager_change_scope(
            if global { &selected } else { &snapshot },
            controller,
            &change,
            global,
        )?;
        if method.ends_with("preview") {
            return target.preview_configuration_change_as(&change, &actor);
        }
        if global {
            for project in self
                .manager_services
                .as_ref()
                .unwrap()
                .inventory
                .snapshots()?
            {
                check_configuration_retirements(
                    &change,
                    &project
                        .projects
                        .first()
                        .map(|p| p.title.clone())
                        .unwrap_or_default(),
                    &project,
                )?;
            }
        }
        // Persist authorization before a cross-store write. It is a precise in-flight
        // intent, not a new permission grant; replay observes the original action.
        self.store.transact(None,&mut|s|{
            self.manager_live(s,controller,call)?;
            if !s.governance.iter().any(|r|r.id==format!("{action_id}:authorization")) {
                let mut r=record("manager_write_authorization",call.id.clone(),Scope::project(&controller.project_id),"经理配置写入".into(),1,None,json!({"actor":actor,"method":method,"arguments_digest":fingerprint,"global":global}),true);r.id=format!("{action_id}:authorization");s.governance.push(r);
            }
            Ok(Value::Null)
        })?;
        let c = Command {
            command_id: action_id.clone(),
            target: None,
            expected_revision: None,
            payload: json!({"changeset":change,"preview_hash":p["preview_hash"]}),
        };
        // Use the same candidate service and preview fingerprint as the UI. The tool
        // action ledger additionally binds the exact native call for crash replay.
        target
            .store
            .transact(Some((&action_id, &fingerprint)), &mut |s| {
                let result = target
                    .configuration_change_command("configuration.change.apply", &c, s, &actor)?
                    .ok_or_else(|| err("method_not_found", "配置用例未处理"))?;
                InvocationContext::workflow(actor.clone(), Some(controller.project_id.clone()))
                    .record(s, method, &c)?;
                Ok(result)
            })
    }
}
