//! Managed engineering operations. Handler identifiers are protocols, not business actions.
use crate::governance::{record, update_record};
use crate::*;

pub(crate) const PLATFORM_HANDLERS: &[&str] = &[
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
    "workspace.current",
    "workspace.snapshot",
    "tasks.create",
    "tasks.complete",
    "materials.capture",
    "tests.run",
    "git.candidate",
    "git.advance",
    "git.publish",
];
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformToolRequest {
    pub execution_id: Id,
    pub instance_id: Id,
    pub owner_id: Id,
    pub project_id: Id,
    pub definition: ToolDefinition,
    pub arguments: Value,
    pub runtime: Option<ExecutionInstance>,
    pub permissions: Option<VersionedPermissionLock>,
    #[serde(default)]
    pub caller: Option<AgentPlatformCaller>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPlatformCaller {
    pub execution_id: Id,
    pub session_id: Id,
    pub turn_id: Id,
    pub operation_id: Id,
    pub request_id: Value,
    pub call_id: String,
    pub tool_ref: Id,
}
impl PlatformToolRequest {
    fn step_execution_id(&self) -> &str {
        self.caller
            .as_ref()
            .map_or(&self.execution_id, |caller| &caller.execution_id)
    }
    fn caller_active(&self, state: &EngineeringState) -> bool {
        self.caller.as_ref().is_none_or(|caller| {
            state.turns.iter().any(|turn| {
                turn.id == caller.turn_id
                    && turn.session_id == caller.session_id
                    && turn.status == TurnStatus::Running
            }) && state.executions.iter().any(|execution| {
                execution.session_id == caller.session_id
                    && execution.operation_id == caller.operation_id
                    && !execution.writer_revoked
            })
        })
    }
}
impl Engine {
    pub(crate) fn project_agent_platform_call(
        &self,
        state: &mut EngineeringState,
        session: &Session,
        tid: &str,
        observation: &ports::ConversationObservation,
        definition: ConfigurationRecord,
    ) -> Result<()> {
        if crate::controllers::binding(state, &session.id).is_some() {
            let checkpoint = state.clone();
            let result =
                self.project_controller_tool(state, session, tid, observation, definition.clone());
            if let Err(error) = result {
                *state = checkpoint;
                let ports::ConversationObservation::ToolCall {
                    request,
                    call,
                    arguments,
                    ..
                } = observation
                else {
                    return Err(error);
                };
                let bound = crate::controllers::binding(state, &session.id)
                    .unwrap()
                    .clone();
                let worker = state
                    .executions
                    .iter()
                    .find(|e| e.session_id == session.id && !e.writer_revoked)
                    .ok_or_else(|| err("tool_not_authorized", "编排执行者已停止"))?;
                let identity = format!(
                    "controller-call-{}",
                    configuration_hash(&(
                        session.id.as_str(),
                        tid,
                        worker.operation_id.as_str(),
                        call.as_str()
                    ))
                );
                if state.configuration(&identity).is_ok() {
                    return Err(error);
                }
                let mut row = record(
                    "controller_tool_call",
                    text(&bound.data, "controller_id")?,
                    bound.scope,
                    "编排调用拒绝".into(),
                    1,
                    None,
                    json!({"status":"failed","method":definition.data["qualified_name"],"arguments":arguments,"session_id":session.id,"turn_id":tid,"worker_operation_id":worker.operation_id,"request_id":request,"call_id":call,"tool_ref":definition.id,"epoch":bound.data["epoch"],"error":error}),
                    false,
                );
                row.id = identity;
                state.governance.push(row);
            }
            return Ok(());
        }
        let ports::ConversationObservation::ToolCall {
            request,
            call,
            arguments,
            ..
        } = observation
        else {
            return Ok(());
        };
        let binding = crate::agent_tasks::session_binding(state, &session.id)
            .ok_or_else(|| err("tool_not_authorized", "平台工具需要当前 Workflow 执行关联"))?
            .clone();
        let row = state
            .configuration(binding.data["instance_id"].as_str().unwrap_or(""))?
            .clone();
        let instance: ConfiguredWorkflowInstance = row.decode()?;
        let execution = instance
            .steps
            .iter()
            .find(|execution| Some(execution.id.as_str()) == binding.data["execution_id"].as_str())
            .ok_or_else(|| err("tool_not_authorized", "当前步骤不存在"))?;
        require(
            execution.status == StepExecutionStatus::Running
                && execution.session_id.as_deref() == Some(&session.id)
                && !instance.cancel_requested
                && !state.workflow_instance_suspended(&row.id),
            "当前步骤已停止或失效",
        )?;
        let step = instance
            .plan
            .document
            .steps
            .iter()
            .find(|step| step.key == execution.step)
            .unwrap();
        require(
            step.access.tool_refs.contains(&definition.id),
            "平台工具不属于当前 Step 授权",
        )?;
        let operation = state
            .executions
            .iter()
            .find(|execution| execution.session_id == session.id && !execution.writer_revoked)
            .ok_or_else(|| err("tool_not_authorized", "调用 worker 已停止"))?
            .operation_id
            .clone();
        let invocation = format!(
            "platform-call-{}",
            configuration_hash(&json!([session.id, tid, operation, call]))
        );
        if let Some(old) = state.governance.iter().find(|record| {
            record.kind == "agent_platform_request" && record.definition_id == invocation
        }) {
            require(
                old.data["request"]["arguments"] == *arguments,
                "原生调用 ID 被不同参数复用",
            )?;
            return Ok(());
        }
        let tool: ToolDefinition = definition.decode()?;
        let caller = AgentPlatformCaller {
            execution_id: execution.id.clone(),
            session_id: session.id.clone(),
            turn_id: tid.into(),
            operation_id: operation,
            request_id: request.clone(),
            call_id: call.clone(),
            tool_ref: definition.id.clone(),
        };
        let platform = PlatformToolRequest {
            execution_id: invocation.clone(),
            instance_id: row.id.clone(),
            owner_id: instance.owner_id.clone(),
            project_id: instance.project_id.clone(),
            definition: tool,
            arguments: arguments.clone(),
            runtime: None,
            permissions: instance.plan.permission_locks.get(&definition.id).cloned(),
            caller: Some(caller),
        };
        let authorization = (|| {
            self.tool_executor
                .as_ref()
                .ok_or_else(|| err("unsupported", "缺少结构校验器"))?
                .validate_arguments(&platform.definition.input_schema, arguments)?;
            let turn = state
                .turns
                .iter()
                .find(|turn| turn.id == tid)
                .ok_or_else(|| err("not_found", "Turn 不存在"))?;
            let runtime_binding = state.turn_runtime_binding(turn)?;
            let permissions = runtime_binding
                .permissions
                .as_ref()
                .ok_or_else(|| err("permission_unconfigured", "平台调用缺少实际权限"))?;
            for policy in [&permissions.harness.policy, &permissions.sandbox.policy] {
                require(
                    match &policy.tools {
                        ToolPermission::AllAvailable => true,
                        ToolPermission::Restricted { allow, deny } => {
                            !deny.contains(&definition.id)
                                && (allow.is_empty() || allow.contains(&definition.id))
                        }
                    },
                    "Permission 未允许该平台工具",
                )?;
                if ![
                    "workspace.current",
                    "materials.capture",
                    "tests.run",
                    "workflow.outputs.read",
                ]
                .contains(&platform.definition.qualified_name.as_str())
                {
                    require(
                        policy.filesystem != FilesystemPermission::ReadOnly,
                        "只读权限不能执行平台写操作",
                    )?;
                    require(
                        permissions.approval == ApprovalPolicy::Never,
                        "需要批准的平台写操作请使用显式 Tool Step 与确认门禁",
                    )?;
                }
                if platform.definition.qualified_name == "git.publish" {
                    require(
                        policy.network == NetworkPermission::Enabled,
                        "Permission 禁止远端发布",
                    )?;
                }
            }
            Ok::<(), Error>(())
        })();
        let source = record(
            "agent_platform_request",
            invocation.clone(),
            row.scope.clone(),
            definition.name,
            1,
            None,
            json!({"request":platform}),
            true,
        );
        state.governance.push(source);
        if let Err(error) = authorization {
            let mut result = record(
                "platform_call",
                invocation.clone(),
                row.scope.clone(),
                platform.definition.qualified_name.clone(),
                1,
                None,
                json!({"status":"failed","request":platform,"error":error,"remote_effect":"not_applied"}),
                false,
            );
            result.id = invocation;
            state.governance.push(result);
            return Ok(());
        }
        state.governance.push(record("workflow_intent",execution.id.clone(),row.scope,"workflow.tool.execute".into(),1,None,json!({"status":"prepared","configured":true,"method":"workflow.tool.execute","command":Command{command_id:invocation.clone(),target:Some(instance.owner_id),expected_revision:None,payload:json!({"execution_id":execution.id,"agent_platform_call_id":invocation})}}),false));
        Ok(())
    }
    pub fn configured_platform_request(
        &self,
        command: &Command,
    ) -> Result<Option<PlatformToolRequest>> {
        let state = self.store.snapshot()?;
        let execution_id = text(&command.payload, "execution_id")?;
        require(
            state.governance.iter().any(|r| {
                r.kind == "workflow_intent"
                    && r.definition_id == execution_id
                    && r.data["method"] == "workflow.tool.execute"
                    && r.data["command"] == json!(command)
            }),
            "工具缺少确切执行意图",
        )?;
        if let Some(invocation) = command.payload["agent_platform_call_id"].as_str() {
            let source = state
                .governance
                .iter()
                .find(|record| {
                    record.kind == "agent_platform_request" && record.definition_id == invocation
                })
                .ok_or_else(|| err("not_found", "平台调用来源不存在"))?;
            let request: PlatformToolRequest =
                serde_json::from_value(source.data["request"].clone())
                    .map_err(|e| err("storage", e.to_string()))?;
            let instance = state.configuration(&request.instance_id)?;
            require(
                request.execution_id == invocation
                    && request.step_execution_id() == execution_id
                    && request.caller_active(&state)
                    && !state.workflow_instance_suspended(&instance.id)
                    && instance.data["cancel_requested"] != true
                    && instance.data["steps"].as_array().is_some_and(|steps| {
                        steps
                            .iter()
                            .any(|step| step["id"] == execution_id && step["status"] == "running")
                    }),
                "平台调用的执行者已停止或失效",
            )?;
            return Ok(Some(request));
        }
        let row = state
            .governance
            .iter()
            .find(|r| {
                r.kind == "workflow_instance"
                    && r.data["steps"]
                        .as_array()
                        .is_some_and(|s| s.iter().any(|s| s["id"] == execution_id))
            })
            .ok_or_else(|| err("not_found", "工具执行不存在"))?;
        let instance: ConfiguredWorkflowInstance = row.decode()?;
        let execution = instance
            .steps
            .iter()
            .find(|e| e.id == execution_id)
            .unwrap();
        require(
            !state.workflow_instance_suspended(&row.id)
                && !instance.cancel_requested
                && execution.status == StepExecutionStatus::Running,
            "工具执行已暂停或失效",
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
            return Ok(None);
        };
        let definition = instance
            .plan
            .definitions
            .get(tool_ref)
            .ok_or_else(|| err("configuration_required", "工具未冻结"))?;
        if definition.data["implementation"] != "platform" {
            return Ok(None);
        }
        let definition: ToolDefinition = definition.decode()?;
        require(
            PLATFORM_HANDLERS.contains(&definition.qualified_name.as_str()),
            "平台工具处理器不可用",
        )?;
        let facts = crate::configured_runtime::facts(&state, &instance)?;
        let arguments: serde_json::Map<String, Value> = arguments
            .iter()
            .map(|(name, binding)| {
                Ok((
                    name.clone(),
                    match binding {
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
                    },
                ))
            })
            .collect::<Result<_>>()?;
        self.tool_executor
            .as_ref()
            .ok_or_else(|| err("unsupported", "缺少结构校验器"))?
            .validate_arguments(&definition.input_schema, &Value::Object(arguments.clone()))?;
        Ok(Some(PlatformToolRequest {
            execution_id,
            instance_id: row.id.clone(),
            owner_id: instance.owner_id,
            project_id: instance.project_id,
            definition,
            arguments: Value::Object(arguments),
            runtime: None,
            permissions: instance.plan.permission_locks.get(tool_ref).cloned(),
            caller: None,
        }))
    }
    pub fn enqueue_platform_tool(
        &self,
        command: &Command,
        request: &PlatformToolRequest,
    ) -> Result<Value> {
        let fingerprint = command_fingerprint("workflow.tool.execute", command);
        let frozen = self
            .configured_platform_request(command)?
            .ok_or_else(|| err("invalid_params", "不是平台工具调用"))?;
        require(
            frozen.execution_id == request.execution_id
                && frozen.instance_id == request.instance_id
                && frozen.owner_id == request.owner_id
                && frozen.project_id == request.project_id
                && json!(frozen.definition) == json!(request.definition)
                && frozen.arguments == request.arguments
                && frozen.permissions == request.permissions
                && json!(frozen.caller) == json!(request.caller),
            "平台请求改变了冻结的工作流意图",
        )?;
        self.store.transact(Some((&command.command_id,&fingerprint)),&mut |state|{
            require(state.configuration(&request.instance_id)?.data["steps"].as_array().is_some_and(|s|s.iter().any(|e|e["id"]==request.step_execution_id()&&e["status"]=="running")),"执行已失效")?;
            if request.definition.qualified_name=="git.advance" {
                let permit=self.workflow_action_permit(state,request)?;
                state.governance.push(record("workflow_action_permit",request.execution_id.clone(),Scope::Project{project_id:request.project_id.clone()},"候选接纳许可".into(),1,None,json!(permit),true));
            }
            let mut row=record("platform_call",request.execution_id.clone(),Scope::Project{project_id:request.project_id.clone()},request.definition.qualified_name.clone(),1,None,
                json!({"status":"prepared","request":request,"operations":{"base":id(),"candidate":id(),"remote":id()}}),false);row.id=request.execution_id.clone();let result=json!({"id":row.id});state.governance.push(row);Ok(result)
        })
    }
    fn platform_context(
        &self,
        state: &EngineeringState,
        request: &PlatformToolRequest,
        owner: Option<&str>,
    ) -> Result<GitContext> {
        let owner = owner.unwrap_or(&request.owner_id);
        let owner = if owner == "$project" {
            request.project_id.as_str()
        } else {
            owner
        };
        let within = owner == request.owner_id
            || state
                .tasks
                .iter()
                .any(|t| t.id == owner && t.feature_id == request.owner_id)
            || owner == request.project_id;
        require(within, "工作空间不属于当前执行范围")?;
        let project = state
            .projects
            .iter()
            .find(|p| p.id == request.project_id)
            .ok_or_else(|| err("not_found", "Project 不存在"))?;
        let source = state
            .repository_worktrees
            .iter()
            .find(|w| w.project_id == request.project_id && w.owner_id == owner)
            .ok_or_else(|| err("resource_pending", "受管理工作空间尚未就绪"))?;
        let target_owner = state
            .tasks
            .iter()
            .find(|t| t.id == owner)
            .map(|t| t.feature_id.as_str())
            .unwrap_or(&request.project_id);
        let target = state
            .repository_worktrees
            .iter()
            .find(|w| w.project_id == request.project_id && w.owner_id == target_owner)
            .ok_or_else(|| err("resource_pending", "主干工作空间尚未就绪"))?;
        Ok(GitContext {
            project: request.project_id.clone(),
            workspace: project
                .workspace_definition
                .clone()
                .ok_or_else(|| err("configuration_required", "项目未绑定仓库"))?,
            source: CodeLocation {
                owner: owner.into(),
                branch: source.branch.clone(),
                worktree: source.path.clone(),
            },
            target: CodeLocation {
                owner: target_owner.into(),
                branch: target.branch.clone(),
                worktree: target.path.clone(),
            },
            origin: Origin::External,
        })
    }
    fn platform_grant(
        &self,
        state: &EngineeringState,
        request: &PlatformToolRequest,
    ) -> Result<GitContext> {
        let grant = &request.arguments["workspace"];
        let workspace = text(grant, "workspace_id")?;
        let worktree = state
            .repository_worktrees
            .iter()
            .find(|w| w.id == workspace && w.project_id == request.project_id)
            .ok_or_else(|| err("scope_violation", "工作空间不属于当前项目"))?;
        let context = self.platform_context(state, request, Some(&worktree.owner_id))?;
        // A grant must have been emitted by a real platform operation in this root execution.
        let instance: ConfiguredWorkflowInstance =
            state.configuration(&request.instance_id)?.decode()?;
        require(
            state
                .governance
                .iter()
                .filter(|r| {
                    r.kind == "platform_call"
                        && r.data["status"] == "succeeded"
                        && r.data["receipt"]["workspace_id"] == workspace
                        && r.data["receipt"]["revision"] == grant["revision"]
                })
                .any(|r| {
                    r.data["request"]["instance_id"]
                        .as_str()
                        .and_then(|id| state.configuration(id).ok())
                        .is_some_and(|r| r.data["root_instance_id"] == instance.root_instance_id)
                }),
            "工作空间引用缺少当前流程的真实来源",
        )?;
        Ok(context)
    }
    fn platform_test_request(
        &self,
        state: &EngineeringState,
        request: &PlatformToolRequest,
    ) -> Result<TestRequest> {
        let p = &request.arguments;
        let context = self.platform_grant(state, request)?;
        let runtime = request
            .runtime
            .clone()
            .ok_or_else(|| err("configuration_required", "测试工具缺少冻结执行环境"))?;
        let mut value = p["recipe"].clone();
        value["image"] = json!(
            runtime
                .sandbox
                .image
                .clone()
                .ok_or_else(|| err("configuration_required", "测试镜像未冻结"))?
        );
        let recipe: TestRecipe =
            serde_json::from_value(value).map_err(|e| err("invalid_params", e.to_string()))?;
        recipe.validate()?;
        Ok(TestRequest {
            operation_id: request.execution_id.clone(),
            session_id: None,
            candidate: text(&p["workspace"], "revision")?,
            environment: runtime.id.clone(),
            origin: Origin::External,
            code: Some(context),
            runtime: Some(runtime),
            recipe: Some(recipe),
            expect_assertion_failure: p["expect_assertion_failure"] == true,
        })
    }
    fn poll_platform_tool(
        &self,
        state: &EngineeringState,
        request: &PlatformToolRequest,
    ) -> Result<Option<Value>> {
        let p = &request.arguments;
        match request.definition.qualified_name.as_str() {
            "workflow.outputs.read" => {
                let instance: ConfiguredWorkflowInstance =
                    state.configuration(&request.instance_id)?.decode()?;
                let execution = instance
                    .steps
                    .iter()
                    .find(|e| e.id == request.step_execution_id())
                    .ok_or_else(|| err("not_found", "读取产物的步骤不存在"))?;
                let step = instance
                    .plan
                    .document
                    .steps
                    .iter()
                    .find(|s| s.key == execution.step)
                    .unwrap();
                let pointer = p["pointer"].as_str().unwrap_or("");
                let ids: Vec<String> = serde_json::from_value(p["ids"].clone())
                    .map_err(|e| err("invalid_params", e.to_string()))?;
                require(
                    !ids.is_empty()
                        && ids.iter().all(|id| {
                            step.inputs.values().any(|binding| match binding {
                                InputBinding::Source { source } if source.step != "$input" => {
                                    source.pointer == pointer
                                        && instance
                                            .steps
                                            .iter()
                                            .find(|e| e.step == source.step)
                                            .and_then(|e| e.outputs.get(&source.port))
                                            == Some(id)
                                }
                                InputBinding::Material { material_ref } => {
                                    pointer.is_empty() && material_ref == id
                                }
                                _ => false,
                            })
                        }),
                    "只能读取当前步骤显式输入的产物及字段",
                )?;
                let mut query = p.clone();
                query["id"] = json!(request.instance_id);
                query["instance_id"] = json!(request.instance_id);
                query["project_id"] = json!(request.project_id);
                self.workflow_control_query("workflow.outputs.read", &query)
                    .unwrap()
                    .map(Some)
            }
            "workspace.current" => {
                let context = match self.platform_context(state, request, p["owner_id"].as_str()) {
                    Err(error) if error.code == "resource_pending" => return Ok(None),
                    result => result?,
                };
                let revision = self.git.workspace_revision(&context)?;
                let worktree = state
                    .repository_worktrees
                    .iter()
                    .find(|w| w.path == context.source.worktree)
                    .unwrap();
                Ok(Some(
                    json!({"workspace_id":worktree.id,"revision":revision,"owner_id":worktree.owner_id}),
                ))
            }
            "workspace.snapshot" => {
                let context = self.platform_grant(state, request)?;
                require(
                    !state.executions.iter().any(|e| {
                        !e.writer_revoked
                            && state.turns.iter().any(|t| {
                                t.session_id == e.session_id
                                    && t.code_view.as_ref().is_some_and(|v| {
                                        v.context.source.worktree == context.source.worktree
                                    })
                            })
                    }),
                    "工作空间执行者尚未停止",
                )?;
                let allowed_paths: Vec<String> = serde_json::from_value(p["allowed_paths"].clone())
                    .map_err(|e| err("invalid_params", e.to_string()))?;
                let protected_paths: Vec<String> =
                    serde_json::from_value(p.get("protected_paths").cloned().unwrap_or(json!([])))
                        .map_err(|e| err("invalid_params", e.to_string()))?;
                let operation = RepositoryOperation {
                    id: request.execution_id.clone(),
                    project_id: request.project_id.clone(),
                    action: RepositoryAction::Snapshot {
                        run: request.execution_id.clone(),
                        context: context.clone(),
                        expected: text(&p["workspace"], "revision")?,
                        allowed_paths,
                        protected_paths,
                    },
                    status: RepositoryOperationStatus::Dispatched,
                    receipt: None,
                    reason: String::new(),
                    revision: 1,
                };
                let RepositoryReceipt::Snapshot { revision } =
                    self.git.repository_operation(&operation)?
                else {
                    return Err(err("adapter_contract", "工作空间快照回执类型不符"));
                };
                let mut receipt = json!({"workspace_id":p["workspace"]["workspace_id"],"owner_id":context.source.owner,"revision":revision});
                if p["include_diff"] == true {
                    let base = text(&p["workspace"], "revision")?;
                    receipt["changes"] = json!({"base_revision":base,"revision":revision,"diff":self.git.diff(&context,&base,&revision)?});
                }
                if let Some(base) = p.get("diff_base") {
                    require(
                        base["workspace_id"] == p["workspace"]["workspace_id"],
                        "范围 Diff 必须使用同一工作区的真实快照",
                    )?;
                    let mut baseline_request = request.clone();
                    baseline_request.arguments["workspace"] = base.clone();
                    self.platform_grant(state, &baseline_request)?;
                    let base = text(base, "revision")?;
                    receipt["scope_changes"] = json!({"base_revision":base,"revision":revision,"diff":self.git.diff(&context,&base,&revision)?});
                }
                Ok(Some(receipt))
            }
            "materials.capture" => Ok(Some(
                json!({"inputs":p,"captured_at":now(),"operation_id":request.execution_id}),
            )),
            "tasks.create" => {
                let feature = state.feature(&request.owner_id)?;
                require(feature.project_id == request.project_id, "Task 范围错误")?;
                let items = p["tasks"]
                    .as_array()
                    .ok_or_else(|| err("invalid_params", "tasks 应为数组"))?;
                require(
                    !items.is_empty() && items.len() <= WORKFLOW_STEP_LIMIT,
                    "Task 集合不能为空或超过平台上限",
                )?;
                let nodes: Vec<_> = items
                    .iter()
                    .map(|v| {
                        Ok((
                            text(v, "key")?,
                            serde_json::from_value::<Vec<String>>(
                                v.get("dependencies").cloned().unwrap_or(json!([])),
                            )
                            .map_err(|e| err("invalid_params", e.to_string()))?,
                        ))
                    })
                    .collect::<Result<_>>()?;
                let order = topological(&nodes)?;
                for v in items {
                    text(v, "title")?;
                }
                // Mutation is committed with the receipt by drive_platform_tools below.
                Ok(Some(
                    json!({"tasks":order.iter().map(|key|items.iter().find(|v|v["key"]==*key).unwrap()).collect::<Vec<_>>()}),
                ))
            }
            "tasks.complete" => {
                let task_id = text(p, "task_id")?;
                let task = state
                    .tasks
                    .iter()
                    .find(|task| task.id == task_id)
                    .ok_or_else(|| err("not_found", "Task 不存在"))?;
                require(
                    task.id == request.owner_id || task.feature_id == request.owner_id,
                    "Task 不属于当前执行范围",
                )?;
                require(
                    task.dependencies.iter().all(|id| {
                        state
                            .tasks
                            .iter()
                            .any(|task| task.id == *id && task.status == Status::Completed)
                    }),
                    "Task 依赖尚未完成",
                )?;
                require(
                    !state.executions.iter().any(|execution| {
                        !execution.writer_revoked
                            && state.turns.iter().any(|turn| {
                                turn.session_id == execution.session_id
                                    && turn
                                        .code_view
                                        .as_ref()
                                        .is_some_and(|view| view.context.source.owner == task_id)
                            })
                    }),
                    "Task 执行者尚未停止",
                )?;
                Ok(Some(
                    json!({"task_id":task_id,"status":"completed","operation_id":request.execution_id}),
                ))
            }
            "tests.run" => {
                let request = self.platform_test_request(state, request)?;
                let output = self.tests.poll_test(&request)?;
                if let Some(output) = &output {
                    let receipt: TestResult = serde_json::from_value(output.clone())
                        .map_err(|e| err("adapter_contract", e.to_string()))?;
                    require(
                        receipt.operation_id == request.operation_id
                            && receipt.candidate == request.candidate
                            && receipt.environment == request.environment
                            && receipt.origin == Origin::External,
                        "测试回执归属不符",
                    )?;
                }
                Ok(output)
            }
            "git.candidate" => {
                let context = self.platform_grant(state, request)?;
                let source = text(&p["workspace"], "revision")?;
                let target = text(p, "target_revision")?;
                self.git.preflight_advance(&context, &target)?;
                let revision =
                    self.git
                        .candidate(&context, &source, &target, &request.execution_id)?;
                let parents = self.git.commit_parents(&context, &revision)?;
                require(
                    parents == vec![target.clone()],
                    "候选实际父节点与冻结目标不符",
                )?;
                let diff = self.git.diff(&context, &target, &revision)?;
                Ok(Some(
                    json!({"revision":revision,"source_revision":source,"target_revision":target,"parents":parents,"diff":diff,"workspace_id":p["workspace"]["workspace_id"],"owner_id":context.source.owner,"operation_id":request.execution_id}),
                ))
            }
            "git.advance" => {
                let context = self.platform_grant(state, request)?;
                let candidate = &p["candidate"];
                let revision = text(candidate, "revision")?;
                let expected = text(candidate, "target_revision")?;
                let operation = text(candidate, "operation_id")?;
                require(
                    state.configuration(&operation).is_ok_and(|r| {
                        r.kind == "platform_call"
                            && r.data["request"]["instance_id"] == request.instance_id
                            && r.data["status"] == "succeeded"
                            && r.data["request"]["definition"]["qualified_name"] == "git.candidate"
                            && r.data["receipt"] == *candidate
                    }),
                    "候选缺少真实平台回执",
                )?;
                let permit = self.workflow_action_permit(state, request)?;
                require(
                    state.governance.iter().any(|r| {
                        r.kind == "workflow_action_permit"
                            && r.definition_id == request.execution_id
                            && r.data == json!(permit)
                    }),
                    "操作缺少当前计划的确切许可",
                )?;
                let actual = self.git.workspace_revision(&GitContext {
                    source: context.target.clone(),
                    ..context.clone()
                })?;
                self.git.preflight_advance(&context, &expected)?;
                let observed = self.git.advance(
                    &context,
                    &actual,
                    &expected,
                    &revision,
                    &request.execution_id,
                )?;
                require(observed == revision, "Git 推进未观察到确切候选")?;
                Ok(Some(
                    json!({"revision":observed,"previous":expected,"operation_id":request.execution_id}),
                ))
            }
            "git.publish" => {
                let context = self.platform_grant(state, request)?;
                let candidate = &p["candidate"];
                let revision = text(candidate, "revision")?;
                let expected = text(candidate, "target_revision")?;
                let accepted = &p["accepted"];
                require(
                    accepted["revision"] == revision
                        && accepted["previous"] == expected
                        && state.governance.iter().any(|r| {
                            r.kind == "platform_call"
                                && r.data["request"]["instance_id"] == request.instance_id
                                && r.data["status"] == "succeeded"
                                && r.data["request"]["definition"]["qualified_name"]
                                    == "git.advance"
                                && r.data["receipt"] == *accepted
                        }),
                    "发布需要当前确切候选的真实本地接纳回执",
                )?;
                let operation = state.configuration(&request.execution_id)?;
                let candidate_branch = format!("devdesk/candidates/{}", request.execution_id);
                for (key, branch, revision, expected, stage) in [
                    (
                        "base",
                        context.target.branch.clone(),
                        expected.clone(),
                        None,
                        PublishStage::TargetBase,
                    ),
                    (
                        "candidate",
                        candidate_branch.clone(),
                        revision.clone(),
                        None,
                        PublishStage::Candidate,
                    ),
                ] {
                    self.ensure_platform_repository_operation(
                        request,
                        key,
                        &RepositoryOperation {
                            id: text(&operation.data["operations"], key)?,
                            project_id: request.project_id.clone(),
                            action: RepositoryAction::PublishRef {
                                merge: request.execution_id.clone(),
                                context: context.clone(),
                                branch,
                                revision,
                                expected,
                                stage,
                            },
                            status: RepositoryOperationStatus::Dispatched,
                            receipt: None,
                            reason: String::new(),
                            revision: 1,
                        },
                    )?;
                }
                let adapter = self
                    .pull_requests
                    .as_ref()
                    .ok_or_else(|| err("unsupported", "未配置 GitHub PR 适配器"))?;
                let pr_request = PullRequestRequest {
                    repository: context.workspace.repository.clone(),
                    project: context.project.clone(),
                    merge: request.execution_id.clone(),
                    head_branch: candidate_branch,
                    base_branch: context.target.branch.clone(),
                    candidate: revision.clone(),
                    expected_target: expected.clone(),
                    number: None,
                };
                let pr = adapter.ensure(&pr_request)?;
                require(
                    pr.repository == pr_request.repository
                        && pr.marker == pr_request.marker()
                        && pr.head == revision
                        && pr.head_branch == pr_request.head_branch
                        && pr.base_branch == pr_request.base_branch,
                    "GitHub PR 回执归属不符",
                )?;
                self.ensure_platform_repository_operation(
                    request,
                    "remote",
                    &RepositoryOperation {
                        id: text(&operation.data["operations"], "remote")?,
                        project_id: request.project_id.clone(),
                        action: RepositoryAction::PublishRef {
                            merge: request.execution_id.clone(),
                            context: context.clone(),
                            branch: context.target.branch.clone(),
                            revision: revision.clone(),
                            expected: Some(expected),
                            stage: PublishStage::RemoteAdvance,
                        },
                        status: RepositoryOperationStatus::Dispatched,
                        receipt: None,
                        reason: String::new(),
                        revision: 1,
                    },
                )?;
                let pr = adapter.read(&PullRequestRequest {
                    number: Some(pr.number),
                    ..pr_request
                })?;
                if !pr.merged {
                    return Ok(None);
                }
                require(
                    pr.merge_commit.as_deref() == Some(revision.as_str()) && pr.head == revision,
                    "GitHub merged 结果与确切候选不符",
                )?;
                Ok(Some(
                    json!({"revision":revision,"pull_request":pr,"operation_id":request.execution_id}),
                ))
            }
            _ => Err(err("unsupported", "平台工具处理器不受支持")),
        }
    }
    fn ensure_platform_repository_operation(
        &self,
        request: &PlatformToolRequest,
        key: &str,
        operation: &RepositoryOperation,
    ) -> Result<RepositoryReceipt> {
        let state = self.store.snapshot()?;
        if let Some(record) = state
            .governance
            .iter()
            .find(|r| r.kind == "platform_repository_receipt" && r.definition_id == operation.id)
        {
            require(
                record.data["operation"] == json!(operation),
                "平台 Git 操作被不同请求复用",
            )?;
            return serde_json::from_value(record.data["receipt"].clone())
                .map_err(|e| err("adapter_contract", e.to_string()));
        }
        let receipt = self.git.repository_operation(operation)?;
        require(
            matches!(&receipt,RepositoryReceipt::PublishedRef{revision,branch} if matches!(&operation.action,RepositoryAction::PublishRef{revision:expected,branch:expected_branch,..} if expected==revision&&expected_branch==branch)),
            "发布 ref 回执与冻结请求不符",
        )?;
        self.store.transact(None, &mut |state| {
            state.governance.push(record(
                "platform_repository_receipt",
                operation.id.clone(),
                Scope::Project {
                    project_id: request.project_id.clone(),
                },
                key.into(),
                1,
                None,
                json!({"operation":operation,"receipt":receipt}),
                true,
            ));
            Ok(Value::Null)
        })?;
        Ok(receipt)
    }
    pub fn drive_platform_tools(&self) -> Result<()> {
        let snapshot = self.store.snapshot()?;
        for row in snapshot.governance.iter().filter(|r| {
            r.kind == "platform_call"
                && ["prepared", "running"].contains(&r.data["status"].as_str().unwrap_or(""))
        }) {
            let request: PlatformToolRequest = serde_json::from_value(row.data["request"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            let instance = snapshot.configuration(&request.instance_id)?;
            if snapshot.workflow_instance_suspended(&request.instance_id)
                && row.data["status"] == "prepared"
                && row.data["cancel_requested"] != true
                && instance.data["cancel_requested"] != true
            {
                continue;
            }
            let cancelled = !request.caller_active(&snapshot)
                || instance.data["cancel_requested"] == true
                || row.data["cancel_requested"] == true
                || !instance.data["steps"].as_array().is_some_and(|steps| {
                    steps.iter().any(|step| {
                        step["id"] == request.step_execution_id() && step["status"] == "running"
                    })
                });
            let result = if cancelled {
                if request.definition.qualified_name == "tests.run" {
                    let test = self.platform_test_request(&snapshot, &request)?;
                    if !self.tests.stop_test(&test)? {
                        continue;
                    }
                    self.tests.cleanup_test(&test)?;
                }
                Err(err(
                    "cancelled",
                    "平台工具已停止；取消不会撤销已完成的外部副作用",
                ))
            } else {
                self.poll_platform_tool(&snapshot, &request)
            };
            let persisted = self.store.transact(None, &mut |state| {
                let mut data = row.data.clone();
                match &result {
                    Ok(Some(receipt)) => {
                        let mut receipt = receipt.clone();
                        if request.definition.qualified_name == "tasks.create" {
                            let tasks = receipt["tasks"].as_array().unwrap();
                            let ids: std::collections::BTreeMap<_, _> = tasks
                                .iter()
                                .map(|v| (v["key"].as_str().unwrap().to_owned(), id()))
                                .collect();
                            let feature = state.feature(&request.owner_id)?.clone();
                            let mut members = vec![];
                            for item in tasks {
                                let tid = ids[item["key"].as_str().unwrap()].clone();
                                let dependencies = item["dependencies"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .map(|v| ids[v.as_str().unwrap()].clone())
                                    .collect();
                                state.tasks.push(Task {
                                    id: tid.clone(),
                                    feature_id: feature.id.clone(),
                                    title: item["title"].as_str().unwrap().into(),
                                    status: Status::Draft,
                                    revision: 1,

                                    inherited_snapshot: None,
                                    dependencies,
                                    worktree: String::new(),
                                    scope: TaskScope::default(),
                                });
                                state.contexts.push(Context {
                                    id: id(),
                                    owner_id: tid.clone(),
                                    content: item.to_string(),
                                    revision: 1,
                                });
                                let mut member = item.clone();
                                member["id"] = json!(tid);
                                member["feature_id"] = json!(feature.id);
                                member["dependency_keys"] =
                                    item.get("dependencies").cloned().unwrap_or(json!([]));
                                member["dependencies"] =
                                    json!(state.tasks.last().unwrap().dependencies);
                                members.push(member);
                            }
                            receipt = json!({"tasks":members});
                        }
                        if request.definition.qualified_name == "workspace.snapshot"
                            || request.definition.qualified_name == "git.advance"
                        {
                            let context = self.platform_grant(state, &request)?;
                            let owner = if request.definition.qualified_name == "git.advance" {
                                context.target.owner
                            } else {
                                context.source.owner
                            };
                            let head_id = if owner == request.project_id {
                                format!("main:{owner}")
                            } else {
                                owner
                            };
                            let head = state
                                .heads
                                .iter_mut()
                                .find(|h| h.id == head_id)
                                .ok_or_else(|| err("configuration_required", "代码头未登记"))?;
                            head.revision = text(&receipt, "revision")?;
                        }
                        if request.definition.qualified_name == "tasks.complete" {
                            if let Some(task) = state
                                .tasks
                                .iter_mut()
                                .find(|task| receipt["task_id"] == task.id)
                            {
                                if task.status != Status::Completed {
                                    task.status = Status::Completed;
                                    task.revision += 1;
                                }
                            }
                        }
                        self.tool_executor
                            .as_ref()
                            .ok_or_else(|| err("unsupported", "缺少结构校验器"))?
                            .validate_arguments(&request.definition.output_schema, &receipt)?;
                        data["status"] = json!("succeeded");
                        data["receipt"] = receipt;
                        data["finished"] = json!(now());
                    }
                    Ok(None) => {
                        data["status"] = json!("running");
                    }
                    Err(error) if error.code == "resource_pending" => {}
                    Err(error) => {
                        data["status"] = json!(if error.code == "cancelled" {
                            "cancelled"
                        } else if error.code == "needs_reconciliation" {
                            "needs_reconciliation"
                        } else {
                            "failed"
                        });
                        data["error"] = json!(error);
                    }
                }
                update_record(
                    state
                        .governance
                        .iter_mut()
                        .find(|r| r.id == row.id)
                        .unwrap(),
                    data,
                );
                Ok(Value::Null)
            });
            if let Err(error) = persisted {
                self.store.transact(None, &mut |state| {
                    let call = state
                        .governance
                        .iter_mut()
                        .find(|record| record.id == row.id)
                        .unwrap();
                    let mut data = call.data.clone();
                    data["status"] = json!("failed");
                    data["error"] = json!({"code":"receipt_rejected","message":error.message});
                    if let Ok(Some(observed)) = &result {
                        data["observed_receipt"] = observed.clone();
                    }
                    data["remote_effect"] = json!("unknown");
                    data["finished"] = json!(now());
                    update_record(call, data);
                    Ok(Value::Null)
                })?;
            }
        }
        self.reply_agent_platform_calls()
    }

    fn reply_agent_platform_calls(&self) -> Result<()> {
        let snapshot = self.store.snapshot()?;
        for source in snapshot
            .governance
            .iter()
            .filter(|record| record.kind == "agent_platform_request")
        {
            let request: PlatformToolRequest =
                serde_json::from_value(source.data["request"].clone())
                    .map_err(|error| err("storage", error.to_string()))?;
            let Some(caller) = &request.caller else {
                continue;
            };
            let result = snapshot
                .governance
                .iter()
                .find(|record| record.id == request.execution_id && record.kind == "platform_call");
            let failed = snapshot.governance.iter().find(|record| {
                record.kind == "workflow_intent"
                    && record.data["command"]["payload"]["agent_platform_call_id"]
                        == request.execution_id
                    && record.data["status"] == "failed"
            });
            if result.is_none() && failed.is_some() {
                self.store.transact(None,&mut|state|{
                    let mut result=record("platform_call",request.execution_id.clone(),source.scope.clone(),request.definition.qualified_name.clone(),1,None,json!({"status":"failed","request":request,"remote_effect":"not_applied","error":{"code":"dispatch_failed","message":failed.unwrap().data["error"]}}),false);result.id=request.execution_id.clone();state.governance.push(result);Ok(Value::Null)
                })?;
                continue;
            }
            let Some(result) = result else { continue };
            if snapshot.governance.iter().any(|record| {
                record.kind == "platform_tool_reply" && record.definition_id == request.execution_id
            }) || !["succeeded", "failed", "cancelled", "needs_reconciliation"]
                .contains(&result.data["status"].as_str().unwrap_or(""))
            {
                continue;
            }
            let active = request.caller_active(&snapshot);
            let success = result.data["status"] == "succeeded";
            let receipt = ToolReceipt {
                invocation_id: request.execution_id.clone(),
                success,
                content: vec![
                    json!({"type":"text","text":if success{result.data["receipt"].to_string()}else{result.data["error"].to_string()}}),
                ],
                structured_content: success.then(|| result.data["receipt"].clone()),
                remote_effect: if success {
                    "applied"
                } else {
                    result.data["remote_effect"].as_str().unwrap_or("unknown")
                }
                .into(),
                error: (!success).then(|| result.data["error"].clone()),
                origin: Origin::External,
            };
            let session = snapshot
                .sessions
                .iter()
                .find(|session| session.id == caller.session_id)
                .ok_or_else(|| err("storage", "平台调用 Session 缺失"))?;
            if !active
                || self
                    .harness
                    .answer_tool_call(session, &caller.request_id, &receipt)
                    .is_ok()
            {
                self.store.transact(None, &mut |state| {
                    if !state.governance.iter().any(|record|record.kind=="platform_tool_reply"&&record.definition_id==request.execution_id){
                        state.governance.push(record("platform_tool_reply",request.execution_id.clone(),source.scope.clone(),"原生工具回执交付".into(),1,None,json!({"status":if active{"delivered"}else{"worker_unavailable"},"receipt":receipt}),true));
                    }
                    Ok(Value::Null)
                })?;
            }
        }
        Ok(())
    }
}

/// Common call-history projection; the stored platform request and receipt retain their own protocol.
pub(crate) fn project_platform_call(
    state: &EngineeringState,
    record: &ConfigurationRecord,
) -> ConfigurationRecord {
    let mut projected = record.clone();
    let request = &record.data["request"];
    let caller = &request["caller"];
    let session = caller["session_id"]
        .as_str()
        .and_then(|id| state.sessions.iter().find(|session| session.id == id));
    let agent_instance = session
        .and_then(|session| session.runtime_binding.as_ref())
        .and_then(|binding| binding.agent_configuration.as_ref())
        .map(|configuration| &configuration.agent_instance_id);
    let definition = caller["tool_ref"].as_str().or_else(|| {
        state
            .governance
            .iter()
            .find(|definition| {
                definition.kind == "tool" && definition.data == request["definition"]
            })
            .map(|definition| definition.id.as_str())
    });
    let task = state
        .tasks
        .iter()
        .find(|task| request["owner_id"] == task.id);
    let feature = task.map(|task| task.feature_id.as_str()).or_else(|| {
        state
            .features
            .iter()
            .find(|feature| request["owner_id"] == feature.id)
            .map(|feature| feature.id.as_str())
    });
    projected.data["invocation"] = json!({"id":record.id,"project_id":request["project_id"],"feature_id":feature,"task_id":task.map(|task|&task.id),"agent_instance_id":agent_instance,"session_id":caller["session_id"],"turn_id":caller["turn_id"],"operation_id":caller["operation_id"].as_str().unwrap_or(&record.id),"tool":{"revision_id":definition,"definition":request["definition"]},"arguments":request["arguments"],"input_hash":configuration_hash(&request["arguments"])});
    projected.data["platform_receipt"] = record.data["receipt"].clone();
    projected.data["receipt"]=state.governance.iter().find(|reply|reply.kind=="platform_tool_reply"&&reply.definition_id==record.id).map(|reply|reply.data["receipt"].clone()).unwrap_or_else(||json!({"success":record.data["status"]=="succeeded","structured_content":record.data["receipt"],"error":record.data["error"]}));
    projected
}
