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
    match v {
        Value::Object(map) => {
            for (key, value) in map {
                if ["$ref", "$dynamicRef"].contains(&key.as_str()) {
                    require(
                        value.as_str().is_some_and(|s| s.starts_with('#')),
                        "仅允许包内 Schema 引用",
                    )?;
                }
                if key == "$schema" {
                    require(
                        value.as_str().is_some_and(|s| {
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
                validate_schema_tree(value, depth + 1)?;
            }
        }
        Value::Array(a) => {
            for value in a {
                validate_schema_tree(value, depth + 1)?;
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
    jsonschema::validator_for(schema).map_err(|e| {
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
        let db = if root.join("global.sqlite3").is_file() {
            root.join("global.sqlite3")
        } else {
            root.join("engineering.sqlite3")
        };
        if !db.is_file() {
            return Ok(false);
        }
        let c = Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(storage)?;
        let disabled: Vec<ConfigurationRecord> = load_table(&c, "governance_records")?;
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
            r.kind == "integration_disabled" && identities.contains(&r.definition_id.as_str())
        }))
    }
    fn secret(&self, b: &IntegrationBinding) -> Result<Option<String>> {
        let Some(key) = b.auth.credential_ref() else {
            return Ok(None);
        };
        let root = self
            .root
            .ancestors()
            .find(|p| p.join("global.sqlite3").is_file())
            .unwrap_or(&self.root);
        crate::credentials::CredentialVault::new(root)?
            .read_integration(key)
            .map(Some)
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
            profile_target: None,
        };
        let docker = DockerSandbox::new(&self.root)?;
        docker.create(&spec)?;
        let request = json!({"invocation_id":id,"action":action,"source":source.source,"binding":binding,"tool":tool,"arguments":arguments,"secret":self.secret(binding)?});
        let output = crate::process::capture_command(
            docker.attach_command(&spec.operation_id)?,
            Duration::from_millis(binding.timeout_ms + 5000),
            Some(serde_json::to_vec(&request).map_err(storage)?),
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
        validator(schema)?.validate(value).map_err(|e| {
            err(
                "schema_validation",
                format!("数据不符合契约: {}", e.instance_path()),
            )
        })
    }
    fn start(&self, i: &ToolInvocation) -> Result<()> {
        require(!self.revoked(&i.tool)?, "接入已紧急停用")?;
        let mut jobs = self.jobs.lock().map_err(storage)?;
        if jobs.contains_key(&i.id) {
            return Ok(());
        }
        require(
            jobs.values()
                .filter(|j| j.lock().is_ok_and(|v| v.is_none()))
                .count()
                < 4,
            "工具并发已满",
        )?;
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
                let _ = fs::write(dir.join("receipt.json"), bytes);
            }
            if let Ok(mut slot) = job.lock() {
                *slot = Some(Ok(receipt));
            }
        });
        Ok(())
    }
    fn poll(&self, i: &ToolInvocation) -> Result<Option<ToolReceipt>> {
        if self.revoked(&i.tool)? {
            self.cancel(i)?;
            return Err(err("integration_disabled", "接入已停用，调用需核对"));
        }
        if let Some(job) = self.jobs.lock().map_err(storage)?.get(&i.id) {
            return job.lock().map_err(storage)?.clone().transpose();
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
        } else if name.starts_with("scripts/") {
            scripts = true;
        } else {
            files.push(SkillFile {
                path: name,
                content: text,
            });
        }
    }
    let body = body.ok_or_else(|| err("skill_package", "包缺少根目录 SKILL.md"))?;
    let (description, body) = parse_skill_markdown(&body)?;
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

pub fn parse_openapi_document(text: &str) -> Result<Value> {
    serde_yaml::from_str(text).map_err(|_| err("openapi_invalid", "无法解析 OpenAPI JSON/YAML"))
}
