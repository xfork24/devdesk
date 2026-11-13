use super::*;
use devdesk_adapters::integrations::{
    NativeTools, import_openapi_with_auth, import_skill_zip, parse_skill_markdown,
};
use devdesk_core::ports::ToolExecutor;
fn text(v: &Value, k: &str) -> Result<String> {
    v[k].as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| err("invalid_params", format!("缺少 {k}")))
}
impl Workspace {
    pub(super) fn execute_configured_tool(&self, e: &Engine, command: Command) -> Result<Value> {
        let fingerprint = command_fingerprint("workflow.tool.execute", &command);
        if let Some(result) = e.store.replay(&command.command_id, &fingerprint)? {
            return Ok(result);
        }
        if let Some(mut request) = e.configured_platform_request(&command)? {
            if let Some(runtime) = &request.definition.runtime {
                let state = e.store.snapshot()?;
                let harness = state.configuration(&runtime.harness_ref)?;
                let installation = self.materialize_harness(e, harness)?;
                let image = match installation.artifact {
                    RuntimeArtifact::DockerImage { image_id } => image_id,
                    _ => return Err(err("unsupported", "测试工具需要确切 Docker 镜像")),
                };
                let lock = request
                    .permissions
                    .as_ref()
                    .ok_or_else(|| err("permission_unconfigured", "平台工具缺少冻结权限"))?;
                let _ = permission_profiles::compile_permission_profiles(
                    &state,
                    lock,
                    Some(ExecutionScope {
                        work_type: WorkType::Workflow,
                        writable_paths: vec![],
                        read_only_paths: vec![],
                    }),
                )?;
                request.runtime = Some(ExecutionInstance {
                    id: request.execution_id.clone(),
                    operation_id: request.execution_id.clone(),
                    harness: Harness {
                        id: runtime.harness_ref.clone(),
                        provider: "docker".into(),
                        version: installation.key.version.clone(),
                        capabilities: vec![],
                        origin: Origin::External,
                        permission: None,
                    },
                    sandbox: Sandbox {
                        id: runtime.sandbox_ref.clone(),
                        provider: "docker".into(),
                        version: installation.key.version,
                        platform: "linux".into(),
                        image: Some(image),
                        network: false,
                        origin: Origin::External,
                        permission: None,
                    },
                    external_session: None,
                    external_environment: None,
                    status: Status::Prepared,
                    started: None,
                    finished: None,
                });
            }
            return e.enqueue_platform_tool(&command, &request);
        }
        let request = e.configured_tool_request(&command)?;
        let state = e.store.snapshot()?;
        let harness = state.configuration(&request.tool.binding.harness_ref)?;
        let installation = self.materialize_harness(e, harness)?;
        let permissions = devdesk_adapters::permission_profiles::compile_permission_profiles(
            &state,
            &request.permissions,
            Some(ExecutionScope {
                work_type: WorkType::Discussion,
                writable_paths: vec![],
                read_only_paths: vec![],
            }),
        )?;
        let image_id = match installation.artifact {
            RuntimeArtifact::DockerImage { image_id } => image_id,
            _ => return Err(err("unsupported", "工具需要固定 Docker 镜像")),
        };
        let task = state.tasks.iter().find(|t| t.id == request.owner_id);
        let feature = task.map(|t| t.feature_id.clone()).or_else(|| {
            state
                .features
                .iter()
                .find(|f| f.id == request.owner_id)
                .map(|f| f.id.clone())
        });
        let invocation = ToolInvocation {
            id: format!(
                "tool-call-{}",
                configuration_hash(&(&command.command_id, &request.execution_id))
            ),
            project_id: Some(request.project_id.clone()),
            feature_id: feature,
            task_id: task.map(|t| t.id.clone()),
            agent_instance_id: None,
            session_id: None,
            turn_id: None,

            operation_id: Some(request.execution_id),
            request_id: None,
            call_id: command.command_id.clone(),
            tool: request.tool,
            input_hash: configuration_hash(&request.arguments),
            arguments: request.arguments,
            permissions,
            image_id,
            worktree: String::new(),
        };
        e.enqueue_configured_tool(&command, &invocation)
    }

    fn integration_permissions(
        &self,
        e: &Engine,
        b: &IntegrationBinding,
    ) -> Result<(ExecutionPermissions, String)> {
        let state = e.store.snapshot()?;
        let selection = ExecutionPermissionSelection {
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
    pub fn integration_oauth_callback(&self, state: &str, code: &str) -> Result<Value> {
        let root = PathBuf::from(self.global.root());
        let flow = devdesk_adapters::integration_oauth::read_flow(&root, state)?;
        let e = if let Some(pid) = &flow.project_id {
            let e = self.active_engine(Some(pid))?;
            self.sync_configuration(&e)?;
            e
        } else {
            self.configuration.clone()
        };
        let s = e.store.snapshot()?;
        let binding: IntegrationBinding = s.configuration(&flow.binding_ref)?.decode()?;
        let (permissions, image) = self.integration_permissions(&e, &binding)?;
        let executor = NativeTools::new(e.store.root())?;
        let vault = credentials::CredentialVault::new(&root)?;
        let result = devdesk_adapters::integration_oauth::finish(
            &root,
            state,
            code,
            &vault,
            |url, form| executor.oauth_request(url, form, &binding, &image, &permissions),
        )?;
        Ok(json!({"state":result.id,"status":result.status,"credential_ref":result.credential_ref}))
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
            let definition_write = [
                "integration.create",
                "integration.revise",
                "integration.binding.create",
                "integration.binding.revise",
                "integration.tool.create",
                "integration.tool.revise",
                "integration.openapi.import",
                "integration.openapi.preview",
                "skill.import",
                "skill.import_package",
            ]
            .contains(&method);
            let local = (!definition_write
                && (params["local"] == true || params["project_id"].is_string()))
                || params["payload"]["scope"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("project:"))
                || method == "integration.project.bind";
            let global = self.global.snapshot()?;
            let target = params["target"].as_str().or(params["id"].as_str());
            let target_local =
                target.is_some_and(|id| !global.governance.iter().any(|r| r.id == id));
            let e = if (local && !(method == "integration.delete" && !target_local)) || target_local
            {
                let e = self.active_engine(params["project_id"].as_str())?;
                self.sync_configuration(&e)?;
                e
            } else {
                self.configuration.clone()
            };
            if method == "tool.call.artifact" {
                let s = e.store.snapshot()?;
                let r = s.configuration(params["target"].as_str().unwrap_or(""))?;
                require(
                    r.r#type == "tool_call"
                        && configuration_visible(&s, r, e.store.project_id().as_deref()),
                    "文件结果不属于当前作用域",
                )?;
                let receipt: ToolReceipt = serde_json::from_value(r.data["receipt"].clone())
                    .map_err(|_| err("not_found", "文件调用尚无回执"))?;
                let (bytes, mime) =
                    NativeTools::new(e.store.root())?.read_artifact(&r.id, &receipt)?;
                use base64::Engine;
                return Ok(
                    json!({"base64":base64::engine::general_purpose::STANDARD.encode(bytes),"mime":mime}),
                );
            }
            if method.starts_with("query.") || method == "skill.export" {
                return e.query(method, params, self.global.concurrency()?);
            }
            if method == "credentials.integration.configure" {
                let root = PathBuf::from(self.global.root());
                let vault = credentials::CredentialVault::new(&root)?;
                let secret = text(&params["payload"], "secret")?;
                let reference = if let Some(reference) = params["payload"]["reference"].as_str() {
                    vault.write_integration(reference, &secret)?;
                    reference.to_owned()
                } else {
                    vault.configure_integration(&secret)?
                };
                return Ok(json!({"reference":reference,"configured":true}));
            }
            if method == "credentials.integration.status" {
                let root = PathBuf::from(self.global.root());
                let available = credentials::CredentialVault::new(&root)?
                    .read_integration(&text(&params["payload"], "reference")?)
                    .is_ok();
                return Ok(json!({"available":available}));
            }
            if method.starts_with("integration.oauth.") {
                let root = PathBuf::from(self.global.root());
                let vault = credentials::CredentialVault::new(&root)?;
                if method == "integration.oauth.status" {
                    let flow = devdesk_adapters::integration_oauth::read_flow(
                        &root,
                        &text(&params["payload"], "state")?,
                    )?;
                    require(
                        flow.project_id
                            .as_deref()
                            .is_none_or(|id| Some(id) == e.store.project_id().as_deref()),
                        "OAuth 流程不属于当前项目",
                    )?;
                    return Ok(
                        json!({"state":flow.id,"status":flow.status,"credential_ref":flow.credential_ref,"account":flow.account,"scopes":flow.scopes}),
                    );
                }
                let s = e.store.snapshot()?;
                let binding_id = text(&params["payload"], "binding_ref")?;
                let b = s.configuration(&binding_id)?;
                let binding: IntegrationBinding = b.decode()?;
                let source: IntegrationDefinition =
                    s.configuration(&binding.integration_ref)?.decode()?;
                let config = source.oauth.as_ref().ok_or_else(|| {
                    err(
                        "oauth_configuration",
                        "请先在接入源配置 OAuth client 与端点",
                    )
                })?;
                if method == "integration.oauth.start" {
                    let scopes: Vec<String> = serde_json::from_value(
                        params["payload"]
                            .get("scopes")
                            .cloned()
                            .unwrap_or(json!(config.scopes)),
                    )
                    .map_err(|_| err("invalid_params", "无效 OAuth scopes"))?;
                    let flow = devdesk_adapters::integration_oauth::start(
                        &root,
                        config,
                        &binding_id,
                        e.store.project_id(),
                        &text(&params["payload"], "redirect_uri")?,
                        &text(&params["payload"], "account")?,
                        scopes,
                        &vault,
                    )?;
                    return Ok(
                        json!({"state":flow.id,"authorization_url":flow.authorization_url,"expires_at":flow.expires_at}),
                    );
                }
                if method == "integration.oauth.revoke" {
                    let reference = binding
                        .auth
                        .credential_ref()
                        .ok_or_else(|| err("oauth_credential", "绑定没有授权凭据"))?;
                    let (permissions, image) = self.integration_permissions(&e, &binding)?;
                    let executor = NativeTools::new(e.store.root())?;
                    return devdesk_adapters::integration_oauth::revoke(
                        reference,
                        config,
                        &vault,
                        |url, form| {
                            executor.oauth_request(url, form, &binding, &image, &permissions)
                        },
                    );
                }
                return Err(err("method_not_found", "未知 OAuth 操作"));
            }
            let mut c: Command = serde_json::from_value(params.clone())
                .map_err(|e| err("invalid_params", e.to_string()))?;
            let fp = command_fingerprint(method, &c);
            if let Some(previous) = e.store.replay(&c.command_id, &fp)? {
                if previous["type"] == "integration_operation" {
                    let state = e.store.snapshot()?;
                    let operation = state.configuration(previous["id"].as_str().unwrap_or(""))?;
                    if operation.data["status"] == "succeeded" {
                        return Ok(operation.data["result"].clone());
                    }
                    return Err(err(
                        "needs_reconciliation",
                        "先前接入操作已记录，未自动重复外部 I/O；请检查操作状态后以新操作重试",
                    ));
                }
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
                let state = e.store.snapshot()?;
                let definition: IntegrationDefinition = state.configuration(&source)?.decode()?;
                if let Some(servers) = document["servers"].as_array() {
                    if !servers.is_empty() {
                        let server = c.payload["server_index"].as_u64().unwrap_or(0) as usize;
                        let chosen = servers
                            .get(server)
                            .ok_or_else(|| err("invalid_params", "所选 server 不存在"))?;
                        let IntegrationSource::HttpApi { base_url } = &definition.source else {
                            return Err(err("invalid_configuration", "OpenAPI 需要 HTTP 接入源"));
                        };
                        require(
                            chosen.get("variables").is_none()
                                && chosen["url"].as_str().is_some_and(|url| {
                                    url.trim_end_matches('/') == base_url.trim_end_matches('/')
                                }),
                            "所选 OpenAPI server 必须与当前接入源 Base URL 一致；变量需先展开",
                        )?;
                    }
                }
                let mut tools = import_openapi_with_auth(
                    &document,
                    &source,
                    &selected,
                    c.payload["security_scheme"]
                        .as_str()
                        .filter(|s| !s.is_empty()),
                )?;
                if method.ends_with("preview") {
                    return Ok(
                        json!({"tools":tools,"servers":document["servers"],"security_schemes":document.pointer("/components/securitySchemes")}),
                    );
                }
                for tool in &mut tools {
                    if let Some(alias) = c.payload["aliases"][&tool.qualified_name].as_str() {
                        tool.qualified_name = alias.into();
                        tool.validate()?;
                    }
                }
                c.payload["tools"] = json!(tools);
                return e.execute_resolved("integration.tools.publish", c, &fp);
            }
            if method == "skill.import_package" {
                let operation = e.prepare_integration_operation(method, &c, json!({"source":c.payload["source"],"git_url":c.payload["git_url"],"commit":c.payload["commit"],"package_hash":configuration_hash(&c.payload)}))?;
                let operation_id = text(&operation, "id")?;
                let result = (|| {
                    let mut package = if c.payload["git_url"].is_string() {
                        devdesk_adapters::integrations::import_skill_git(
                            Path::new(&e.store.root()),
                            &text(&c.payload, "git_url")?,
                            &text(&c.payload, "commit")?,
                            c.payload["directory"].as_str().unwrap_or(""),
                        )?
                    } else if let Some(encoded) = c.payload["zip_base64"].as_str() {
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
                    if c.payload["markdown"].is_string() {
                        if let Some(previous) = c.payload["source_revision_id"].as_str() {
                            if let Some(old) = e.store.snapshot()?.governance.iter().find(|r| {
                                r.r#type == "skill_package" && r.definition_id == previous
                            }) {
                                let original: SkillPackage = old.decode()?;
                                package.files = original.files;
                                package.scripts_disabled = original.scripts_disabled;
                                package.source = format!("edited:{}", original.source);
                            }
                        }
                    }
                    if let Some(dependencies) = c.payload.get("requires_tools") {
                        package.requires_tools = serde_json::from_value(dependencies.clone())
                            .map_err(|_| err("invalid_params", "无效工具依赖"))?;
                    }
                    c.payload["package"] = json!(package);
                    c.command_id = format!("{}:adopt", c.command_id);
                    e.execute_resolved(
                        "skill.import",
                        c.clone(),
                        &command_fingerprint("skill.import_package.adopt", &c),
                    )
                })();
                e.finish_integration_operation(&operation_id, &result)?;
                return result;
            }
            if method == "integration.discover" || method == "integration.test" {
                let state = e.store.snapshot()?;
                let key = text(&c.payload, "binding_ref")?;
                let record = state.configuration(&key)?;
                require(record.r#type == "integration_binding", "需要接入绑定修订")?;
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
                require(
                    !matches!(binding.auth, IntegrationAuth::OAuthPending { .. }),
                    "接入 OAuth 授权尚未完成",
                )?;
                let executor = NativeTools::new(e.store.root())?;
                if method == "integration.discover" {
                    let operation = e.prepare_integration_operation(method, &c, json!({"binding_ref":key,"binding_hash":record.content_hash,"source_hash":source.content_hash,"image_id":image,"permissions":permissions}))?;
                    let result = executor
                        .discover(&definition, &binding, &image, &permissions)
                        .and_then(|result| {
                            require(result["tools"].is_array(), "发现服务未返回有效工具目录")?;
                            e.record_tool_discovery(&c, &key, result)
                        });
                    e.finish_integration_operation(&text(&operation, "id")?, &result)?;
                    return result;
                }
                let tool_id = text(&c.payload, "tool_ref")?;
                let r = state.configuration(&tool_id)?;
                let tool: ExternalTool = r.decode()?;
                require(tool.integration_ref == source.id, "工具与接入绑定来源不符")?;
                let arguments = c.payload.get("arguments").cloned().unwrap_or(json!({}));
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
                    arguments: arguments.clone(),
                    permissions,
                    image_id: image,
                    worktree: String::new(),
                    input_hash: configuration_hash(&arguments),
                };
                let result = e.enqueue_tool_test(&c, &invocation)?;
                e.drive_tool_calls()?;
                return Ok(result);
            }
            if method == "integration.publish_tools" {
                let s = e.store.snapshot()?;
                let discovery = s.configuration(&text(&c.payload, "discovery_ref")?)?;
                require(discovery.r#type == "integration_discovery", "需要发现记录")?;
                let requested: Scope = serde_json::from_value(
                    c.payload
                        .get("scope")
                        .cloned()
                        .unwrap_or(json!(discovery.scope)),
                )
                .map_err(|_| err("invalid_params", "无效工具发布作用域"))?;
                require(
                    discovery.scope.visible_from(&requested),
                    "不能将项目账户发现的目录发布到更广作用域",
                )?;
                c.payload["scope"] = json!(requested);
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
                        &configuration_hash(&json!(
                            s.configuration(&binding.integration_ref)?.definition_id
                        ))[..8],
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
                        required_auth: None,
                    });
                }
                c.payload["tools"] = json!(tools);
                return e.execute_resolved("integration.tools.publish", c, &fp);
            }
            if method == "tool.call.reconcile" {
                e.recover_tool_receipts()?;
                if let Some(record) = e.store.snapshot()?.governance.iter().find(|r| {
                    Some(r.id.as_str()) == c.target.as_deref()
                        && r.r#type == "tool_call"
                        && r.data["status"] != "needs_reconciliation"
                }) {
                    return Ok(json!(record));
                }
            }
            if method == "integration.delete" {
                let state = e.store.snapshot()?;
                let source = state.configuration(c.target.as_deref().unwrap_or(""))?;
                let targets = devdesk_core::integration_deletion_targets(&state, source)?;
                let mut offset = 0;
                loop {
                    let (projects, total) = self.global.projects(offset, 100)?;
                    for project in &projects {
                        let snapshot = SqliteStore::read_project_snapshot(
                            Path::new(&self.global.root()),
                            &project.id,
                        )?;
                        for target in &targets {
                            require(
                                devdesk_core::project_configuration_references(&snapshot, target)
                                    .is_empty(),
                                format!("项目 {} 仍引用 {}，请先换绑", project.title, target.name),
                            )?;
                        }
                    }
                    offset += projects.len();
                    if offset >= total || projects.is_empty() {
                        break;
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_scoped_integrations_do_not_leak_and_global_creates_use_the_global_catalog() {
        let root = tempfile::tempdir().unwrap();
        let w =
            Workspace::open_with_adapters(root.path(), crate::test_runtime::adapters()).unwrap();
        let a = w
            .dispatch(
                "project.create",
                json!({"command_id":id(),"payload":{"title":"A"}}),
            )
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let b = w
            .dispatch(
                "project.create",
                json!({"command_id":id(),"payload":{"title":"B"}}),
            )
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        w.dispatch(
            "project.open",
            json!({"command_id":id(),"target":a,"payload":{}}),
        )
        .unwrap();
        let source=w.dispatch("integration.create",json!({"project_id":a,"local":true,"command_id":id(),"payload":{"name":"Project A service","scope":format!("project:{a}"),"data":{"source":{"kind":"http_api","base_url":"https://a.example.test"}}}})).unwrap();
        let visible = w
            .dispatch("query.integrations", json!({"project_id":a}))
            .unwrap();
        assert!(
            visible["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["id"] == source["id"])
        );
        w.dispatch(
            "project.open",
            json!({"command_id":id(),"target":b,"payload":{}}),
        )
        .unwrap();
        let hidden = w
            .dispatch("query.integrations", json!({"project_id":b}))
            .unwrap();
        assert!(
            !hidden["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["id"] == source["id"])
        );
        w.dispatch(
            "project.open",
            json!({"command_id":id(),"target":a,"payload":{}}),
        )
        .unwrap();
        let shared=w.dispatch("integration.create",json!({"project_id":a,"local":true,"command_id":id(),"payload":{"name":"Shared service","scope":"global","data":{"source":{"kind":"http_api","base_url":"https://shared.example.test"}}}})).unwrap();
        assert!(
            w.global
                .snapshot()
                .unwrap()
                .configuration(shared["id"].as_str().unwrap())
                .is_ok()
        );
        w.dispatch(
            "project.open",
            json!({"command_id":id(),"target":b,"payload":{}}),
        )
        .unwrap();
        let other = w
            .dispatch("query.integrations", json!({"project_id":b}))
            .unwrap();
        assert!(
            other["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["id"] == shared["id"])
        );
        assert!(
            w.dispatch(
                "integration.discover",
                json!({"project_id":b,"command_id":id(),"payload":{"binding_ref":source["id"]}})
            )
            .is_err()
        );
    }
}
