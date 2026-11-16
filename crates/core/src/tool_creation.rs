//! Manual and workflow tool admission. Configuration validation uses the complete
//! catalog; original source references and absence predicates commit together.
use crate::{concurrency::*, *};
fn fingerprint(c: &Command, context: &InvocationContext) -> String {
    configuration_hash(&("tool-test-create/v1", c, context))
}
fn selectable(state: &EngineeringState, row: &ConfigurationRecord, scope: &Scope) -> Result<()> {
    require(
        row.scope.visible_from(scope)
            && !configuration_retired(state, row)
            && template_record_current(state, row),
        "测试调用的确切源已停用、归档或在范围外",
    )?;
    crate::integrations::enabled(state, row)
}
fn frozen_sources<'a>(
    catalog: &'a ConfigurationCatalogSnapshot,
    i: &ToolInvocation,
) -> Result<[&'a ConfigurationRecord; 3]> {
    validate_frozen_permissions(&catalog.state, &i.permissions)?;
    let tool = catalog.state.configuration(&i.tool.revision_id)?;
    let integration = catalog
        .state
        .configuration(&i.tool.definition.integration_ref)?;
    let binding = catalog.state.configuration(&i.tool.binding_ref)?;
    require(
        tool.content_hash == i.tool.content_hash
            && json!(tool.decode::<ExternalTool>()?) == json!(i.tool.definition)
            && integration.content_hash == i.tool.integration_hash
            && json!(integration.decode::<IntegrationDefinition>()?) == json!(i.tool.integration)
            && binding.content_hash == i.tool.binding_hash
            && json!(binding.decode::<IntegrationBinding>()?) == json!(i.tool.binding),
        "工具调用不能更改确切冻结源",
    )?;
    Ok([tool, integration, binding])
}
/// Captured before runtime preparation. Callers can inspect the frozen request,
/// but cannot replace its original intent, authority, command or Actor.
pub struct PreparedConfiguredTool {
    namespace: StoreNamespace,
    command: String,
    request: ConfiguredToolRequest,
}
impl PreparedConfiguredTool {
    pub fn request(&self) -> &ConfiguredToolRequest {
        &self.request
    }
}
impl Engine {
    pub fn prepare_configured_tool(&self, c: &Command) -> Result<PreparedConfiguredTool> {
        Ok(PreparedConfiguredTool {
            namespace: self.store.namespace(),
            command: command_fingerprint("workflow.tool.execute", c),
            request: self.configured_tool_request(c)?,
        })
    }
    pub fn enqueue_prepared_configured_tool(
        &self,
        c: &Command,
        i: &ToolInvocation,
        prepared: &PreparedConfiguredTool,
    ) -> Result<Value> {
        if prepared.namespace != self.store.namespace() {
            return Err(err("invalid_scope", "工具准备不能跨命名空间"));
        }
        require(
            prepared.command == command_fingerprint("workflow.tool.execute", c),
            "工具准备不能替换原命令",
        )?;
        if let Some(value) = self.replay_workflow_tool(c)? {
            return Ok(value);
        }
        let expected = &prepared.request;
        require(
            i.operation_id.as_ref() == Some(&expected.execution_id)
                && i.arguments == expected.arguments
                && json!(i.tool) == json!(expected.tool)
                && i.project_id.as_ref() == Some(&expected.project_id)
                && i.session_id.is_none()
                && i.turn_id.is_none()
                && i.request_id.is_none()
                && i.permissions.binding_lock.as_ref() == Some(&expected.permissions),
            "工具调用改变了原步骤、参数、归属或冻结权限",
        )?;
        self.create_workflow_tool_call(c, i, expected)
    }
    pub fn replay_workflow_tool(&self, c: &Command) -> Result<Option<Value>> {
        let Some(receipt) = self.store.replay_commit(
            &c.command_id,
            &command_fingerprint("workflow.tool.execute", c),
        )?
        else {
            return Ok(None);
        };
        if receipt.result["type"] == "tool_call" {
            self.store.restore_runtime_record(&receipt.result).map(Some)
        } else {
            Ok(Some(receipt.result))
        }
    }
    pub(crate) fn create_workflow_tool_call(
        &self,
        c: &Command,
        i: &ToolInvocation,
        expected: &ConfiguredToolRequest,
    ) -> Result<Value> {
        if let Some(value) = self.replay_workflow_tool(c)? {
            return Ok(value);
        }
        require(
            matches!(self.store.namespace(), StoreNamespace::Engineering { .. })
                && self
                    .store
                    .project_id()
                    .is_none_or(|id| id == expected.project_id),
            "工作流工具需要原项目库",
        )?;
        expected
            .provenance
            .context
            .require_project(&expected.project_id)?;
        let catalog = self.store.configuration_snapshot()?;
        let sources = frozen_sources(&catalog, i)?;
        let scope = Scope::project(&expected.project_id);
        require(
            sources.iter().all(|r| r.scope.visible_from(&scope)),
            "工具冻结源跨项目",
        )?;
        let workflow = self
            .store
            .workflow_tool_facts(&expected.authority, &expected.project_id)?;
        if workflow.suspended {
            return Err(err(
                "workflow_tool_suspended",
                "原步骤或祖先暂停，工具意图保持待执行",
            ));
        }
        if !workflow.allowed {
            return Err(err("revision_conflict", "工具原步骤或祖先资格已变化"));
        }
        let provenance = expected
            .provenance
            .context
            .prepare_record("workflow.tool.execute", c);
        let keys = vec![
            ResourceKey::new("governance", &i.id),
            provenance.resource.clone(),
            ResourceKey::new("tool_intent", &expected.provenance.intent.id),
            ResourceKey::new("governance", &expected.provenance.dispatch.id),
            ResourceKey::new("governance", &expected.provenance.decision.id),
            ResourceKey::new("catalog_clock", "configuration"),
            ResourceKey::new("source", &i.tool.revision_id),
            ResourceKey::new("source", &i.tool.definition.integration_ref),
            ResourceKey::new("source", &i.tool.binding_ref),
        ];
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == keys.len()
                && rows
                    .iter()
                    .zip(&keys)
                    .all(|(row, key)| &row.resource == key),
            "工作流工具创建上下文不完整",
        )?;
        if rows[0].value.is_some() || rows[1].value.is_some() {
            if let Some(value) = self.replay_workflow_tool(c)? {
                return Ok(value);
            }
            return Err(err("revision_conflict", "工具调用或来源已存在"));
        }
        if rows[2].revision != expected.provenance.intent.revision
            || rows[5].revision != catalog.revision
        {
            return Err(err("revision_conflict", "原工具意图或配置判定已变化"));
        }
        let intent = ConfigurationRecord::from_storage(
            rows[2]
                .value
                .clone()
                .ok_or_else(|| err("not_found", "原工具意图缺失"))?,
        )?;
        require(
            intent.scope == expected.provenance.intent.scope
                && intent.definition_id == expected.execution_id
                && intent.data["method"] == "workflow.tool.execute"
                && intent.data["command"] == json!(c)
                && intent.data["status"] == "prepared",
            "工具原意图不允许创建",
        )?;
        let dispatch = ConfigurationRecord::from_storage(
            rows[3]
                .value
                .clone()
                .ok_or_else(|| err("not_found", "原派发缺失"))?,
        )?;
        let decision = ConfigurationRecord::from_storage(
            rows[4]
                .value
                .clone()
                .ok_or_else(|| err("not_found", "原决定缺失"))?,
        )?;
        require(
            dispatch.r#type == "workflow_dispatch"
                && decision.r#type == "workflow_decision"
                && dispatch.immutable
                && decision.immutable
                && dispatch.data["decision_id"] == decision.id
                && dispatch.definition_id == expected.execution_id
                && dispatch.scope == intent.scope
                && decision.scope == intent.scope
                && decision.data["actor"] == json!(expected.provenance.context.actor())
                && decision.data["execution_id"] == expected.execution_id
                && decision.data["plan_revision_id"] == expected.authority.plan_revision_id
                && decision.data["operation"] == "dispatch",
            "工具原决定、Actor 或派发不符",
        )?;
        require(
            rows[6..].iter().all(|r| r.value.is_some()),
            "工具冻结源缺失",
        )?;
        let mut row = self.tool_call_record(&catalog.state, i)?;
        row.data["workflow_authority"] = json!(expected.authority);
        row.data["origin_context"] = json!(provenance.resource.id);
        row.data["origin_proof"] =
            json!({"intent_id":intent.id,"dispatch_id":dispatch.id,"decision_id":decision.id});
        row.content_hash = content_digest(&row.refs, &row.data);
        let encoded = self.store.encode_runtime_record(&row)?;
        require(
            self.store.restore_runtime_record(&encoded)? == json!(row),
            "工具冻结源重建不一致",
        )?;
        let mut versions = workflow.versions;
        versions.extend(rows.into_iter().map(|r| ResourceVersion {
            resource: r.resource,
            revision: r.revision,
        }));
        let receipt = self.store.commit_changes(&PreparedChangeSet {
            command_id: c.command_id.clone(),
            fingerprint: command_fingerprint("workflow.tool.execute", c),
            read_set: ReadSet::new(versions)?,
            changes: vec![
                ResourceChange {
                    resource: keys[0].clone(),
                    value: encoded.clone(),
                },
                provenance,
            ],
            result: encoded,
            time: now() as i64,
        })?;
        self.store.restore_runtime_record(&receipt.result)
    }
    /// Validate the original human request before runtime preparation can do I/O.
    pub fn preflight_tool_test(&self, c: &Command, context: &InvocationContext) -> Result<()> {
        context.require_scope(self.store.project_id().as_deref())?;
        if !matches!(context.actor(), WorkflowActor::Human) {
            return Err(err("permission_denied", "测试调用需要实际人类入口"));
        }
        if c.expected_revision.is_none() {
            return Err(err(
                "precondition_required",
                "新工具调用需要 expected_revision=0",
            ));
        }
        if c.expected_revision != Some(0) {
            return Err(err("revision_conflict", "新工具调用只允许不存在条件"));
        }
        require(c.target.is_none(), "测试调用不接受已有目标")?;
        let catalog = self.store.configuration_snapshot()?;
        let binding = catalog
            .state
            .configuration(&text(&c.payload, "binding_ref")?)?;
        let binding_value: IntegrationBinding = binding.decode()?;
        let tool = catalog
            .state
            .configuration(&text(&c.payload, "tool_ref")?)?;
        let definition: ExternalTool = tool.decode()?;
        let source = catalog.state.configuration(&definition.integration_ref)?;
        let scope = self
            .store
            .project_id()
            .map_or_else(Scope::global, Scope::project);
        require(
            binding.r#type == "integration_binding"
                && tool.r#type == "tool"
                && source.r#type == "integration"
                && [binding, tool, source]
                    .into_iter()
                    .all(|r| r.scope.visible_from(&scope))
                && binding_value.integration_ref == source.id
                && binding_value.allowed_tools.contains(&tool.id),
            "测试调用改变了原接入授权或范围",
        )?;
        for row in [binding, tool, source] {
            selectable(&catalog.state, row, &scope)?;
        }
        require(
            !matches!(binding_value.auth, IntegrationAuth::OAuthPending { .. })
                && definition
                    .required_auth
                    .as_ref()
                    .is_none_or(|auth| auth.satisfied_by(&binding_value.auth)),
            "测试调用尚未具备原接入认证",
        )?;
        let arguments = c.payload.get("arguments").cloned().unwrap_or(json!({}));
        for (pointer, value) in &definition.constraints {
            require(
                arguments.pointer(pointer) == Some(value),
                "测试参数超出原绑定资源范围",
            )?;
        }
        self.tool_executor
            .as_ref()
            .ok_or_else(|| err("unsupported", "工具执行器尚未配置"))?
            .validate_arguments(&definition.input_schema, &arguments)
    }
    pub fn replay_tool_test(
        &self,
        c: &Command,
        context: &InvocationContext,
    ) -> Result<Option<Value>> {
        context.require_scope(self.store.project_id().as_deref())?;
        if !matches!(context.actor(), WorkflowActor::Human) {
            return Err(err("permission_denied", "测试调用需要实际人类入口"));
        }
        let Some(receipt) = self
            .store
            .replay_commit(&c.command_id, &fingerprint(c, context))?
        else {
            self.preflight_tool_test(c, context)?;
            return Ok(None);
        };
        self.store.restore_runtime_record(&receipt.result).map(Some)
    }
    pub(crate) fn create_manual_tool_call(
        &self,
        c: &Command,
        i: &ToolInvocation,
        context: &InvocationContext,
    ) -> Result<Value> {
        if let Some(value) = self.replay_tool_test(c, context)? {
            return Ok(value);
        }
        if c.expected_revision.is_none() {
            return Err(err(
                "precondition_required",
                "新工具调用需要不存在条件 expected_revision=0",
            ));
        }
        if c.expected_revision != Some(0) {
            return Err(err("revision_conflict", "新工具调用只允许不存在条件"));
        }
        require(
            c.target.is_none()
                && i.session_id.is_none()
                && i.turn_id.is_none()
                && i.operation_id.is_none()
                && i.request_id.is_none(),
            "人工测试不能冒充会话或工作流执行",
        )?;
        require(
            i.project_id == self.store.project_id(),
            "测试调用范围与当前库不符",
        )?;
        require(
            c.payload["binding_ref"] == i.tool.binding_ref
                && c.payload["tool_ref"] == i.tool.revision_id
                && c.payload.get("arguments").cloned().unwrap_or(json!({})) == i.arguments
                && i.input_hash == configuration_hash(&i.arguments),
            "测试调用改变了原绑定、工具或参数",
        )?;
        let catalog = self.store.configuration_snapshot()?;
        let lock = resolve_permission_lock(
            &catalog.state,
            &i.tool.binding.harness_ref,
            &ExecutionPermissionSelection {
                sandbox_ref: i.tool.binding.sandbox_ref.clone(),
                harness: PermissionSelection::FollowTarget,
                sandbox: PermissionSelection::FollowTarget,
            },
        )?;
        if i.permissions.binding_lock.as_ref() != Some(&lock) {
            return Err(err("revision_conflict", "测试调用原权限绑定已变化"));
        }
        let sources = frozen_sources(&catalog, i)?;
        let scope = i
            .project_id
            .as_ref()
            .map_or_else(Scope::global, Scope::project);
        for row in sources {
            selectable(&catalog.state, row, &scope)?;
        }
        let provenance = context.prepare_record("integration.test", c);
        let call = ResourceKey::new("governance", &i.id);
        let clock = ResourceKey::new("catalog_clock", "configuration");
        let mut keys = vec![call.clone(), provenance.resource.clone(), clock.clone()];
        keys.extend(
            [
                &i.tool.revision_id,
                &i.tool.definition.integration_ref,
                &i.tool.binding_ref,
            ]
            .into_iter()
            .map(|id| ResourceKey::new("source", id)),
        );
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == keys.len()
                && rows
                    .iter()
                    .zip(&keys)
                    .all(|(row, key)| &row.resource == key),
            "工具创建上下文不完整",
        )?;
        if rows[2].revision != catalog.revision {
            return Err(err("revision_conflict", "工具配置在原读取期间已变化"));
        }
        if rows[0].value.is_some() || rows[1].value.is_some() {
            // A concurrent identical creator may have committed since the
            // first replay read. Return that original receipt, never reprepare.
            if let Some(value) = self.replay_tool_test(c, context)? {
                return Ok(value);
            }
            return Err(err("revision_conflict", "工具调用或命令来源已存在"));
        }
        require(
            rows[3..].iter().all(|row| row.value.is_some()),
            "工具确切源缺失",
        )?;
        let mut row = self.tool_call_record(&catalog.state, i)?;
        row.data["origin_context"] = json!(provenance.resource.id);
        row.content_hash = content_digest(&row.refs, &row.data);
        let encoded = self.store.encode_runtime_record(&row)?;
        require(
            self.store.restore_runtime_record(&encoded)? == json!(row),
            "测试调用不能改变冻结源重建结果",
        )?;
        let receipt = self.store.commit_changes(&PreparedChangeSet {
            command_id: c.command_id.clone(),
            fingerprint: fingerprint(c, context),
            read_set: ReadSet::new(
                rows.into_iter()
                    .map(|row| ResourceVersion {
                        resource: row.resource,
                        revision: row.revision,
                    })
                    .collect(),
            )?,
            changes: vec![
                ResourceChange {
                    resource: call,
                    value: encoded.clone(),
                },
                provenance,
            ],
            result: encoded,
            time: now() as i64,
        })?;
        self.store.restore_runtime_record(&receipt.result)
    }
}
