use super::*;
use devdesk_adapters::integrations::{
    NativeTools, import_openapi, import_skill_zip, parse_skill_markdown,
};
use devdesk_core::ports::ToolExecutor;
fn text(v: &Value, k: &str) -> Result<String> {
    v[k].as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| err("invalid_params", format!("缺少 {k}")))
}
impl Workspace {
    fn integration_permissions(
        &self,
        e: &Engine,
        b: &IntegrationBinding,
    ) -> Result<(ExecutionPermissions, String)> {
        let state = e.store.snapshot()?;
        let selection = AgentPermissionSelection {
            sandbox_ref: b.sandbox_ref.clone(),
            harness: PermissionSelection::FollowTarget,
            sandbox: PermissionSelection::FollowTarget,
        };
        let lock = resolve_permission_lock(&state, &b.harness_ref, &selection)?;
        let h = state.configuration(&b.harness_ref)?;
        let installation = self.materialize_harness(e, h)?;
        require(
            installation.key.provider == HarnessProvider::Codex,
            "首版接入执行器只支持 Codex 的 Docker 环境",
        )?;
        let p = devdesk_adapters::permission_profiles::compile_permission_profiles(
            &state,
            &lock,
            Some(ExecutionScope {
                work_type: WorkType::Discussion,
                writable_paths: vec![],
                read_only_paths: vec![],
            }),
        )?;
        let image = match installation.artifact {
            RuntimeArtifact::DockerImage { image_id } => image_id,
            _ => return Err(err("unsupported", "工具执行器需要 Docker 镜像")),
        };
        Ok((p, image))
    }
    pub(super) fn integration_dispatch(
        &self,
        method: &str,
        params: &Value,
    ) -> Option<Result<Value>> {
        if !(method.starts_with("integration.")
            || method.starts_with("tool.call.")
            || method.starts_with("credentials.integration.")
            || [
                "query.integrations",
                "query.integration_bindings",
                "query.tool_calls",
                "skill.import",
                "skill.export",
                "skill.import_package",
                "integration.openapi.preview",
            ]
            .contains(&method))
        {
            return None;
        }
        Some((|| {
            let local = params["local"] == true
                || params["payload"]["scope"]["kind"] == "project"
                || method == "integration.project.bind";
            let global = self.global.snapshot()?;
            let target = params["target"].as_str().or(params["id"].as_str());
            let target_local =
                target.is_some_and(|id| !global.governance.iter().any(|r| r.id == id));
            let e = if local || target_local {
                let e = self.active_engine(params["project_id"].as_str())?;
                self.sync_configuration(&e)?;
                e
            } else {
                self.configuration.clone()
            };
            if method.starts_with("query.") || method == "skill.export" {
                return e.query(method, params, self.global.concurrency()?);
            }
            if method == "credentials.integration.configure" {
                let root = PathBuf::from(self.global.root());
                let reference = credentials::CredentialVault::new(&root)?
                    .configure_integration(&text(&params["payload"], "secret")?)?;
                return Ok(json!({"reference":reference,"configured":true}));
            }
            if method == "credentials.integration.status" {
                let root = PathBuf::from(self.global.root());
                let available = credentials::CredentialVault::new(&root)?
                    .read_integration(&text(&params["payload"], "reference")?)
                    .is_ok();
                return Ok(json!({"available":available}));
            }
            let mut c: Command = serde_json::from_value(params.clone())
                .map_err(|e| err("invalid_params", e.to_string()))?;
            let fp = command_fingerprint(method, &c);
            if let Some(previous) = e.store.replay(&c.command_id, &fp)? {
                return Ok(previous);
            }
            if method == "integration.openapi.preview" || method == "integration.openapi.import" {
                let source = text(&c.payload, "integration_ref")?;
                let document = if let Some(s) = c.payload["document"].as_str() {
                    require(s.len() <= 2_097_152, "OpenAPI 文件太大")?;
                    serde_yaml_value(s)?
                } else {
                    c.payload["document"].clone()
                };
                let selected: Vec<String> = serde_json::from_value(
                    c.payload.get("operations").cloned().unwrap_or(json!([])),
                )
                .map_err(|_| err("invalid_params", "operations 无效"))?;
                let tools = import_openapi(&document, &source, &selected)?;
                if method.ends_with("preview") {
                    return Ok(
                        json!({"tools":tools,"servers":document["servers"],"security_schemes":document.pointer("/components/securitySchemes")}),
                    );
                }
                c.payload["tools"] = json!(tools);
                return e.execute_resolved("integration.tools.publish", c, &fp);
            }
            if method == "skill.import_package" {
                let package = if let Some(encoded) = c.payload["zip_base64"].as_str() {
                    use base64::Engine;
                    require(encoded.len() <= 3_000_000, "Skill 包过大")?;
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(encoded)
                        .map_err(|_| err("invalid_params", "无效 ZIP base64"))?;
                    import_skill_zip(&bytes)?.remove(0)
                } else {
                    let body = text(&c.payload, "markdown")?;
                    let (description, _) = parse_skill_markdown(&body)?;
                    SkillPackage {
                        body,
                        description,
                        files: vec![],
                        requires_tools: serde_json::from_value(
                            c.payload
                                .get("requires_tools")
                                .cloned()
                                .unwrap_or(json!([])),
                        )
                        .map_err(|_| err("invalid_params", "无效依赖"))?,
                        source: c.payload["source"].as_str().unwrap_or("markdown").into(),
                        scripts_disabled: false,
                    }
                };
                c.payload["package"] = json!(package);
                return e.execute_resolved("skill.import", c, &fp);
            }
            if method == "integration.discover" || method == "integration.test" {
                let state = e.store.snapshot()?;
                let key = text(&c.payload, "binding_ref")?;
                let record = state.configuration(&key)?;
                require(record.kind == "integration_binding", "需要接入绑定修订")?;
                let binding: IntegrationBinding = record.decode()?;
                require(
                    devdesk_core::configuration_visible(
                        &state,
                        record,
                        e.store.project_id().as_deref(),
                    ),
                    "绑定作用域不可见",
                )?;
                let source = state.configuration(&binding.integration_ref)?;
                let definition: IntegrationDefinition = source.decode()?;
                let (permissions, image) = self.integration_permissions(&e, &binding)?;
                let executor = NativeTools::new(e.store.root())?;
                if method == "integration.discover" {
                    let result = executor.discover(&definition, &binding, &image, &permissions)?;
                    return e.record_tool_discovery(&c, &key, result);
                }
                let tool_id = text(&c.payload, "tool_ref")?;
                let r = state.configuration(&tool_id)?;
                let tool: ExternalTool = r.decode()?;
                require(tool.integration_ref == source.id, "工具与接入绑定来源不符")?;
                let invocation = ToolInvocation {
                    id: format!(
                        "tool-call-{}",
                        &configuration_hash(&json!([c.command_id, e.store.root()]))[..32]
                    ),
                    project_id: e.store.project_id(),
                    feature_id: None,
                    task_id: None,
                    agent_instance_id: None,
                    session_id: None,
                    turn_id: None,
                    run_id: None,
                    operation_id: None,
                    request_id: None,
                    call_id: c.command_id.clone(),
                    tool: BoundTool {
                        revision_id: r.id.clone(),
                        content_hash: r.content_hash.clone(),
                        definition: tool,
                        integration: definition,
                        integration_hash: source.content_hash.clone(),
                        binding_ref: key,
                        binding,
                        binding_hash: record.content_hash.clone(),
                    },
                    arguments: c.payload.get("arguments").cloned().unwrap_or(json!({})),
                    permissions,
                    image_id: image,
                    worktree: String::new(),
                    input_hash: configuration_hash(&c.payload["arguments"]),
                };
                let result = e.enqueue_tool_test(&c, &invocation)?;
                e.drive_tool_calls()?;
                return Ok(result);
            }
            if method == "integration.publish_tools" {
                let s = e.store.snapshot()?;
                let discovery = s.configuration(&text(&c.payload, "discovery_ref")?)?;
                require(discovery.kind == "integration_discovery", "需要发现记录")?;
                let b = s.configuration(&text(&discovery.data, "binding_ref")?)?;
                let binding: IntegrationBinding = b.decode()?;
                let names: Vec<String> = serde_json::from_value(c.payload["names"].clone())
                    .map_err(|_| err("invalid_params", "请选择发现的工具"))?;
                let descriptors = discovery.data["result"]["tools"]
                    .as_array()
                    .ok_or_else(|| err("invalid_configuration", "发现结果无工具"))?;
                let mut tools = vec![];
                for name in names {
                    let d = descriptors
                        .iter()
                        .find(|d| d["name"] == name)
                        .ok_or_else(|| err("invalid_params", "所选工具不在发现快照"))?;
                    let default = format!(
                        "mcp_{}_{}",
                        &configuration_hash(&json!(binding.integration_ref))[..8],
                        name.chars()
                            .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                                c
                            } else {
                                '_'
                            })
                            .take(42)
                            .collect::<String>()
                    );
                    tools.push(ExternalTool {
                        schema_version: 2,
                        qualified_name: c.payload["aliases"][&name]
                            .as_str()
                            .unwrap_or(&default)
                            .into(),
                        description: d["description"].as_str().unwrap_or(&name).into(),
                        integration_ref: binding.integration_ref.clone(),
                        implementation: ToolImplementation::Mcp {
                            name: name.clone(),
                            descriptor_hash: configuration_hash(d),
                        },
                        input_schema: d["inputSchema"].clone(),
                        output_schema: d.get("outputSchema").cloned(),
                        effect: c.payload["effects"][&name]
                            .as_str()
                            .unwrap_or("unknown")
                            .into(),
                        constraints: Default::default(),
                        sensitive_parameters: vec![],
                    });
                }
                c.payload["tools"] = json!(tools);
                return e.execute_resolved("integration.tools.publish", c, &fp);
            }
            if method == "tool.call.reconcile" {
                let state = e.store.snapshot()?;
                if let Some(record) = state
                    .governance
                    .iter()
                    .find(|r| Some(r.id.as_str()) == c.target.as_deref() && r.kind == "tool_call")
                {
                    let i: ToolInvocation =
                        serde_json::from_value(record.data["invocation"].clone())
                            .map_err(|_| err("invalid_configuration", "调用输入无效"))?;
                    if let Some(executor) = &e.tool_executor {
                        if executor.poll(&i)?.is_some() {
                            return Err(err(
                                "reconciliation_available",
                                "存在实际回执，先接纳执行器核对结果",
                            ));
                        }
                    }
                }
            }
            e.execute(method, c)
        })())
    }
}
fn serde_yaml_value(s: &str) -> Result<Value> {
    devdesk_adapters::integrations::parse_openapi_document(s)
}
