//! Freeze a platform request before image preparation. A later live lease may
//! admit that same request; it never recomputes arguments or changes its origin.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    material_commands::stable_object_id,
    tool_calls::WorkflowToolAuthority,
    workflow_resources::{WorkflowResources, version},
    *,
};

pub struct PreparedConfiguredPlatform {
    root: String,
    namespace: StoreNamespace,
    command: String,
    grant: ConfigurationRecord,
    guard: ResourceVersion,
    request: PlatformToolRequest,
    operation: Id,
}
impl PreparedConfiguredPlatform {
    pub fn request(&self) -> &PlatformToolRequest {
        &self.request
    }
    pub fn runtime_operation_id(&self) -> &str {
        &self.operation
    }
    pub fn runtime_required(&self) -> bool {
        self.request.definition.runtime.is_some()
            && self.grant.data["regression_not_configured"] != true
    }
}
impl Engine {
    pub fn prepare_configured_platform(
        &self,
        c: &Command,
    ) -> Result<Option<PreparedConfiguredPlatform>> {
        let fingerprint = command_fingerprint("workflow.tool.execute", c);
        let key = ResourceKey::new(
            "governance",
            format!("platform-preparation:{}", configuration_hash(&c.command_id)),
        );
        let saved = self
            .store
            .read_resources(std::slice::from_ref(&key))?
            .remove(0);
        if let Some(raw) = saved.value.as_ref() {
            let grant: ConfigurationRecord =
                serde_json::from_value(self.store.restore_runtime_record(raw)?)
                    .map_err(|e| err("storage", e.to_string()))?;
            require(
                grant.r#type == "platform_call_preparation",
                "平台准备类型不符",
            )?;
            let request = if grant.data["status"] == "committed"
                && grant.data.get("command_id").is_some()
            {
                let call_id = text(&grant.data, "call_id")?;
                require(
                    grant.data["command_id"] == c.command_id
                        && self
                            .replay_workflow_tool(c)?
                            .is_some_and(|result| result["id"] == call_id),
                    "已提交的平台准备缺少原命令回执",
                )?;
                let call = self
                    .store
                    .read_resources(&[ResourceKey::new("governance", &call_id)])?
                    .remove(0);
                let raw = call
                    .value
                    .as_ref()
                    .ok_or_else(|| err("storage", "平台调用回执缺失"))?;
                let call =
                    ConfigurationRecord::from_storage(self.store.restore_runtime_record(raw)?)?;
                require(
                    call.r#type == "platform_call" && call.data["preparation_id"] == grant.id,
                    "平台准备与原调用不匹配",
                )?;
                let mut request: PlatformToolRequest =
                    serde_json::from_value(call.data["request"].clone())
                        .map_err(|e| err("storage", e.to_string()))?;
                request.runtime = None;
                request
            } else {
                require(
                    grant.data["command"] == json!(c) && grant.data["fingerprint"] == fingerprint,
                    "平台准备身份被不同命令复用",
                )?;
                serde_json::from_value(grant.data["request"].clone())
                    .map_err(|e| err("storage", e.to_string()))?
            };
            let operation = text(&grant.data, "runtime_operation_id")?;
            return Ok(Some(PreparedConfiguredPlatform {
                root: self.store.root(),
                namespace: self.store.namespace(),
                command: fingerprint,
                grant,
                guard: version(&saved),
                request,
                operation,
            }));
        }
        let (intent, intent_guard) = self
            .original_runtime_intent("workflow.tool.execute", c)?
            .ok_or_else(|| err("permission_denied", "平台调用缺少原工作流意图"))?;
        let authority: WorkflowToolAuthority =
            serde_json::from_value(intent.data["execution_authority"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        require(
            c.payload["execution_id"] == authority.execution_id
                && intent.definition_id == authority.execution_id,
            "平台原步骤身份不符",
        )?;
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
            return Err(err(
                "workflow_tool_suspended",
                "原流程暂停，保留原调用等待恢复",
            ));
        }
        require(live.allowed, "原步骤资格已失效")?;
        let mut basis = ReadSet::new(vec![intent_guard])?;
        let mut origin = self.workflow_command_origin(
            "workflow.tool.execute",
            c,
            &resources,
            &intent,
            &mut basis,
        )?;
        let request = if let Some(invocation) = c.payload["agent_platform_call_id"].as_str() {
            let source = self
                .store
                .read_resources(&[ResourceKey::new(
                    "governance",
                    format!("agent-platform-request:{invocation}"),
                )])?
                .remove(0);
            let raw = source
                .value
                .as_ref()
                .ok_or_else(|| err("not_found", "原生平台调用来源缺失"))?;
            let row: ConfigurationRecord =
                serde_json::from_value(self.store.restore_runtime_record(raw)?)
                    .map_err(|e| err("storage", e.to_string()))?;
            let request: PlatformToolRequest = serde_json::from_value(row.data["request"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            require(
                row.r#type == "agent_platform_request"
                    && row.immutable
                    && row.definition_id == invocation
                    && request.execution_id == invocation
                    && request.step_execution_id() == authority.execution_id
                    && request.instance_id == *instance,
                "平台请求改变了原生来源",
            )?;
            let caller = request
                .caller
                .as_ref()
                .ok_or_else(|| err("storage", "原生平台调用缺少实际 worker"))?;
            let guards = self.platform_caller_guards(caller)?;
            basis = basis
                .merge([version(&source)])?
                .merge(guards.versions().iter().cloned())?;
            let mut audit = ConfigurationRecord::from_storage(origin.value)?;
            audit.data["origin"] = row.data["origin"].clone();
            audit.data["provider_poll_id"] = row.data["provider_poll_id"].clone();
            audit.content_hash = content_digest(&audit.refs, &audit.data);
            origin.value = audit.to_storage();
            request
        } else {
            let step = resources
                .instance
                .plan
                .document
                .steps
                .iter()
                .find(|s| s.key == resources.execution.step)
                .ok_or_else(|| err("storage", "原工具步骤定义缺失"))?;
            let StepExecutor::Tool { tool_ref, .. } = &step.executor else {
                return Ok(None);
            };
            let row = resources
                .instance
                .plan
                .definitions
                .get(tool_ref)
                .ok_or_else(|| err("configuration_required", "原工具未冻结"))?;
            if !matches!(
                row.data["implementation"].as_str(),
                Some("platform" | "domain")
            ) {
                return Ok(None);
            }
            let definition: ToolDefinition = row.decode()?;
            let arguments = c.payload["arguments"].clone();
            require(
                arguments.is_object()
                    && c.payload["arguments_digest"] == configuration_hash(&arguments),
                "工具参数必须来自原派发意图，不能重新读取当前上游",
            )?;
            PlatformToolRequest {
                definition_ref: Some(tool_ref.clone()),
                execution_id: authority.execution_id.clone(),
                instance_id: instance.clone(),
                owner_id: resources.instance.owner_id.clone(),
                project_id: resources.instance.project_id.clone(),
                definition,
                arguments,
                runtime: None,
                permissions: resources
                    .instance
                    .plan
                    .permission_locks
                    .get(tool_ref)
                    .cloned(),
                caller: None,
            }
        };
        require(
            request.owner_id == resources.instance.owner_id
                && request.project_id == resources.instance.project_id
                && request.runtime.is_none()
                // The kernel runs a platform handler or the handler of a domain tool.
                && (crate::platform_tools::PLATFORM_HANDLERS
                    .contains(&request.definition.qualified_name.as_str())
                    || request.definition.implementation == "domain"),
            "平台请求归属或处理器不符",
        )?;
        self.tool_executor
            .as_ref()
            .ok_or_else(|| err("unsupported", "缺少结构校验器"))?
            .validate_arguments(&request.definition.input_schema, &request.arguments)?;
        let source = ResourceKey::new(
            "source",
            request
                .definition_ref
                .as_ref()
                .ok_or_else(|| err("storage", "平台调用缺少确切工具源"))?,
        );
        let original = self
            .store
            .read_resources(std::slice::from_ref(&source))?
            .remove(0);
        require(original.value.is_some(), "平台工具源缺失")?;
        // The fine eligibility witness changes only if the original task,
        // plan, contract or session changes. Pause/resume is a later live lease.
        basis = basis.merge([version(&original)])?.merge(
            live.versions
                .iter()
                .filter(|v| {
                    v.resource.kind == "workflow_step_authority"
                        && v.resource.id == json!([instance, authority.execution_id])
                })
                .cloned(),
        )?;
        let operation = if uuid::Uuid::parse_str(&request.execution_id).is_ok() {
            request.execution_id.clone()
        } else {
            stable_object_id(
                self.store.as_ref(),
                &request.execution_id,
                "platform-runtime",
            )?
        };
        let mut grant = record(
            "platform_call_preparation",
            request.execution_id.clone(),
            resources.record.scope.clone(),
            "冻结平台调用".into(),
            1,
            None,
            json!({"status":"prepared","command":c,"fingerprint":fingerprint,"request":request,"authority":authority,"source_basis":basis,"origin":origin,"runtime_operation_id":operation,"root_instance_id":resources.instance.root_instance_id,"regression_not_configured":request.definition.qualified_name == "tests.project" && resources.instance.inputs.get("test_configuration").and_then(|c|c.get("regression")).is_some_and(Value::is_null)}),
            false,
        );
        grant.id = key.id.clone();
        let initial = basis
            .merge(resources.reads.versions().iter().cloned())?
            .merge(live.versions)?
            .merge([version(&saved)])?;
        self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("platform-prepare:{}", c.command_id),
            fingerprint: configuration_hash(&("platform-prepare/v1", c)),
            read_set: initial,
            changes: vec![ResourceChange {
                resource: key.clone(),
                value: self.store.encode_runtime_record(&grant)?,
            }],
            result: json!({"id":grant.id}),
            time: now() as i64,
        })?;
        Ok(Some(PreparedConfiguredPlatform {
            root: self.store.root(),
            namespace: self.store.namespace(),
            command: fingerprint,
            grant,
            guard: ResourceVersion {
                resource: key,
                revision: 1,
            },
            request,
            operation,
        }))
    }
    pub(crate) fn platform_caller_guards(
        &self,
        caller: &crate::platform_tools::AgentPlatformCaller,
    ) -> Result<ReadSet> {
        self.native_worker_guards(&caller.session_id, &caller.turn_id, &caller.operation_id)
    }
    pub(crate) fn native_worker_guards(
        &self,
        session: &str,
        turn: &str,
        worker: &str,
    ) -> Result<ReadSet> {
        let keys = [
            ResourceKey::new("tool_worker", worker),
            ResourceKey::new("tool_turn", turn),
        ];
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == 2 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "原平台 worker 资格读取不完整",
        )?;
        if !(rows[0].revision == 1
            && rows[0]
                .value
                .as_ref()
                .is_some_and(|w| w["session_id"] == session)
            && rows[1].value.as_ref().is_some_and(|t| {
                t["session_id"] == session
                    && t["status"] == "running"
                    && t["cancel_requested"] == false
            }))
        {
            return Err(err(
                "revision_conflict",
                "原平台调用的 worker 或 Turn 已撤销",
            ));
        }
        ReadSet::new(rows.iter().map(version).collect())
    }
    pub fn enqueue_prepared_platform_tool(
        &self,
        c: &Command,
        request: &PlatformToolRequest,
        prepared: &PreparedConfiguredPlatform,
    ) -> Result<Value> {
        require(
            prepared.root == self.store.root()
                && prepared.namespace == self.store.namespace()
                && prepared.command == command_fingerprint("workflow.tool.execute", c),
            "平台准备不能跨库或更换原命令",
        )?;
        if let Some(result) = self.replay_workflow_tool(c)? {
            return Ok(result);
        }
        let mut literal = request.clone();
        literal.runtime = None;
        require(
            json!(literal) == json!(prepared.request),
            "平台请求改变了原冻结依据",
        )?;
        if let Some(runtime) = &request.runtime {
            let source = request
                .definition
                .runtime
                .as_ref()
                .ok_or_else(|| err("invalid_params", "此工具未声明运行环境"))?;
            require(
                runtime.id == prepared.operation
                    && runtime.operation_id == prepared.operation
                    && runtime.harness.id == source.harness_ref
                    && runtime.sandbox.id == source.sandbox_ref
                    && runtime
                        .sandbox
                        .image
                        .as_ref()
                        .is_some_and(|i| !i.is_empty()),
                "平台运行环境改变了原确切源或操作身份",
            )?;
        } else {
            require(!prepared.runtime_required(), "工具需要原冻结运行环境")?;
        }
        let authority: WorkflowToolAuthority =
            serde_json::from_value(prepared.grant.data["authority"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        let live = self
            .store
            .workflow_tool_facts(&authority, &request.project_id)?;
        if live.suspended {
            return Err(err(
                "workflow_tool_suspended",
                "原流程暂停，平台准备保持原请求",
            ));
        }
        require(live.allowed, "平台原执行资格已失效")?;
        let basis: ReadSet = serde_json::from_value(prepared.grant.data["source_basis"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let origin: ResourceChange = serde_json::from_value(prepared.grant.data["origin"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let key = ResourceKey::new("governance", &request.execution_id);
        let mut reads = basis.merge(live.versions)?.merge([
            prepared.guard.clone(),
            ResourceVersion {
                resource: key.clone(),
                revision: 0,
            },
            ResourceVersion {
                resource: origin.resource.clone(),
                revision: 0,
            },
        ])?;
        if let Some(caller) = &request.caller {
            reads = reads.merge(
                self.platform_caller_guards(caller)?
                    .versions()
                    .iter()
                    .cloned(),
            )?;
        }
        let mut row = record(
            "platform_call",
            request.execution_id.clone(),
            prepared.grant.scope.clone(),
            request.definition.qualified_name.clone(),
            1,
            None,
            json!({"status":"prepared","request":request,"handler":request.definition.qualified_name,"root_instance_id":prepared.grant.data["root_instance_id"],"workflow_authority":authority,"preparation_id":prepared.grant.id,"issuer":"kernel_platform","operations":{"base":stable_object_id(self.store.as_ref(),&request.execution_id,"publish-base")?,"candidate":stable_object_id(self.store.as_ref(),&request.execution_id,"publish-candidate")?,"pull_request":stable_object_id(self.store.as_ref(),&request.execution_id,"publish-pr")?}}),
            false,
        );
        row.id = request.execution_id.clone();
        let mut grant = prepared.grant.clone();
        let mut data = grant.data.clone();
        data["status"] = json!("committed");
        data["call_id"] = json!(row.id);
        data["command_id"] = json!(c.command_id);
        data["origin_id"] = json!(origin.resource.id);
        for field in [
            "command",
            "fingerprint",
            "request",
            "authority",
            "source_basis",
            "origin",
        ] {
            data.as_object_mut().unwrap().remove(field);
        }
        update_record(&mut grant, data);
        self.store
            .commit_changes(&PreparedChangeSet {
                command_id: c.command_id.clone(),
                fingerprint: prepared.command.clone(),
                read_set: reads,
                changes: vec![
                    ResourceChange {
                        resource: key,
                        value: self.store.encode_runtime_record(&row)?,
                    },
                    ResourceChange {
                        resource: prepared.guard.resource.clone(),
                        value: self.store.encode_runtime_record(&grant)?,
                    },
                    origin,
                ],
                result: json!({"id":row.id}),
                time: now() as i64,
            })
            .map(|r| r.result)
    }
    // Compatibility entry points consume the already persisted preparation;
    // they cannot reconstruct or refresh an earlier request after I/O.
    pub fn configured_platform_request(&self, c: &Command) -> Result<Option<PlatformToolRequest>> {
        self.prepare_configured_platform(c)
            .map(|p| p.map(|p| p.request))
    }
    pub fn enqueue_platform_tool(
        &self,
        c: &Command,
        request: &PlatformToolRequest,
    ) -> Result<Value> {
        let prepared = self
            .prepare_configured_platform(c)?
            .ok_or_else(|| err("invalid_params", "不是平台工具请求"))?;
        self.enqueue_prepared_platform_tool(c, request, &prepared)
    }
}
