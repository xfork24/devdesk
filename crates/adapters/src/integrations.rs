//! Docker-isolated HTTP/MCP execution and offline import/validation adapters.
use super::*;
use devdesk_core::ports::ToolExecutor;
fn text(v: &Value, key: &str) -> Result<String> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| err("invalid_params", format!("缺少 {key}")))
}
use crate::docker::{ContainerPurpose, ContainerSpec, DockerSandbox};
use std::{collections::HashMap, sync::Arc, time::Duration};
type Job = Arc<Mutex<Option<Result<ToolReceipt>>>>;
pub struct NativeTools {
    root: PathBuf,
    jobs: Mutex<HashMap<String, Job>>,
}
fn operation(id: &str) -> String {
    let bytes = Sha256::digest(id.as_bytes());
    uuid::Uuid::from_slice(&bytes[..16]).unwrap().to_string()
}
fn validate_schema_tree(v: &Value, depth: usize) -> Result<()> {
    require(depth < 48, "Schema 嵌套过深")?;
    if let Some(map) = v.as_object() {
        for key in ["$ref", "$dynamicRef"] {
            if let Some(reference) = map.get(key) {
                require(
                    reference.as_str().is_some_and(|s| s.starts_with('#')),
                    "仅允许包内 Schema 引用",
                )?;
            }
        }
        if let Some(dialect) = map.get("$schema") {
            require(
                dialect.as_str().is_some_and(|s| {
                    [
                        "https://json-schema.org/draft/2020-12/schema",
                        "http://json-schema.org/draft-07/schema#",
                        "https://json-schema.org/draft-07/schema#",
                    ]
                    .contains(&s)
                }),
                "不支持的 JSON Schema 方言",
            )?;
        }
        // Only schema locations contain keywords. Defaults, examples and property names are data.
        for key in [
            "properties",
            "patternProperties",
            "$defs",
            "definitions",
            "dependentSchemas",
        ] {
            if let Some(children) = map.get(key).and_then(Value::as_object) {
                for child in children.values() {
                    validate_schema_tree(child, depth + 1)?;
                }
            }
        }
        for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
            if let Some(children) = map.get(key).and_then(Value::as_array) {
                for child in children {
                    validate_schema_tree(child, depth + 1)?;
                }
            }
        }
        for key in [
            "items",
            "additionalItems",
            "additionalProperties",
            "unevaluatedProperties",
            "unevaluatedItems",
            "propertyNames",
            "contains",
            "not",
            "if",
            "then",
            "else",
            "contentSchema",
        ] {
            if let Some(child) = map.get(key) {
                if let Some(children) = child.as_array() {
                    for child in children {
                        validate_schema_tree(child, depth + 1)?;
                    }
                } else {
                    validate_schema_tree(child, depth + 1)?;
                }
            }
        }
        if let Some(deps) = map.get("dependencies").and_then(Value::as_object) {
            for child in deps.values().filter(|v| v.is_object() || v.is_boolean()) {
                validate_schema_tree(child, depth + 1)?;
            }
        }
    }
    Ok(())
}
fn schema_budget<'a>(
    root: &'a Value,
    node: &'a Value,
    depth: usize,
    remaining: &mut usize,
    stack: &mut Vec<*const Value>,
) -> Result<()> {
    require(depth < 48 && *remaining > 0, "Schema 展开复杂度超过边界")?;
    *remaining -= 1;
    let pointer = node as *const Value;
    require(
        !stack.contains(&pointer),
        "Schema 递归引用不属于当前有界校验原语",
    )?;
    stack.push(pointer);
    if let Some(map) = node.as_object() {
        for key in ["$ref", "$dynamicRef"] {
            if let Some(reference) = map.get(key).and_then(Value::as_str) {
                let target = root
                    .pointer(
                        reference
                            .strip_prefix('#')
                            .ok_or_else(|| err("schema_invalid", "仅允许本地引用"))?,
                    )
                    .ok_or_else(|| err("schema_invalid", "Schema 引用不存在"))?;
                schema_budget(root, target, depth + 1, remaining, stack)?;
            }
        }
        for key in [
            "properties",
            "patternProperties",
            "$defs",
            "definitions",
            "dependentSchemas",
        ] {
            if let Some(children) = map.get(key).and_then(Value::as_object) {
                for child in children.values() {
                    schema_budget(root, child, depth + 1, remaining, stack)?;
                }
            }
        }
        for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
            if let Some(children) = map.get(key).and_then(Value::as_array) {
                for child in children {
                    schema_budget(root, child, depth + 1, remaining, stack)?;
                }
            }
        }
        for key in [
            "items",
            "additionalItems",
            "additionalProperties",
            "unevaluatedProperties",
            "unevaluatedItems",
            "propertyNames",
            "contains",
            "not",
            "if",
            "then",
            "else",
            "contentSchema",
        ] {
            if let Some(child) = map.get(key) {
                if let Some(children) = child.as_array() {
                    for child in children {
                        schema_budget(root, child, depth + 1, remaining, stack)?;
                    }
                } else {
                    schema_budget(root, child, depth + 1, remaining, stack)?;
                }
            }
        }
    }
    stack.pop();
    Ok(())
}
fn data_budget(value: &Value, depth: usize, remaining: &mut usize) -> Result<()> {
    require(
        depth <= 64 && *remaining > 0,
        "校验内容嵌套或节点数量超过边界",
    )?;
    *remaining -= 1;
    match value {
        Value::Object(map) => {
            for child in map.values() {
                data_budget(child, depth + 1, remaining)?
            }
        }
        Value::Array(values) => {
            for child in values {
                data_budget(child, depth + 1, remaining)?
            }
        }
        _ => {}
    }
    Ok(())
}
fn validator(schema: &Value) -> Result<jsonschema::Validator> {
    require(
        serde_json::to_vec(schema).map_err(storage)?.len() <= 131072,
        "Schema 太大",
    )?;
    validate_schema_tree(schema, 0)?;
    schema_budget(schema, schema, 0, &mut 4096, &mut vec![])?;
    jsonschema::options()
        .with_pattern_options(
            jsonschema::PatternOptions::regex()
                .size_limit(1024 * 1024)
                .dfa_size_limit(2 * 1024 * 1024),
        )
        .build(schema)
        .map_err(|e| {
            err(
                "schema_invalid",
                format!("Schema 不合法: {}", e.instance_path()),
            )
        })
}
impl NativeTools {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            root: fs::canonicalize(root).map_err(storage)?,
            jobs: Mutex::new(HashMap::new()),
        })
    }
    fn directory(&self, id: &str) -> PathBuf {
        self.root.join("tool-executions").join(operation(id))
    }
    fn revoked(&self, tool: &BoundTool) -> Result<bool> {
        let root = self
            .root
            .ancestors()
            .find(|p| p.join("global.sqlite3").is_file())
            .unwrap_or(&self.root);
        let mut disabled: Vec<ConfigurationRecord> = Vec::new();
        for db in [
            root.join("global.sqlite3"),
            self.root.join("engineering.sqlite3"),
        ] {
            if !db.is_file() {
                continue;
            }
            let c = Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(storage)?;
            disabled.extend(
                load_table::<Value>(&c, "governance_records")?
                    .into_iter()
                    .map(ConfigurationRecord::from_storage)
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        let refs = [
            &tool.revision_id,
            &tool.definition.integration_ref,
            &tool.binding_ref,
        ];
        let identities: Vec<_> = disabled
            .iter()
            .filter(|r| refs.contains(&&r.id))
            .map(|r| r.definition_id.as_str())
            .collect();
        Ok(disabled.iter().any(|r| {
            r.r#type == "integration_disabled" && identities.contains(&r.definition_id.as_str())
        }))
    }
    fn secret(
        &self,
        b: &IntegrationBinding,
        source: &IntegrationDefinition,
        image: &str,
        permissions: &ExecutionPermissions,
    ) -> Result<Option<String>> {
        let Some(key) = b.auth.credential_ref() else {
            return Ok(None);
        };
        let root = self
            .root
            .ancestors()
            .find(|p| p.join("global.sqlite3").is_file())
            .unwrap_or(&self.root);
        let vault = crate::credentials::CredentialVault::new(root)?;
        if let IntegrationAuth::OAuth {
            account, scopes, ..
        } = &b.auth
        {
            let config = source
                .oauth
                .as_ref()
                .ok_or_else(|| err("oauth_configuration", "接入源未配置 OAuth"))?;
            return crate::integration_oauth::access_token(
                key,
                config,
                account,
                scopes,
                &vault,
                |endpoint, form| self.oauth_request(endpoint, form, b, image, permissions),
            )
            .map(Some);
        }
        vault.read_integration(key).map(Some)
    }
    fn run(
        &self,
        id: &str,
        source: &IntegrationDefinition,
        binding: &IntegrationBinding,
        image: &str,
        permissions: &ExecutionPermissions,
        worktree: &str,
        action: &str,
        tool: Option<&ExternalTool>,
        arguments: Value,
    ) -> Result<Value> {
        require(
            image.starts_with("sha256:") && image.len() == 71,
            "Tool Executor 需要确切 Docker 镜像身份",
        )?;
        require(
            !matches!(
                permissions.harness.policy.filesystem,
                FilesystemPermission::Scoped { .. }
            ),
            "当前工具执行器无法完整执行自定义 Harness 文件规则，请选用完整或只读 profile",
        )?;
        let dir = self.directory(id);
        fs::create_dir_all(&dir).map_err(storage)?;
        let state = dir.join("state");
        let empty = dir.join("work");
        fs::create_dir_all(&state).map_err(storage)?;
        fs::create_dir_all(&empty).map_err(storage)?;
        fs::write(
            state.join("tool-worker.mjs"),
            include_str!("../../../resources/integrations/tool-worker.mjs"),
        )
        .map_err(storage)?;
        let work = if worktree.is_empty() {
            empty
        } else {
            let p = fs::canonicalize(worktree).map_err(storage)?;
            require(
                p.starts_with(&self.root) && p != self.root,
                "工具 worktree 不在当前执行根下",
            )?;
            p
        };
        let mut p = permissions.clone();
        // The service may not broaden the effective tool network by running outside Codex.
        if p.harness.policy.network == NetworkPermission::Disabled {
            p.sandbox.policy.network = NetworkPermission::Disabled;
        }
        let network = p.sandbox.policy.network == NetworkPermission::Enabled;
        if !matches!(&source.source,IntegrationSource::Mcp{transport,..}if transport=="stdio") {
            require(network, "Permission 禁止远程工具网络")?;
        }
        let spec = ContainerSpec {
            host_gateway: binding.allow_private_network
                && binding.allowed_origins.iter().any(|s| {
                    s.starts_with("http://host.docker.internal:")
                        || s.starts_with("https://host.docker.internal:")
                }),
            permissions: Some(p.clone()),
            purpose: ContainerPurpose::Harness,
            project_id: operation(&format!("{id}:project")),
            session_id: operation(&format!("{id}:session")),
            operation_id: operation(id),
            image_id: image.into(),
            worktree: work,
            state,
            writable: p.harness.policy.filesystem != FilesystemPermission::ReadOnly
                && p.sandbox.policy.filesystem != FilesystemPermission::ReadOnly
                && !worktree.is_empty()
                && p.scope.writable_paths.contains(&".".into()),
            writable_subdirectories: p
                .scope
                .writable_paths
                .iter()
                .filter(|p| p.as_str() != ".")
                .cloned()
                .collect(),
            read_only_subdirectories: p.scope.read_only_paths.clone(),
            network,
            command: vec!["node".into(), "/state/tool-worker.mjs".into(), "-".into()],
            nested_sandbox: p.isolation.nested_sandbox,
            credential_env_file: None,
            profile_file: None,
            runner_file: None,
            skill_directory: None,
            sensitive_output: action == "oauth",
            profile_target: None,
        };
        let docker = DockerSandbox::new(&self.root)?;
        let request = json!({"invocation_id":id,"action":action,"source":source.source,"binding":binding,"tool":tool,"arguments":arguments,"secret":if action=="oauth"{None}else{self.secret(binding,source,image,permissions)?}});
        docker.create(&spec)?;
        let output = crate::process::capture_command_limited(
            docker.attach_command(&spec.operation_id)?,
            Duration::from_millis(binding.timeout_ms + 5000),
            Some(serde_json::to_vec(&request).map_err(storage)?),
            binding
                .max_result_bytes
                .saturating_mul(8)
                .saturating_add(65536),
        );
        let _ = docker.stop(&spec.operation_id);
        let value = output.and_then(|r| {
            require(!r.truncated, "工具输出超限")?;
            serde_json::from_slice::<Value>(&r.stdout)
                .map_err(|_| err("tool_transport", "工具进程未返回完整结果"))
        });
        let _ = docker.remove(&spec.operation_id);
        value
    }
    pub fn oauth_request(
        &self,
        endpoint: &str,
        form: Value,
        binding: &IntegrationBinding,
        image: &str,
        permissions: &ExecutionPermissions,
    ) -> Result<Value> {
        require(
            !endpoint.contains(['@', '#', '\n', '\r']),
            "无效 OAuth endpoint",
        )?;
        let (scheme, rest) = endpoint
            .split_once("://")
            .ok_or_else(|| err("oauth_endpoint", "OAuth endpoint 缺少 scheme"))?;
        require(["https", "http"].contains(&scheme), "OAuth 只支持 HTTP(S)")?;
        let origin = format!("{scheme}://{}", rest.split('/').next().unwrap_or(""));
        let mut b = binding.clone();
        b.auth = IntegrationAuth::None;
        b.allowed_origins = vec![origin.clone()];
        let source = IntegrationDefinition {
            source: IntegrationSource::HttpApi { base_url: origin },
            description: "OAuth exchange".into(),
            oauth: None,
        };
        let result = self.run(
            &id(),
            &source,
            &b,
            image,
            permissions,
            "",
            "oauth",
            None,
            json!({"endpoint":endpoint,"form":form}),
        )?;
        require(
            result.get("success") != Some(&json!(false)),
            "OAuth 协议交换失败",
        )?;
        Ok(result)
    }
    pub fn read_artifact(&self, id: &str, receipt: &ToolReceipt) -> Result<(Vec<u8>, String)> {
        require(receipt.invocation_id == id, "文件回执不属于该调用")?;
        let link = receipt
            .content
            .iter()
            .find(|c| {
                c["type"] == "resource_link" && c["uri"] == format!("devdesk-tool-artifact:{id}")
            })
            .ok_or_else(|| err("not_found", "调用没有文件结果"))?;
        let bytes = fs::read(self.directory(id).join("result.bin")).map_err(storage)?;
        require(
            link["sha256"] == format!("{:x}", Sha256::digest(&bytes))
                && link["size"].as_u64() == Some(bytes.len() as u64),
            "文件结果摘要不符",
        )?;
        Ok((
            bytes,
            link["mimeType"]
                .as_str()
                .unwrap_or("application/octet-stream")
                .into(),
        ))
    }
    fn execute(&self, i: &ToolInvocation) -> Result<ToolReceipt> {
        require(!self.revoked(&i.tool)?, "接入已紧急停用")?;
        let value = self.run(
            &i.id,
            &i.tool.integration,
            &i.tool.binding,
            &i.image_id,
            &i.permissions,
            &i.worktree,
            "call",
            Some(&i.tool.definition),
            i.arguments.clone(),
        )?;
        let mut receipt: ToolReceipt =
            serde_json::from_value(value).map_err(|_| err("tool_protocol", "工具回执格式不符"))?;
        if receipt.success {
            if let Some(schema) = &i.tool.definition.output_schema {
                let result = receipt
                    .structured_content
                    .as_ref()
                    .ok_or_else(|| err("output_schema", "输出契约需要结构化结果"));
                if result
                    .and_then(|v| self.validate_arguments(schema, v))
                    .is_err()
                {
                    receipt.success = false;
                    receipt.error = Some(
                        json!({"code":"output_schema","message":"远端动作已返回，但结果不符合声明契约"}),
                    );
                    receipt.content = vec![
                        json!({"type":"text","text":"Tool output failed schema validation. Do not automatically retry a side effect."}),
                    ];
                }
            }
        }
        for content in &mut receipt.content {
            if content["type"] == "artifact" {
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(content["base64"].as_str().unwrap_or(""))
                    .map_err(|_| err("tool_protocol", "文件结果编码无效"))?;
                require(
                    bytes.len() <= i.tool.binding.max_result_bytes,
                    "文件结果过大",
                )?;
                let directory = self.directory(&i.id);
                let path = directory.join("result.bin");
                fs::write(&path, &bytes).map_err(storage)?;
                *content = json!({"type":"resource_link","uri":format!("devdesk-tool-artifact:{}",i.id),"mimeType":content["mime"],"sha256":format!("{:x}",Sha256::digest(&bytes)),"size":bytes.len()});
            }
        }
        Ok(receipt)
    }
}
impl ports::ToolExecutor for NativeTools {
    fn validate_schema(&self, schema: &Value) -> Result<()> {
        validator(schema).map(|_| ())
    }
    fn validate_arguments(&self, schema: &Value, value: &Value) -> Result<()> {
        require(
            serde_json::to_vec(value).map_err(storage)?.len() <= SPEC_VALUE_LIMIT,
            "校验内容超过 8 MiB",
        )?;
        data_budget(value, 0, &mut 100000)?;
        validator(schema)?.validate(value).map_err(|e| {
            err(
                "schema_validation",
                format!("数据不符合契约: {}", e.instance_path()),
            )
        })
    }
    fn start(&self, i: &ToolInvocation) -> Result<()> {
        require(
            i.tool.binding.allowed_tools.contains(&i.tool.revision_id),
            "工具修订不在绑定授权中",
        )?;
        require(
            i.tool
                .definition
                .required_auth
                .as_ref()
                .is_none_or(|a| a.satisfied_by(&i.tool.binding.auth)),
            "绑定认证不满足工具方案",
        )?;
        require(
            i.input_hash == configuration_hash(&i.arguments),
            "调用输入摘要不符",
        )?;
        self.validate_arguments(&i.tool.definition.input_schema, &i.arguments)?;
        require(!self.revoked(&i.tool)?, "接入已紧急停用")?;
        let mut jobs = self.jobs.lock().map_err(storage)?;
        require(
            jobs.contains_key(&i.id)
                || jobs
                    .values()
                    .filter(|j| j.lock().is_ok_and(|v| v.is_none()))
                    .count()
                    < 4,
            "工具并发已满",
        )?;
        let directory = self.directory(&i.id);
        fs::create_dir_all(&directory).map_err(storage)?;
        let identity = directory.join("input-hash");
        let fingerprint = configuration_hash(&json!([
            i.input_hash,
            i.tool,
            i.permissions,
            i.image_id,
            i.worktree
        ]));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&identity)
        {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(fingerprint.as_bytes()).map_err(storage)?;
                file.sync_all().map_err(storage)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                require(
                    fs::read_to_string(&identity).map_err(storage)? == fingerprint,
                    "调用 ID 被不同的输入或授权复用",
                )?;
                if jobs.contains_key(&i.id) || directory.join("receipt.json").is_file() {
                    return Ok(());
                }
                return Err(err(
                    "needs_reconciliation",
                    "已存在执行意图且无回执，禁止重新派发",
                ));
            }
            Err(e) => return Err(storage(e)),
        }
        let job = Arc::new(Mutex::new(None));
        jobs.insert(i.id.clone(), job.clone());
        let root = self.root.clone();
        let invocation = i.clone();
        std::thread::spawn(move || {
            let result = NativeTools::new(&root).and_then(|e| e.execute(&invocation));
            let receipt = result.unwrap_or_else(|e| ToolReceipt {
                invocation_id: invocation.id.clone(),
                success: false,
                content: vec![json!({"type":"text","text":e.message})],
                structured_content: None,
                remote_effect: "unknown".into(),
                error: Some(json!({"code":e.code})),
                origin: Origin::External,
            });
            let dir = root.join("tool-executions").join(operation(&invocation.id));
            let _ = fs::create_dir_all(&dir);
            if let Ok(bytes) = serde_json::to_vec(&receipt) {
                let temporary = dir.join("receipt.tmp");
                if fs::write(&temporary, bytes).is_ok() {
                    let _ = fs::rename(temporary, dir.join("receipt.json"));
                }
            }
            if let Ok(mut slot) = job.lock() {
                *slot = Some(Ok(receipt));
            }
        });
        Ok(())
    }
    fn poll(&self, i: &ToolInvocation) -> Result<Option<ToolReceipt>> {
        if let Some(job) = self.jobs.lock().map_err(storage)?.get(&i.id) {
            let result = job.lock().map_err(storage)?.clone().transpose()?;
            if result.is_some() {
                return Ok(result);
            }
            if self.revoked(&i.tool)? {
                self.cancel(i)?;
                return Err(err("integration_disabled", "接入已停用，调用需核对"));
            }
            return Ok(None);
        }
        let path = self.directory(&i.id).join("receipt.json");
        if path.is_file() {
            return serde_json::from_slice(&fs::read(path).map_err(storage)?)
                .map(Some)
                .map_err(storage);
        }
        Err(err(
            "needs_reconciliation",
            "执行器未持有该调用且没有持久回执，禁止重新派发",
        ))
    }
    fn cancel(&self, i: &ToolInvocation) -> Result<()> {
        DockerSandbox::new(&self.root)?.stop(&operation(&i.id))
    }
    fn discover(
        &self,
        source: &IntegrationDefinition,
        binding: &IntegrationBinding,
        image: &str,
        permissions: &ExecutionPermissions,
    ) -> Result<Value> {
        self.run(
            &id(),
            source,
            binding,
            image,
            permissions,
            "",
            "discover",
            None,
            json!({}),
        )
    }
}
impl NativeTools {
    pub fn discover_with_permissions(
        &self,
        source: &IntegrationDefinition,
        binding: &IntegrationBinding,
        image: &str,
        permissions: &ExecutionPermissions,
    ) -> Result<Value> {
        self.run(
            &id(),
            source,
            binding,
            image,
            permissions,
            "",
            "discover",
            None,
            json!({}),
        )
    }
}

/// Import supported OpenAPI 3.1 operations, without loading any external reference.
pub fn import_openapi(
    value: &Value,
    integration: &str,
    selected: &[String],
) -> Result<Vec<ExternalTool>> {
    import_openapi_with_auth(value, integration, selected, None)
}
pub fn import_openapi_with_auth(
    value: &Value,
    integration: &str,
    selected: &[String],
    security_scheme: Option<&str>,
) -> Result<Vec<ExternalTool>> {
    require(
        value["openapi"]
            .as_str()
            .is_some_and(|v| v.starts_with("3.1.")),
        "仅支持 OpenAPI 3.1.x",
    )?;
    let paths = value["paths"]
        .as_object()
        .ok_or_else(|| err("openapi_invalid", "缺少 paths"))?;
    let mut result = vec![];
    let schemas = value
        .pointer("/components/schemas")
        .cloned()
        .unwrap_or(json!({}));
    fn rewrite(v: &mut Value) {
        match v {
            Value::Object(m) => {
                if let Some(Value::String(r)) = m.get_mut("$ref") {
                    if r.starts_with("#/components/schemas/") {
                        *r = r.replacen("#/components/schemas/", "#/$defs/", 1);
                    }
                }
                for value in m.values_mut() {
                    rewrite(value);
                }
            }
            Value::Array(a) => {
                for value in a {
                    rewrite(value)
                }
            }
            _ => {}
        }
    }
    for (path, item) in paths {
        for method in ["get", "post", "put", "patch", "delete", "head"] {
            let Some(op) = item.get(method) else { continue };
            let fallback = format!("{}_{}", method, &configuration_hash(&json!(path))[..12]);
            let name = op["operationId"].as_str().unwrap_or(&fallback);
            if !selected.is_empty() && !selected.contains(&name.to_owned()) {
                continue;
            }
            require(
                op.get("servers").is_none() && item.get("servers").is_none(),
                "operation/path servers 需要独立接入源，不能忽略覆盖地址",
            )?;
            let security = op.get("security").or(value.get("security"));
            let mut alternatives = vec![];
            if let Some(requirements) = security {
                for requirement in requirements
                    .as_array()
                    .ok_or_else(|| err("openapi_security", "security 必须为数组"))?
                {
                    let scheme = requirement
                        .as_object()
                        .ok_or_else(|| err("openapi_security", "无效 Security Requirement"))?;
                    if let Some(selected) = security_scheme {
                        if !(scheme.contains_key(selected)
                            || (scheme.is_empty() && selected == "none"))
                        {
                            continue;
                        }
                    }
                    require(
                        scheme.len() <= 1,
                        "首版不支持 Security Requirement 的多方案 AND 组合",
                    )?;
                    if scheme.is_empty() {
                        alternatives.push(("none".to_owned(), ToolAuthRequirement::None));
                        continue;
                    }
                    let (name, scopes) = scheme.iter().next().unwrap();
                    let def = &value["components"]["securitySchemes"][name];
                    let auth = match (def["type"].as_str(), def["scheme"].as_str()) {
                        (Some("http"), Some("bearer")) => ToolAuthRequirement::Bearer,
                        (Some("apiKey"), _) if def["in"] == "header" => {
                            ToolAuthRequirement::ApiKey {
                                header: text(def, "name")?,
                            }
                        }
                        (Some("oauth2"), _)
                            if def.pointer("/flows/authorizationCode").is_some() =>
                        {
                            ToolAuthRequirement::OAuth {
                                scopes: serde_json::from_value(scopes.clone())
                                    .map_err(|_| err("openapi_security", "无效 OAuth scopes"))?,
                            }
                        }
                        _ => {
                            return Err(err(
                                "openapi_security",
                                format!(
                                    "认证方案 {name} 尚不支持；需要 Bearer、Header API key 或授权码 OAuth"
                                ),
                            ));
                        }
                    };
                    alternatives.push((name.clone(), auth));
                }
            }
            if security_scheme.is_some()
                && security
                    .and_then(Value::as_array)
                    .is_some_and(|a| !a.is_empty())
            {
                require(
                    !alternatives.is_empty(),
                    "所选认证方案不满足 operation 的 OR 组合",
                )?;
            }
            let required_auth = if alternatives.is_empty() {
                Some(ToolAuthRequirement::None)
            } else if let Some(selected) = security_scheme {
                Some(
                    alternatives
                        .iter()
                        .find(|(name, _)| name == selected)
                        .ok_or_else(|| {
                            err(
                                "openapi_security",
                                "所选认证方案不满足此 operation 的 OR 组合",
                            )
                        })?
                        .1
                        .clone(),
                )
            } else {
                require(
                    alternatives.len() == 1,
                    "operation 存在替代认证方案，请明确选择 security_scheme",
                )?;
                Some(alternatives.remove(0).1)
            };
            require(
                !item.get("$ref").is_some(),
                "path 外部引用需要导入时显式展开",
            )?;
            let mut properties = serde_json::Map::new();
            let mut required = vec![];
            let mut parameters = vec![];
            for p in item["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(op["parameters"].as_array().into_iter().flatten())
            {
                require(
                    p.get("$ref").is_none()
                        && p.get("style")
                            .is_none_or(|s| ["simple", "form"].contains(&s.as_str().unwrap_or("")))
                        && p.get("content").is_none(),
                    "不支持的参数引用或序列化方式",
                )?;
                let key = text(p, "name")?;
                let location = text(p, "in")?;
                require(
                    ["path", "query", "header"].contains(&location.as_str()),
                    "不支持 cookie 参数",
                )?;
                let schema = p.get("schema").cloned().unwrap_or(json!({"type":"string"}));
                require(schema["type"] != "object", "对象参数需要显式受支持映射")?;
                require(
                    schema["type"] != "array"
                        || (location == "query"
                            && p.get("explode").is_none_or(|v| v == true)
                            && p.get("style").is_none_or(|v| v == "form")),
                    "数组仅支持 query form explode=true",
                )?;
                require(!properties.contains_key(&key), "参数同名，需要显式改名映射")?;
                properties.insert(key.clone(), schema);
                let needed = p["required"] == true || location == "path";
                if needed {
                    required.push(key.clone());
                }
                parameters.push(HttpParameter {
                    pointer: format!("/{}", key.replace('~', "~0").replace('/', "~1")),
                    name: key,
                    location,
                    required: needed,
                });
            }
            let body = op
                .pointer("/requestBody/content/application~1json/schema")
                .cloned();
            if op.get("requestBody").is_some() {
                require(body.is_some(), "只支持 JSON requestBody")?;
            }
            if let Some(schema) = body {
                properties.insert("body".into(), schema);
                if op["requestBody"]["required"] == true {
                    required.push("body".into());
                }
            }
            let mut input = json!({"type":"object","properties":properties,"required":required,"additionalProperties":false,"$defs":schemas});
            rewrite(&mut input);
            validator(&input)?;
            let mut outputs = vec![];
            if let Some(responses) = op["responses"].as_object() {
                for (status, r) in responses {
                    if status.starts_with('2') {
                        if let Some(schema) = r.pointer("/content/application~1json/schema") {
                            let mut schema = schema.clone();
                            if let Some(m) = schema.as_object_mut() {
                                m.insert("$defs".into(), schemas.clone());
                            }
                            rewrite(&mut schema);
                            validator(&schema)?;
                            outputs.push(schema);
                        }
                    }
                }
            }
            let output = match outputs.len() {
                0 => None,
                1 => outputs.pop(),
                _ => Some(json!({"anyOf":outputs,"$defs":schemas})),
            };
            let tool = ExternalTool {
                schema_version: 2,
                qualified_name: name.into(),
                description: op["description"]
                    .as_str()
                    .or(op["summary"].as_str())
                    .unwrap_or(name)
                    .into(),
                integration_ref: integration.into(),
                implementation: ToolImplementation::HttpApi {
                    method: method.to_uppercase(),
                    path: path.clone(),
                    parameters,
                    body_pointer: op.get("requestBody").map(|_| "/body".into()),
                    idempotency_header: None,
                },
                input_schema: input,
                output_schema: output,
                effect: if ["get", "head"].contains(&method) {
                    "read_only"
                } else {
                    "unknown"
                }
                .into(),
                constraints: Default::default(),
                sensitive_parameters: vec![],
                required_auth,
            };
            tool.validate()?;
            result.push(tool);
        }
    }
    require(!result.is_empty(), "没有选中的可导入 operation")?;
    Ok(result)
}

pub fn import_skill_zip(bytes: &[u8]) -> Result<Vec<SkillPackage>> {
    use std::io::Read;
    require(bytes.len() <= 2_097_152, "Skill ZIP 太大")?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|_| err("skill_package", "无效 ZIP"))?;
    require(archive.len() <= 128, "包内文件过多")?;
    let mut files = vec![];
    let mut total = 0;
    let mut body = None;
    let mut scripts = false;
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(storage)?;
        if file.is_dir() {
            continue;
        }
        require(
            file.enclosed_name().is_some()
                && file
                    .unix_mode()
                    .is_none_or(|mode| mode & 0o170000 != 0o120000),
            "包内路径或链接不合法",
        )?;
        let name = file.name().to_owned();
        require(
            !name.contains('\\') && file.size() <= 1_048_576,
            "包文件不合法或过大",
        )?;
        let mut text = String::new();
        file.take(1_048_577)
            .read_to_string(&mut text)
            .map_err(|_| err("skill_package", "首版 Skill 材料只支持 UTF-8 文本"))?;
        total += text.len();
        require(total <= 2_097_152, "包解压后太大")?;
        if name.eq_ignore_ascii_case("SKILL.md") {
            require(body.is_none(), "包必须只有一个 SKILL.md")?;
            body = Some(text);
        } else if name.to_ascii_lowercase().starts_with("scripts/")
            || [
                "sh", "py", "js", "mjs", "cjs", "ts", "ps1", "bat", "cmd", "exe",
            ]
            .contains(
                &name
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str(),
            )
        {
            scripts = true;
        } else {
            files.push(SkillFile {
                path: name,
                content: text,
            });
        }
    }
    let body = body.ok_or_else(|| err("skill_package", "包缺少根目录 SKILL.md"))?;
    let (description, _) = parse_skill_markdown(&body)?;
    let package = SkillPackage {
        body,
        description,
        files,
        requires_tools: vec![],
        source: format!("zip:sha256:{:x}", Sha256::digest(bytes)),
        scripts_disabled: scripts,
    };
    package.validate()?;
    Ok(vec![package])
}
pub fn parse_skill_markdown(text: &str) -> Result<(String, String)> {
    if let Some(rest) = text.strip_prefix("---\n") {
        let (front, body) = rest
            .split_once("\n---\n")
            .ok_or_else(|| err("skill_manifest", "未闭合 Skill front matter"))?;
        let data: Value = serde_yaml::from_str(front)
            .map_err(|_| err("skill_manifest", "无效 Skill front matter"))?;
        require(
            data["name"].is_string() && data["description"].is_string(),
            "SKILL.md 需要 name 和 description",
        )?;
        Ok((data["description"].as_str().unwrap().into(), body.into()))
    } else {
        Ok(("Imported Markdown skill".into(), text.into()))
    }
}

/// Public HTTPS Git import at an exact commit, without checking out or executing repository code.
pub fn import_skill_git(
    root: &Path,
    url: &str,
    commit: &str,
    directory: &str,
) -> Result<SkillPackage> {
    require(
        url.starts_with("https://")
            && !url.contains(['\n', '\r', '@'])
            && commit.len() == 40
            && commit.bytes().all(|c| c.is_ascii_hexdigit()),
        "Git Skill 需要公开 HTTPS 来源和确切 40 位 commit",
    )?;
    require(
        !directory.starts_with('/') && directory.split('/').all(|p| p != ".." && p != "."),
        "Skill 子目录无效",
    )?;
    let path = root.join("skill-imports").join(id());
    fs::create_dir_all(&path).map_err(storage)?;
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(path.clone());
    let git = |args: Vec<String>| -> Result<Vec<u8>> {
        let mut cmd = std::process::Command::new("git");
        cmd.current_dir(&path)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ASKPASS", "/usr/bin/false")
            .env("SSH_ASKPASS", "/usr/bin/false")
            .env("GIT_CONFIG_COUNT", "0")
            .env_remove("GIT_CONFIG_PARAMETERS")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args([
                "-c",
                "credential.helper=",
                "-c",
                "http.followRedirects=false",
                "-c",
                "protocol.file.allow=never",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args);
        let out = crate::process::capture_command(cmd, Duration::from_secs(30), None)?;
        require(
            out.code == 0 && !out.truncated,
            "Git Skill 导入失败或输出超限",
        )?;
        Ok(out.stdout)
    };
    git(vec!["init".into(), "--quiet".into()])?;
    git(vec![
        "fetch".into(),
        "--quiet".into(),
        "--depth=1".into(),
        "--filter=blob:none".into(),
        url.into(),
        commit.into(),
    ])?;
    let actual =
        String::from_utf8(git(vec!["rev-parse".into(), "FETCH_HEAD".into()])?).map_err(storage)?;
    require(
        actual.trim().eq_ignore_ascii_case(commit),
        "Git 返回的 commit 与指定来源不同",
    )?;
    let listing = git(vec![
        "ls-tree".into(),
        "-r".into(),
        "-z".into(),
        commit.into(),
        "--".into(),
        if directory.is_empty() {
            ".".into()
        } else {
            directory.into()
        },
    ])?;
    let mut body = None;
    let mut files = vec![];
    let mut total = 0;
    let mut scripts = false;
    for entry in listing.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let text = std::str::from_utf8(entry).map_err(storage)?;
        let (meta, name) = text
            .split_once('\t')
            .ok_or_else(|| err("skill_git", "无效文件目录"))?;
        let parts: Vec<_> = meta.split_whitespace().collect();
        require(
            parts.len() == 3 && parts[1] == "blob" && ["100644", "100755"].contains(&parts[0]),
            "Skill 不接受符号链接或子模块",
        )?;
        let relative = if directory.is_empty() {
            name
        } else {
            name.strip_prefix(&format!("{}/", directory.trim_end_matches('/')))
                .ok_or_else(|| err("skill_git", "文件不在所选目录"))?
        };
        if relative.to_ascii_lowercase().starts_with("scripts/")
            || parts[0] == "100755"
            || [
                "sh", "py", "js", "mjs", "cjs", "ts", "ps1", "bat", "cmd", "exe",
            ]
            .contains(
                &relative
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str(),
            )
        {
            scripts = true;
            continue;
        }
        let size = String::from_utf8(git(vec!["cat-file".into(), "-s".into(), parts[2].into()])?)
            .map_err(storage)?
            .trim()
            .parse::<usize>()
            .map_err(storage)?;
        total += size;
        require(
            size <= 1_048_576 && total <= 2_097_152 && files.len() < 128,
            "Skill 文件或材料总量超限",
        )?;
        let content = String::from_utf8(git(vec![
            "cat-file".into(),
            "blob".into(),
            parts[2].into(),
        ])?)
        .map_err(|_| err("skill_git", "Skill 材料需要 UTF-8 文本"))?;
        if relative.eq_ignore_ascii_case("SKILL.md") {
            require(body.is_none(), "Skill 主文档重复")?;
            body = Some(content);
        } else {
            files.push(SkillFile {
                path: relative.into(),
                content,
            });
        }
    }
    let body = body.ok_or_else(|| err("skill_git", "所选目录缺少 SKILL.md"))?;
    let (description, _) = parse_skill_markdown(&body)?;
    let package = SkillPackage {
        body,
        description,
        files,
        requires_tools: vec![],
        source: format!("git:{url}@{}:{directory}", commit.to_lowercase()),
        scripts_disabled: scripts,
    };
    package.validate()?;
    Ok(package)
}

pub fn parse_openapi_document(text: &str) -> Result<Value> {
    serde_yaml::from_str(text).map_err(|_| err("openapi_invalid", "无法解析 OpenAPI JSON/YAML"))
}
