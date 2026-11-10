//! Integration resource commands and durable tool invocation coordination.
use crate::governance::{record, reference, update_record};
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

fn decode<T: serde::de::DeserializeOwned>(v: &Value) -> Result<T> {
    serde_json::from_value(v.clone()).map_err(|e| err("invalid_configuration", e.to_string()))
}
fn live(s: &EngineeringState, r: &ConfigurationRecord) -> Result<()> {
    enabled(s, r)?;
    require(!crate::configuration::retired(s, r), "接入已停用新的选择")
}
fn enabled(s: &EngineeringState, r: &ConfigurationRecord) -> Result<()> {
    require(
        !s.governance
            .iter()
            .any(|v| v.kind == "integration_disabled" && v.definition_id == r.definition_id),
        "接入已停用",
    )
}
fn latest<'a>(s: &'a EngineeringState, kind: &str, key: &str) -> Option<&'a ConfigurationRecord> {
    s.governance
        .iter()
        .filter(|r| r.kind == kind && r.definition_id == key)
        .max_by_key(|r| r.version)
}
pub fn freeze_integrations(
    s: &EngineeringState,
    a: &AgentComposition,
    project: Option<&str>,
) -> Result<IntegrationLock> {
    let scope = project
        .map(|p| Scope::Project {
            project_id: p.into(),
        })
        .unwrap_or(Scope::Global);
    let mapping = project
        .and_then(|p| latest(s, "integration_project_configuration", p))
        .map(|r| r.data.clone())
        .unwrap_or(json!({}));
    let mut lock = IntegrationLock::default();
    let mut names = BTreeSet::new();
    let mut sources = BTreeMap::new();
    for key in &a.tools {
        let r = s.configuration(key)?;
        if r.data["implementation"] == "platform" {
            live(s, r)?;
            let definition: ToolDefinition = r.decode()?;
            require(names.insert(definition.call_name()), "工具名称冲突")?;
            lock.platform_tools.push(r.clone());
            continue;
        }
        if r.data["schema_version"] != 2 {
            continue;
        }
        let tool: ExternalTool = r.decode()?;
        tool.validate()?;
        live(s, r)?;
        let source = reference(s, &tool.integration_ref, "integration", &scope)?;
        live(s, source)?;
        require(names.insert(tool.qualified_name.clone()), "工具名称冲突")?;
        require(
            sources
                .insert(source.definition_id.clone(), source.id.clone())
                .is_none_or(|id| id == source.id),
            "同一接入源不能混用目录修订",
        )?;
        let binding_id = mapping[&source.id].as_str().ok_or_else(|| {
            err(
                "integration_binding_required",
                format!("请为 {} 选择项目接入绑定", source.name),
            )
        })?;
        let binding = reference(s, binding_id, "integration_binding", &scope)?;
        live(s, binding)?;
        let b: IntegrationBinding = binding.decode()?;
        require(
            !matches!(b.auth, IntegrationAuth::OAuthPending { .. }),
            "接入 OAuth 授权尚未完成",
        )?;
        require(
            tool.required_auth
                .as_ref()
                .is_none_or(|auth| auth.satisfied_by(&b.auth)),
            "绑定认证不满足工具所需方案或 scope",
        )?;
        require(
            b.integration_ref == source.id
                && b.harness_ref == a.harness_ref
                && b.allowed_tools.contains(&r.id),
            "接入绑定不适用于该工具或 Harness",
        )?;
        lock.tools.push(BoundTool {
            revision_id: r.id.clone(),
            content_hash: r.content_hash.clone(),
            definition: tool,
            integration: source.decode()?,
            integration_hash: source.content_hash.clone(),
            binding_ref: binding.id.clone(),
            binding: b,
            binding_hash: binding.content_hash.clone(),
        });
    }
    for id in &a.skills {
        if let Some(r) = s
            .governance
            .iter()
            .find(|r| r.kind == "skill_package" && r.definition_id == *id)
        {
            let package: SkillPackage = r.decode()?;
            package.validate()?;
            for required in &package.requires_tools {
                require(
                    a.tools.iter().any(|key| {
                        key == required
                            || s.configuration(key)
                                .is_ok_and(|r| r.definition_id == *required)
                    }),
                    format!("Skill 缺少工具依赖: {required}"),
                )?;
            }
            lock.skills.insert(id.clone(), package);
        }
    }
    Ok(lock)
}
/// Catalog deletion preserves immutable revisions and frozen execution history.
/// Deleting a source also removes its own bindings/actions from future choices.
pub fn integration_deletion_targets(
    s: &EngineeringState,
    source: &ConfigurationRecord,
) -> Result<Vec<ConfigurationRecord>> {
    require(
        ["integration", "integration_binding", "tool"].contains(&source.kind.as_str())
            && (source.kind != "tool" || source.data["schema_version"] == 2),
        "只能删除接入源、绑定或已发布的外部工具",
    )?;
    let source_ids: Vec<_> = s
        .governance
        .iter()
        .filter(|r| r.kind == source.kind && r.definition_id == source.definition_id)
        .map(|r| r.id.as_str())
        .collect();
    let mut targets: Vec<_> = s
        .governance
        .iter()
        .filter(|r| r.kind == source.kind && r.definition_id == source.definition_id)
        .cloned()
        .collect();
    if source.kind == "integration" {
        // A tool/binding family may have moved to another source in a later revision.
        // Only cascade families whose current revision still belongs to this source.
        let families: BTreeSet<_> = s
            .governance
            .iter()
            .filter(|r| {
                ["integration_binding", "tool"].contains(&r.kind.as_str())
                    && r.data["integration_ref"]
                        .as_str()
                        .is_some_and(|id| source_ids.contains(&id))
                    && !s.governance.iter().any(|next| {
                        next.kind == r.kind
                            && next.definition_id == r.definition_id
                            && next.version > r.version
                    })
            })
            .map(|r| (r.kind.clone(), r.definition_id.clone()))
            .collect();
        targets.extend(
            s.governance
                .iter()
                .filter(|r| families.contains(&(r.kind.clone(), r.definition_id.clone())))
                .cloned(),
        );
    }
    Ok(targets)
}

impl Engine {
    pub(crate) fn project_tool_request(
        &self,
        s: &mut EngineeringState,
        session: &Session,
        tid: &str,
        observation: &ports::ConversationObservation,
    ) -> Result<()> {
        let ports::ConversationObservation::ToolCall {
            turn,
            request,
            thread,
            external_turn,
            call,
            tool,
            arguments,
            worktree,
        } = observation
        else {
            return Ok(());
        };
        let t = s
            .turns
            .iter()
            .find(|t| t.id == tid)
            .ok_or_else(|| err("not_found", "Turn 不存在"))?;
        require(
            turn == tid
                && session.external_session.as_ref() == Some(thread)
                && t.external_turn.as_ref() == Some(external_turn),
            "工具调用线程或轮次归属不符",
        )?;
        let b = s.turn_runtime_binding(t)?;
        let config = b
            .agent_configuration
            .as_ref()
            .ok_or_else(|| err("invalid_configuration", "缺少 Agent 工具锁"))?;
        if let Some(definition) = config.integration_lock.as_ref().and_then(|lock| {
            lock.platform_tools.iter().find(|definition| {
                definition
                    .decode::<ToolDefinition>()
                    .is_ok_and(|definition| definition.call_name() == *tool)
            })
        }) {
            return self.project_agent_platform_call(
                s,
                session,
                tid,
                observation,
                definition.clone(),
            );
        }
        let bound = config
            .integration_lock
            .as_ref()
            .and_then(|l| {
                l.tools
                    .iter()
                    .find(|v| v.definition.qualified_name == *tool)
            })
            .ok_or_else(|| err("tool_not_authorized", "工具未加入当前冻结 Capability"))?
            .clone();
        let permissions = b
            .permissions
            .clone()
            .ok_or_else(|| err("invalid_configuration", "外部工具要求明确 Permission"))?;
        let image = match &b.installation.artifact {
            RuntimeArtifact::DockerImage { image_id } => image_id.clone(),
            _ => return Err(err("unsupported", "工具执行要求 Docker 镜像")),
        };
        let operation = s
            .executions
            .iter()
            .find(|e| e.session_id == session.id && !e.writer_revoked)
            .map(|e| e.operation_id.clone())
            .ok_or_else(|| err("tool_not_authorized", "原生 worker 已静止"))?;
        let input_hash = configuration_hash(arguments);
        let identity = configuration_hash(&json!([session.id, tid, operation, call]));
        let invocation = ToolInvocation {
            id: format!("tool-call-{}", &identity[..32]),
            project_id: session.project_id.clone(),
            feature_id: session.feature_id.clone(),
            task_id: session.task_id.clone(),
            agent_instance_id: Some(config.agent_instance_id.clone()),
            session_id: Some(session.id.clone()),
            turn_id: Some(tid.into()),
            operation_id: Some(operation),
            request_id: Some(request.clone()),
            call_id: call.clone(),
            tool: bound,
            arguments: arguments.clone(),
            permissions,
            image_id: image,
            worktree: worktree.clone(),
            input_hash,
        };
        if let Err(error) = self.insert_tool_invocation(s, &invocation) {
            if s.configuration(&invocation.id).is_ok() {
                return Err(error);
            }
            let receipt = ToolReceipt {
                invocation_id: invocation.id.clone(),
                success: false,
                content: vec![json!({"type":"text","text":error.message})],
                structured_content: None,
                remote_effect: "not_applied".into(),
                error: Some(json!({"code":error.code})),
                origin: Origin::External,
            };
            let mut r = record(
                "tool_call",
                invocation.id.clone(),
                Scope::Project {
                    project_id: session.project_id.clone().unwrap_or_default(),
                },
                tool.clone(),
                1,
                None,
                json!({"status":"failed","remote_effect":"not_applied","invocation":invocation,"receipt":receipt,"created_at":now(),"finished_at":now(),"reply_sent":false}),
                false,
            );
            r.id = receipt.invocation_id.clone();
            s.governance.push(r);
        }
        Ok(())
    }
    fn insert_tool_invocation(
        &self,
        s: &mut EngineeringState,
        i: &ToolInvocation,
    ) -> Result<Value> {
        if let Ok(old) = s.configuration(&i.id) {
            require(
                old.data["invocation"]["input_hash"] == i.input_hash,
                "原生调用 ID 被不同参数复用",
            )?;
            return Ok(json!(old));
        }
        enabled(s, s.configuration(&i.tool.revision_id)?)?;
        enabled(s, s.configuration(&i.tool.definition.integration_ref)?)?;
        enabled(s, s.configuration(&i.tool.binding_ref)?)?;
        require(
            i.tool
                .definition
                .required_auth
                .as_ref()
                .is_none_or(|auth| auth.satisfied_by(&i.tool.binding.auth)),
            "绑定认证不满足工具所需方案或 scope",
        )?;
        self.executor()?
            .validate_arguments(&i.tool.definition.input_schema, &i.arguments)?;
        for (pointer, value) in &i.tool.definition.constraints {
            require(
                i.arguments.pointer(pointer) == Some(value),
                "参数超出绑定的资源范围",
            )?;
        }
        require(
            i.tool.binding.allowed_tools.contains(&i.tool.revision_id),
            "动作不在绑定授权中",
        )?;
        require(
            i.permissions.harness.policy.network == NetworkPermission::Enabled
                && i.permissions.sandbox.policy.network == NetworkPermission::Enabled
                || matches!(&i.tool.integration.source,IntegrationSource::Mcp{transport,..}if transport=="stdio"),
            "Permission 禁止外部网络工具",
        )?;
        if i.tool.definition.effect != "read_only" {
            require(
                i.permissions.harness.policy.filesystem != FilesystemPermission::ReadOnly,
                "只读权限不能执行写动作",
            )?;
        }
        let allowed = match &i.permissions.harness.policy.tools {
            ToolPermission::AllAvailable => true,
            ToolPermission::Restricted { allow, deny } => {
                !deny.contains(&i.tool.revision_id)
                    && (allow.is_empty() || allow.contains(&i.tool.revision_id))
            }
        };
        require(allowed, "Permission 未允许该工具")?;
        let status = if i.permissions.approval == ApprovalPolicy::OnRequest
            && i.tool.definition.effect != "read_only"
        {
            "awaiting_approval"
        } else {
            "prepared"
        };
        let scope = i
            .project_id
            .as_ref()
            .map(|id| Scope::Project {
                project_id: id.clone(),
            })
            .unwrap_or(Scope::Global);
        let mut r = record(
            "tool_call",
            i.id.clone(),
            scope,
            i.tool.definition.qualified_name.clone(),
            1,
            None,
            json!({"status":status,"remote_effect":"not_applied","invocation":i,"created_at":now(),"cancel_requested":false,"reply_sent":false}),
            false,
        );
        r.id = i.id.clone();
        let result = json!(r);
        s.governance.push(r);
        Ok(result)
    }
    /// Trusted coordinator entry: the browser never supplies the resolved invocation.
    pub fn enqueue_tool_test(&self, c: &Command, invocation: &ToolInvocation) -> Result<Value> {
        let fp = command_fingerprint("integration.test", c);
        self.store.transact(Some((&c.command_id, &fp)), &mut |s| {
            self.insert_tool_invocation(s, invocation)
        })
    }
    pub fn enqueue_configured_tool(
        &self,
        command: &Command,
        invocation: &ToolInvocation,
    ) -> Result<Value> {
        let expected = self.configured_tool_request(command)?;
        require(
            invocation.operation_id.as_ref() == Some(&expected.execution_id)
                && invocation.arguments == expected.arguments
                && json!(invocation.tool) == json!(expected.tool)
                && invocation.project_id.as_ref() == Some(&expected.project_id)
                && invocation.session_id.is_none()
                && invocation.turn_id.is_none(),
            "工具调用改变了冻结步骤、参数或归属",
        )?;
        require(
            invocation.permissions.binding_lock.as_ref() == Some(&expected.permissions),
            "工具权限与冻结步骤不一致",
        )?;
        let fingerprint = command_fingerprint("workflow.tool.execute", command);
        self.store
            .transact(Some((&command.command_id, &fingerprint)), &mut |state| {
                self.insert_tool_invocation(state, invocation)
            })
    }
    pub fn record_tool_discovery(
        &self,
        _c: &Command,
        binding_ref: &str,
        result: Value,
    ) -> Result<Value> {
        self.store.transact(None, &mut |s| {
            let b = s.configuration(binding_ref)?.clone();
            let previous = latest(s, "integration_discovery", binding_ref).map(|r|r.data["result"]["tools"].as_array().cloned().unwrap_or_default()).unwrap_or_default();
            let current = result["tools"].as_array().cloned().unwrap_or_default();
            let added:Vec<_> = current.iter().filter(|v|!previous.iter().any(|p|p["name"] == v["name"])).map(|v|v["name"].clone()).collect();
            let removed:Vec<_> = previous.iter().filter(|v|!current.iter().any(|p|p["name"] == v["name"])).map(|v|v["name"].clone()).collect();
            let changed:Vec<_> = current.iter().filter(|v|previous.iter().any(|p|p["name"] == v["name"] && configuration_hash(p) != configuration_hash(v))).map(|v|v["name"].clone()).collect();
            let r = record(
                "integration_discovery",
                binding_ref.into(),
                b.scope,
                "工具发现".into(),
                latest(s, "integration_discovery", binding_ref).map_or(1, |r| r.version + 1),
                None,
                json!({"binding_ref":binding_ref,"result":result,"diff":{"added":added,"removed":removed,"changed":changed},"captured_at":now()}),
                true,
            );
            let value = json!(r);
            s.governance.push(r);
            Ok(value)
        })
    }
    /// Persist an external operation before any discovery or Git import I/O.
    pub fn prepare_integration_operation(
        &self,
        method: &str,
        c: &Command,
        input: Value,
    ) -> Result<Value> {
        let fp = command_fingerprint(method, c);
        self.store.transact(Some((&c.command_id, &fp)), &mut |s| {
            let mut r = record(
                "integration_operation",
                c.command_id.clone(),
                self.store
                    .project_id()
                    .map(|project_id| Scope::Project { project_id })
                    .unwrap_or(Scope::Global),
                method.into(),
                1,
                None,
                json!({"status":"prepared", "input":input, "created_at":now()}),
                false,
            );
            r.id = format!(
                "integration-op-{}",
                &configuration_hash(&json!([c.command_id, self.store.root()]))[..32]
            );
            let value = json!(r);
            s.governance.push(r);
            Ok(value)
        })
    }
    pub fn finish_integration_operation(&self, key: &str, result: &Result<Value>) -> Result<()> {
        self.store.transact(None, &mut |s| {
            let r = s
                .governance
                .iter_mut()
                .find(|r| r.id == key && r.kind == "integration_operation")
                .ok_or_else(|| err("not_found", "接入操作意图缺失"))?;
            let mut data = r.data.clone();
            match result {
                Ok(result) => {
                    data["status"] = json!("succeeded");
                    data["result"] = result.clone();
                }
                Err(error) => {
                    data["status"] = json!("failed");
                    data["error"] = json!({"code":error.code,"message":error.message});
                }
            }
            data["finished_at"] = json!(now());
            update_record(r, data);
            Ok(json!({}))
        })?;
        Ok(())
    }
    pub fn recover_tool_receipts(&self) -> Result<()> {
        let Some(executor) = &self.tool_executor else {
            return Ok(());
        };
        for r in self
            .store
            .snapshot()?
            .governance
            .iter()
            .filter(|r| r.kind == "tool_call" && r.data["status"] == "needs_reconciliation")
        {
            let i: ToolInvocation = decode(&r.data["invocation"])?;
            if let Ok(Some(receipt)) = executor.poll(&i) {
                if r.data["receipt"] != json!(receipt) {
                    self.accept_tool_receipt(&r.id, &receipt)?;
                }
            }
        }
        Ok(())
    }
    pub fn drive_tool_calls(&self) -> Result<()> {
        // Reconcile late persisted receipts on every tick; never redispatch an unknown call.
        self.recover_tool_receipts()?;
        let Some(executor) = &self.tool_executor else {
            return Ok(());
        };
        let calls: Vec<_> = self
            .store
            .snapshot()?
            .governance
            .into_iter()
            .filter(|r| {
                r.kind == "tool_call"
                    && ([
                        "awaiting_approval",
                        "prepared",
                        "dispatched",
                        "succeeded",
                        "failed",
                        "cancelled",
                    ]
                    .contains(&r.data["status"].as_str().unwrap_or(""))
                        || (r.data["status"] == "needs_reconciliation"
                            && r.data["receipt"].is_object()
                            && r.data["reply_sent"] != true))
            })
            .collect();
        for r in calls {
            let invocation: ToolInvocation = decode(&r.data["invocation"])?;
            if ["succeeded", "failed", "cancelled"]
                .contains(&r.data["status"].as_str().unwrap_or(""))
                || (r.data["status"] == "needs_reconciliation" && r.data["receipt"].is_object())
            {
                if r.data["reply_sent"] != true {
                    if let (Some(sid), Some(request)) =
                        (&invocation.session_id, &invocation.request_id)
                    {
                        let state = self.store.snapshot()?;
                        let session = state
                            .sessions
                            .iter()
                            .find(|s| &s.id == sid)
                            .ok_or_else(|| err("not_found", "调用 Session 缺失"))?;
                        let receipt: ToolReceipt = decode(&r.data["receipt"])?;
                        let active = state
                            .executions
                            .iter()
                            .any(|e| e.session_id == *sid && !e.writer_revoked);
                        if !active {
                            self.tool_record_update(&r.id, |d| {
                                d["reply_sent"] = json!(true);
                                d["reply_status"] = json!("worker_unavailable");
                            })?;
                        } else if self
                            .harness
                            .answer_tool_call(session, request, &receipt)
                            .is_ok()
                        {
                            self.tool_record_update(&r.id, |d| {
                                d["reply_sent"] = json!(true);
                            })?;
                        }
                    }
                }
                continue;
            }
            if r.data["cancel_requested"] == true {
                if r.data["status"] != "dispatched" {
                    let receipt = ToolReceipt {
                        invocation_id: invocation.id.clone(),
                        success: false,
                        content: vec![
                            json!({"type":"text", "text":"Tool call cancelled before dispatch"}),
                        ],
                        structured_content: None,
                        remote_effect: "not_applied".into(),
                        error: Some(json!({"code":"cancelled"})),
                        origin: Origin::External,
                    };
                    self.accept_tool_receipt(&r.id, &receipt)?;
                    self.tool_record_update(&r.id, |d| d["status"] = json!("cancelled"))?;
                    continue;
                }
                executor.cancel(&invocation)?;
                self.tool_record_update(&r.id, |d| {
                    d["status"] = json!("needs_reconciliation");
                    d["remote_effect"] = json!("unknown");
                })?;
                continue;
            }
            if r.data["status"] == "awaiting_approval" {
                continue;
            }
            if r.data["status"] == "prepared" {
                self.tool_record_update(&r.id, |d| {
                    d["status"] = json!("dispatched");
                    d["dispatched_at"] = json!(now());
                })?;
                if let Err(error) = executor.start(&invocation) {
                    if error.code == "needs_reconciliation" {
                        self.tool_record_update(&r.id, |d| {
                            d["status"] = json!("needs_reconciliation");
                            d["remote_effect"] = json!("unknown");
                        })?;
                        continue;
                    }
                    let receipt = ToolReceipt {
                        invocation_id: invocation.id.clone(),
                        success: false,
                        content: vec![json!({"type":"text","text":error.message})],
                        structured_content: None,
                        remote_effect: "not_applied".into(),
                        error: Some(json!({"code":error.code})),
                        origin: Origin::External,
                    };
                    self.accept_tool_receipt(&r.id, &receipt)?;
                }
                continue;
            }
            match executor.poll(&invocation) {
                Ok(Some(receipt)) => self.accept_tool_receipt(&r.id, &receipt)?,
                Ok(None) => {}
                Err(_) => self.tool_record_update(&r.id, |d| {
                    d["status"] = json!("needs_reconciliation");
                    d["remote_effect"] = json!("unknown");
                })?,
            }
        }
        Ok(())
    }
    fn tool_record_update(&self, key: &str, mut f: impl FnMut(&mut Value)) -> Result<()> {
        self.store.transact(None, &mut |s| {
            let r = s
                .governance
                .iter_mut()
                .find(|r| r.id == key && r.kind == "tool_call")
                .ok_or_else(|| err("not_found", "ToolCall 缺失"))?;
            let mut data = r.data.clone();
            f(&mut data);
            update_record(r, data);
            Ok(json!({}))
        })?;
        Ok(())
    }
    fn accept_tool_receipt(&self, key: &str, receipt: &ToolReceipt) -> Result<()> {
        require(
            receipt.invocation_id == key && receipt.origin == Origin::External,
            "工具回执身份或来源不符",
        )?;
        self.tool_record_update(key, |d| {
            d["status"] = json!(if receipt.success {
                "succeeded"
            } else if receipt.remote_effect == "unknown" {
                "needs_reconciliation"
            } else {
                "failed"
            });
            d["remote_effect"] = json!(receipt.remote_effect);
            d["receipt"] = json!(receipt);
            d["finished_at"] = json!(now());
        })
    }
    pub fn with_tool_executor(mut self, executor: Arc<dyn ports::ToolExecutor>) -> Self {
        self.tool_executor = Some(executor);
        self
    }
    fn executor(&self) -> Result<&Arc<dyn ports::ToolExecutor>> {
        self.tool_executor
            .as_ref()
            .ok_or_else(|| err("unsupported", "工具执行器尚未配置"))
    }
    pub(crate) fn integration_command(
        &self,
        method: &str,
        c: &Command,
        s: &mut EngineeringState,
    ) -> Result<Option<Value>> {
        let p = &c.payload;
        if method == "integration.tools.publish" {
            let tools = p["tools"]
                .as_array()
                .ok_or_else(|| err("invalid_params", "需要工具清单"))?;
            require(
                !tools.is_empty() && tools.len() <= 256,
                "工具清单为空或过大",
            )?;
            let mut rows = vec![];
            for (index, tool) in tools.iter().enumerate() {
                let source = s.configuration(tool["integration_ref"].as_str().unwrap_or(""))?;
                let old = s
                    .governance
                    .iter()
                    .filter(|r| {
                        r.kind == "tool"
                            && r.data["schema_version"] == 2
                            && r.data["qualified_name"] == tool["qualified_name"]
                    })
                    .filter(|r| {
                        r.data["integration_ref"]
                            .as_str()
                            .and_then(|id| s.configuration(id).ok())
                            .is_some_and(|previous| previous.definition_id == source.definition_id)
                    })
                    .max_by_key(|r| r.version)
                    .cloned();
                let next = Command {
                    command_id: format!("{}:{index}", c.command_id),
                    target: old.as_ref().map(|r| r.id.clone()),
                    expected_revision: old.as_ref().map(|r| r.revision),
                    payload: json!({"name":tool["qualified_name"],"scope":p.get("scope").cloned().unwrap_or(json!({"kind":"global"})),"data":tool}),
                };
                rows.push(
                    self.integration_command(
                        if old.is_some() {
                            "integration.tool.revise"
                        } else {
                            "integration.tool.create"
                        },
                        &next,
                        s,
                    )?
                    .unwrap(),
                );
            }
            return Ok(Some(json!({"records":rows})));
        }
        if method == "integration.project.bind" {
            let project = text(p, "project_id")?;
            require(s.projects.iter().any(|r| r.id == project), "需要当前项目")?;
            let scope = Scope::Project {
                project_id: project.clone(),
            };
            let bindings = p
                .get("bindings")
                .and_then(Value::as_object)
                .ok_or_else(|| err("invalid_params", "需要 bindings"))?;
            for (source, key) in bindings {
                let r = reference(s, key.as_str().unwrap_or(""), "integration_binding", &scope)?;
                let b: IntegrationBinding = r.decode()?;
                require(&b.integration_ref == source, "接入绑定来源不符")?;
                live(s, r)?;
            }
            let old = latest(s, "integration_project_configuration", &project).cloned();
            if let Some(expected) = p.get("source_configuration_id") {
                require(
                    expected.as_str() == old.as_ref().map(|r| r.id.as_str()),
                    "项目接入映射已有新修订，请刷新后保存",
                )?;
            }
            let r = record(
                "integration_project_configuration",
                project,
                scope,
                "项目接入绑定".into(),
                old.as_ref().map_or(1, |r| r.version + 1),
                old.map(|r| r.id),
                json!(bindings),
                true,
            );
            let value = json!(r);
            s.governance.push(r);
            return Ok(Some(value));
        }
        if method == "integration.delete" {
            let source = s.configuration(c.target.as_deref().unwrap_or(""))?.clone();
            let targets = integration_deletion_targets(s, &source)?;
            let ids: Vec<_> = targets.iter().map(|r| r.id.as_str()).collect();
            for target in &targets {
                let refs = crate::configuration::integration_reference_users(s, target);
                let external: Vec<_> = refs
                    .iter()
                    .filter(|r| !ids.contains(&r["id"].as_str().unwrap_or("")))
                    .collect();
                require(
                    external.is_empty(),
                    format!(
                        "请先替换在用引用: {}",
                        external
                            .iter()
                            .filter_map(|r| r["name"].as_str())
                            .collect::<Vec<_>>()
                            .join("、")
                    ),
                )?;
            }
            let mut removed = vec![];
            for target in targets {
                if crate::configuration::retired(s, &target) {
                    continue;
                }
                let r = record(
                    "configuration_retirement",
                    target.id.clone(),
                    target.scope,
                    "删除接入资源".into(),
                    1,
                    None,
                    json!({"reason": opt(p,"reason").unwrap_or_else(|| "用户删除接入资源".into())}),
                    true,
                );
                removed.push(target.id);
                s.governance.push(r);
            }
            return Ok(Some(json!({"deleted":removed,"history_preserved":true})));
        }
        if method == "integration.disable" || method == "integration.retire" {
            let source = s.configuration(c.target.as_deref().unwrap_or(""))?.clone();
            require(
                ["integration", "integration_binding", "tool"].contains(&source.kind.as_str()),
                "无效接入资源",
            )?;
            let kind = if method == "integration.disable" {
                "integration_disabled"
            } else {
                "configuration_retirement"
            };
            if method.ends_with("retire") {
                crate::configuration::ensure_unreferenced(s, &source)?;
            }
            let r = record(
                kind,
                if kind == "configuration_retirement" {
                    source.id
                } else {
                    source.definition_id
                },
                source.scope,
                "接入停用".into(),
                1,
                None,
                json!({"reason":opt(p,"reason").unwrap_or_else(||"用户停用".into())}),
                true,
            );
            let value = json!(r);
            s.governance.push(r);
            return Ok(Some(value));
        }
        if method == "tool.call.cancel"
            && s.governance.iter().any(|record| {
                record.id == c.target.as_deref().unwrap_or("") && record.kind == "platform_call"
            })
        {
            let call = s
                .governance
                .iter_mut()
                .find(|record| record.id == c.target.as_deref().unwrap_or(""))
                .unwrap();
            require(
                ["prepared", "running"].contains(&call.data["status"].as_str().unwrap_or("")),
                "平台调用已结束或需要工作流核对",
            )?;
            let mut data = call.data.clone();
            data["cancel_requested"] = json!(true);
            update_record(call, data);
            return Ok(Some(json!(call)));
        }
        if method == "tool.call.cancel" || method == "tool.call.reconcile" {
            let index = s
                .governance
                .iter()
                .position(|r| r.id == c.target.as_deref().unwrap_or("") && r.kind == "tool_call")
                .ok_or_else(|| err("not_found", "ToolCall 不存在"))?;
            let mut data = s.governance[index].data.clone();
            if method.ends_with("reconcile") {
                require(
                    data["status"] == "needs_reconciliation",
                    "仅允许核对未确认调用",
                )?;
                let effect = text(p, "remote_effect")?;
                require(
                    ["not_applied", "applied", "unknown"].contains(&effect.as_str()),
                    "无效远端事实",
                )?;
                data["reconciliation"] =
                    json!({"effect":effect,"evidence":text(p,"evidence")?,"time":now()});
                data["remote_effect"] = json!(effect);
                if effect != "unknown" {
                    if data["receipt"].is_object() {
                        data["original_receipt"] = data["receipt"].clone();
                    }
                    data["status"] = json!("failed");
                    data["receipt"] = json!(ToolReceipt {
                        invocation_id: s.governance[index].id.clone(),
                        success: false,
                        content: vec![
                            json!({"type":"text", "text":"An operator reconciled this uncertain call. No automatic retry was performed."})
                        ],
                        structured_content: None,
                        remote_effect: effect,
                        error: Some(json!({"code":"human_reconciled"})),
                        origin: Origin::Human
                    });
                }
            } else {
                require(
                    !["succeeded", "failed", "cancelled"]
                        .contains(&data["status"].as_str().unwrap_or("")),
                    "调用已经结束",
                )?;
                data["cancel_requested"] = json!(true);
                if ["prepared", "awaiting_approval"]
                    .contains(&data["status"].as_str().unwrap_or(""))
                {
                    data["status"] = json!("cancelled");
                    data["finished_at"] = json!(now());
                    data["receipt"] = json!(ToolReceipt {
                        invocation_id: s.governance[index].id.clone(),
                        success: false,
                        content: vec![
                            json!({"type":"text","text":"User cancelled tool call before dispatch"})
                        ],
                        structured_content: None,
                        remote_effect: "not_applied".into(),
                        error: Some(json!({"code":"cancelled"})),
                        origin: Origin::Human
                    });
                }
            }
            update_record(&mut s.governance[index], data);
            return Ok(Some(json!(s.governance[index])));
        }
        if method == "tool.call.approve" {
            let r = s
                .governance
                .iter_mut()
                .find(|r| r.id == c.target.as_deref().unwrap_or("") && r.kind == "tool_call")
                .ok_or_else(|| err("not_found", "ToolCall 不存在"))?;
            require(
                r.data["status"] == "awaiting_approval",
                "该调用不在等待批准",
            )?;
            let mut data = r.data.clone();
            data["status"] = json!(if p["approve"] == true {
                "prepared"
            } else {
                "cancelled"
            });
            data["approved_input_hash"] = data["invocation"]["input_hash"].clone();
            if p["approve"] != true {
                data["receipt"] = json!(ToolReceipt {
                    invocation_id: r.id.clone(),
                    success: false,
                    content: vec![json!({"type":"text","text":"User declined tool call"})],
                    structured_content: None,
                    remote_effect: "not_applied".into(),
                    error: Some(json!({"code":"declined"})),
                    origin: Origin::External
                });
            }
            update_record(r, data);
            return Ok(Some(json!(r)));
        }
        if method == "skill.import" {
            let package: SkillPackage = decode(p.get("package").unwrap_or(p))?;
            package.validate()?;
            let scope: Scope = decode(p.get("scope").unwrap_or(&json!({"kind":"global"})))?;
            require(
                scope
                    .project()
                    .is_none_or(|id| self.store.project_id().as_deref() == Some(id)),
                "Skill 作用域不符",
            )?;
            let old = opt(p, "source_revision_id")
                .map(|id| reference(s, &id, "skill", &scope).cloned())
                .transpose()?;
            if let Some(previous) = &old {
                require(
                    latest(s, "skill", &previous.definition_id)
                        .is_some_and(|r| r.id == previous.id),
                    "Skill 已有新修订，请选择最新修订",
                )?;
            }
            let r = record(
                "skill",
                old.as_ref()
                    .map(|r| r.definition_id.clone())
                    .unwrap_or_else(id),
                scope.clone(),
                text(p, "name")?,
                old.as_ref().map_or(1, |r| r.version + 1),
                old.as_ref().map(|r| r.id.clone()),
                json!({"body":package.body,"rules":[]}),
                true,
            );
            s.definitions.push(Definition {
                id: r.id.clone(),
                family: "skill".into(),
                name: r.name.clone(),
                version: r.version,
                supersedes: r.supersedes.clone(),
                body: package.body.clone(),

                agent_profile: None,
            });
            let bundle = record(
                "skill_package",
                r.id.clone(),
                scope,
                r.name.clone(),
                1,
                None,
                json!(package),
                true,
            );
            s.governance.push(bundle);
            let value = json!(r);
            s.governance.push(r);
            return Ok(Some(value));
        }
        let family = match method {
            "integration.create" | "integration.revise" => "integration",
            "integration.binding.create" | "integration.binding.revise" => "integration_binding",
            "integration.tool.create" | "integration.tool.revise" => "tool",
            _ => return Ok(None),
        };
        let scope: Scope = decode(p.get("scope").unwrap_or(&json!({"kind":"global"})))?;
        require(
            !matches!(scope, Scope::Feature { .. })
                && scope
                    .project()
                    .is_none_or(|id| self.store.project_id().as_deref() == Some(id)),
            "接入作用域不符",
        )?;
        let data = p
            .get("data")
            .ok_or_else(|| err("invalid_params", "缺少 data"))?
            .clone();
        match family {
            "integration" => {
                let def: IntegrationDefinition = decode(&data)?;
                def.validate()?;
                if let Some(oauth) = &def.oauth {
                    require(
                        !oauth.client_id.trim().is_empty() && oauth.client_id.len() <= 1024,
                        "OAuth client_id 无效",
                    )?;
                    for endpoint in std::iter::once(&oauth.authorization_url)
                        .chain(std::iter::once(&oauth.token_url))
                        .chain(oauth.revocation_url.iter())
                    {
                        require(
                            (endpoint.starts_with("https://")
                                || endpoint.starts_with("http://127.0.0.1:")
                                || endpoint.starts_with("http://localhost:")
                                || endpoint.starts_with("http://host.docker.internal:"))
                                && !endpoint.contains(['@', '#', '\n', '\r']),
                            "OAuth endpoint 需要 HTTPS 或本机测试服务",
                        )?;
                    }
                    require(
                        oauth
                            .scopes
                            .iter()
                            .all(|s| !s.is_empty() && !s.chars().any(char::is_whitespace)),
                        "OAuth scope 不能含空格",
                    )?;
                }
                match def.source {
                    IntegrationSource::HttpApi { base_url } => require(
                        base_url.starts_with("https://") || base_url.starts_with("http://"),
                        "API 需要 HTTP(S) URL",
                    )?,
                    IntegrationSource::Mcp {
                        transport,
                        url,
                        command,
                        protocol_version,
                    } => {
                        require(
                            ["stdio", "streamable_http"].contains(&transport.as_str())
                                && ["2025-11-25", "2026-07-28"]
                                    .contains(&protocol_version.as_str()),
                            "不支持的 MCP 传输或协议版本",
                        )?;
                        require(
                            if transport == "stdio" {
                                !command.is_empty() && url.is_none()
                            } else {
                                url.is_some() && command.is_empty()
                            },
                            "MCP 地址/命令与传输不匹配",
                        )?;
                    }
                    IntegrationSource::SkillPackage { .. } => {}
                }
            }
            "integration_binding" => {
                let b: IntegrationBinding = decode(&data)?;
                b.validate_origins()?;
                let source = reference(s, &b.integration_ref, "integration", &scope)?;
                for key in &b.allowed_tools {
                    let tool = reference(s, key, "tool", &scope)?;
                    require(
                        tool.data["integration_ref"] == b.integration_ref,
                        "授权工具不属于绑定的接入源修订",
                    )?;
                }
                if let IntegrationAuth::OAuth {
                    account, scopes, ..
                }
                | IntegrationAuth::OAuthPending { account, scopes } = &b.auth
                {
                    let definition: IntegrationDefinition = source.decode()?;
                    let config = definition
                        .oauth
                        .as_ref()
                        .ok_or_else(|| err("oauth_configuration", "来源未配置 OAuth 公共客户端"))?;
                    require(
                        !account.trim().is_empty()
                            && account.len() <= 256
                            && scopes.iter().all(|scope| config.scopes.contains(scope)),
                        "账户标签或授权范围无效",
                    )?;
                }
                reference(s, &b.harness_ref, "harness", &scope)?;
                require(
                    s.governance
                        .iter()
                        .any(|r| r.id == b.sandbox_ref && r.kind == "sandbox")
                        || s.sandboxes.iter().any(|r| r.id == b.sandbox_ref),
                    "Sandbox 不存在",
                )?;
                require(
                    (100..=120_000).contains(&b.timeout_ms)
                        && (1024..=8_388_608).contains(&b.max_result_bytes),
                    "调用限制超出范围",
                )?;
                require(
                    b.allowed_tools.iter().collect::<BTreeSet<_>>().len() == b.allowed_tools.len(),
                    "工具授权重复",
                )?;
                if let IntegrationAuth::ApiKey { header, .. } = &b.auth {
                    require(
                        !header.is_empty()
                            && header
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                            && ![
                                "host",
                                "cookie",
                                "content-length",
                                "connection",
                                "authorization",
                                "proxy-authorization",
                                "transfer-encoding",
                            ]
                            .contains(&header.to_lowercase().as_str()),
                        "无效认证 Header",
                    )?;
                }
                if let Some(key) = b.auth.credential_ref() {
                    require(
                        key.strip_prefix("integration-secret:").is_some_and(|v| {
                            v.len() == 36 && v.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-')
                        }),
                        "需要系统凭据引用",
                    )?;
                }
            }
            "tool" => {
                let tool: ExternalTool = decode(&data)?;
                tool.validate()?;
                let source = reference(s, &tool.integration_ref, "integration", &scope)?;
                live(s, source)?;
                self.executor()?.validate_schema(&tool.input_schema)?;
                if let Some(schema) = &tool.output_schema {
                    self.executor()?.validate_schema(schema)?;
                }
                require(
                    matches!(
                        (
                            &tool.implementation,
                            source.decode::<IntegrationDefinition>()?.source
                        ),
                        (
                            ToolImplementation::HttpApi { .. },
                            IntegrationSource::HttpApi { .. }
                        ) | (
                            ToolImplementation::Mcp { .. },
                            IntegrationSource::Mcp { .. }
                        )
                    ),
                    "工具实现与接入源不符",
                )?;
            }
            _ => unreachable!(),
        }
        let old = if method.ends_with("revise") {
            let r = reference(s, c.target.as_deref().unwrap_or(""), family, &scope)?.clone();
            require(
                r.scope == scope && c.expected_revision == Some(r.revision),
                "接入修订冲突或作用域变更",
            )?;
            require(
                latest(s, family, &r.definition_id).is_some_and(|n| n.id == r.id),
                "已有新修订",
            )?;
            Some(r)
        } else {
            None
        };
        let r = record(
            family,
            old.as_ref()
                .map(|r| r.definition_id.clone())
                .unwrap_or_else(id),
            scope,
            text(p, "name")?,
            old.as_ref().map_or(1, |r| r.version + 1),
            old.map(|r| r.id),
            data,
            true,
        );
        let value = json!(r);
        s.governance.push(r);
        Ok(Some(value))
    }
    pub(crate) fn integration_query(&self, method: &str, p: &Value) -> Option<Result<Value>> {
        if ![
            "query.integrations",
            "query.tool_calls",
            "query.integration_bindings",
            "skill.export",
        ]
        .contains(&method)
        {
            return None;
        }
        Some((|| {
            let s = self.store.snapshot()?;
            if method == "skill.export" {
                let key = text(p, "id")?;
                let r = s.configuration(&key)?;
                let package=s.governance.iter().find(|r|r.kind=="skill_package"&&r.definition_id==key).map(|r|r.data.clone()).unwrap_or_else(||json!({"body":r.data["body"],"description":r.name,"files":[],"requires_tools":[],"source":"inline_markdown","scripts_disabled":false}));
                return Ok(package);
            }
            let rows = integration_records(&s, method, p);
            let offset = p["offset"].as_u64().unwrap_or(0) as usize;
            let limit = p["limit"].as_u64().unwrap_or(100).min(100) as usize;
            Ok(
                json!({"records":rows.iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":rows.len()}),
            )
        })())
    }
}

// Use the same projections and redaction for tool lists and the combined history.
pub(crate) fn integration_records(
    s: &EngineeringState,
    method: &str,
    p: &Value,
) -> Vec<ConfigurationRecord> {
    s.governance
        .iter()
        .filter(|r| match method {
            "query.tool_calls" => ["tool_call", "platform_call"].contains(&r.kind.as_str()),
            "query.integration_bindings" => {
                ["integration_binding", "integration_project_configuration"]
                    .contains(&r.kind.as_str())
            }
            _ => [
                "integration",
                "integration_binding",
                "integration_discovery",
                "integration_disabled",
                "skill_package",
                "integration_operation",
            ]
            .contains(&r.kind.as_str()),
        })
        .map(|record| {
            if record.kind == "platform_call" {
                crate::platform_tools::project_platform_call(s, record)
            } else {
                record.clone()
            }
        })
        .filter(|r| p["include_archived"] == true || template_record_current(s, r))
        .filter(|r| crate::configuration::visible(s, r, p["project_id"].as_str()))
        .filter(|r| {
            p["include_history"] == true
                || r.kind == "tool_call"
                || latest(s, &r.kind, &r.definition_id).is_none_or(|n| n.id == r.id)
        })
        .filter(|r| p["include_retired"] == true || !crate::configuration::retired(s, r))
        .filter(|r| {
            [
                "session_id",
                "turn_id",
                "feature_id",
                "task_id",
                "agent_instance_id",
            ]
            .iter()
            .all(|key| p.get(*key).is_none_or(|v| r.data["invocation"][key] == *v))
        })
        .filter(|r| {
            p.get("tool_ref")
                .is_none_or(|v| r.data["invocation"]["tool"]["revision_id"] == *v)
        })
        .filter(|r| {
            p.get("integration_ref")
                .is_none_or(|v| r.data["invocation"]["tool"]["definition"]["integration_ref"] == *v)
        })
        .rev()
        .map(|r| {
            let mut r = r.clone();
            if r.kind == "tool_call" {
                let pointers: Vec<String> = serde_json::from_value(
                    r.data["invocation"]["tool"]["definition"]["sensitive_parameters"].clone(),
                )
                .unwrap_or_default();
                for pointer in pointers {
                    if let Some(value) = r.data["invocation"]["arguments"].pointer_mut(&pointer) {
                        *value = json!("[redacted]");
                    }
                }
                // Query projections do not expose raw remote error bodies or system credentials.
            }
            r
        })
        .collect()
}
