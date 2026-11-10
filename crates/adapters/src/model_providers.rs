//! Deterministic DSH route configuration. Profiles contain references, never key values.
use super::*;

pub fn binding_model_connection(binding: &RuntimeBinding) -> Result<ModelConnection> {
    require(
        binding.installation.key.provider == HarnessProvider::Dsh,
        "需要 DSH 绑定",
    )?;
    let parameters = binding.effective_model_parameters();
    if let Some(connection) = &binding.model_connection {
        connection.validate(&binding.model, &parameters)?;
        require(
            binding.model_profile_digest.as_deref()
                == Some(
                    configuration_digest(&json!(dsh_model_patch(connection, &binding.model)?))?
                        .as_str(),
                ),
            "冻结模型 profile 摘要不一致",
        )?;
        require(
            binding
                .agent_configuration
                .as_ref()
                .is_none_or(|a| a.model_provider == connection.provider.as_str()),
            "Agent 服务商与模型连接不一致",
        )?;
        if connection.provider != DshModelProvider::Deepseek {
            require(
                binding.installation.key.version.split('+').next() == Some("0.2.2-rc.1"),
                "DSH 安装尚未验证此路由",
            )?;
        }
        return Ok(connection.clone());
    }
    require(
        binding
            .agent_configuration
            .as_ref()
            .is_none_or(|a| a.model_provider == "deepseek"),
        "新服务商缺少冻结模型连接，不能回退 DeepSeek",
    )?;
    require(
        binding.model_profile_digest.is_none(),
        "缺少连接快照却包含新 profile 摘要",
    )?;
    let m: ModelDefinition = serde_json::from_value(
        json!({"provider":"deepseek","model":binding.model,"parameters":parameters}),
    )
    .map_err(storage)?;
    let h: HarnessDefinition = serde_json::from_value(
        json!({"provider":"dsh","runtime_version":binding.installation.key.version}),
    )
    .map_err(storage)?;
    resolve_model_connection(&m, &h)?
        .ok_or_else(|| err("invalid_configuration", "缺少 DSH 模型连接"))
}

pub fn dsh_model_patch(connection: &ModelConnection, model: &str) -> Result<String> {
    connection.validate(model, &Default::default())?;
    if connection.provider == DshModelProvider::Deepseek {
        return Ok("\n- id: llm-pi-ai\n  disabled: true\n".into());
    }
    let mut route = json!({"apiKeyEnv":connection.provider.api_key_env(),"api":connection.protocol,"baseURL":connection.worker_endpoint,
        "models":[{"id":model,"contextWindow":connection.context_window,"maxTokens":connection.max_output_tokens}]});
    if connection.provider == DshModelProvider::Ollama {
        route["compat"] = json!({"supportsStore":false,"supportsDeveloperRole":false,"maxTokensField":"max_tokens","supportsStrictMode":false});
        route["models"][0]["reasoningEfforts"] = json!(false);
    }
    let config = json!({"providers":{connection.route.clone():route}});
    Ok(format!(
        "\n- id: llm-deepseek\n  disabled: true\n- id: llm-deepseek-account\n  disabled: true\n- id: llm-pi-ai\n  disabled: false\n  config: {}\n",
        serde_json::to_string(&config).map_err(storage)?
    ))
}

/// Read the local Ollama server without starting a model or requesting inference.
pub fn discover_ollama_models(endpoint: &str) -> Result<Value> {
    let (url, _, _) = ollama_endpoint(endpoint)?;
    let result = process::capture(
        "curl",
        &[
            "--silent".into(),
            "--show-error".into(),
            "--fail".into(),
            "--max-time".into(),
            "10".into(),
            "--max-redirs".into(),
            "0".into(),
            "--proto".into(),
            "=http,https".into(),
            "--noproxy".into(),
            "*".into(),
            "--url".into(),
            format!("{url}/models"),
        ],
        std::time::Duration::from_secs(12),
    )?;
    require(
        result.code == 0 && !result.truncated,
        "Ollama 模型目录不可达，请检查服务地址；此检查未验证容器访问或推理",
    )?;
    let body: Value = serde_json::from_slice(&result.stdout)
        .map_err(|_| err("model_catalog_invalid", "Ollama 返回了无效模型目录"))?;
    let models = body["data"]
        .as_array()
        .ok_or_else(|| err("model_catalog_invalid", "Ollama 目录缺少 data 数组"))?;
    require(models.len() <= 4096, "模型目录过大")?;
    let models: Vec<Value> = models
        .iter()
        .filter_map(|row| row["id"].as_str())
        .filter(|id| !id.is_empty() && id.len() <= 160 && !id.chars().any(char::is_control))
        .map(|id| json!({"id":id}))
        .collect();
    Ok(
        json!({"provider":"ollama","source":"host_endpoint","inference_verified":false,"container_verified":false,"models":models}),
    )
}

/// Read the account's published model directory, never invoking inference.
/// Authentication goes over curl's stdin, not process arguments or a file.
pub fn discover_deepseek_models(dataset: &Path, reference: &str) -> Result<Value> {
    let mut key = crate::credentials::CredentialVault::new(dataset)?
        .read_model_reference(DshModelProvider::Deepseek, reference)?;
    require(
        key.is_ascii() && !key.iter().any(|b| b.is_ascii_control()),
        "模型凭据包含无效字符",
    )?;
    let header = format!(
        "Authorization: Bearer {}",
        std::str::from_utf8(&key).map_err(storage)?
    );
    let input = format!(
        "header = {}\n",
        serde_json::to_string(&header).map_err(storage)?
    )
    .into_bytes();
    key.fill(0);
    let mut command = std::process::Command::new("curl");
    command.args([
        "-q",
        "--config",
        "-",
        "--silent",
        "--fail",
        "--max-time",
        "10",
        "--max-redirs",
        "0",
        "--proto",
        "=https",
        "--url",
        "https://api.deepseek.com/models",
    ]);
    let result =
        process::capture_command(command, std::time::Duration::from_secs(12), Some(input))?;
    require(
        result.code == 0 && !result.truncated,
        "无法读取 DeepSeek 模型目录，请检查所配置的 API key 与网络",
    )?;
    let body: Value = serde_json::from_slice(&result.stdout)
        .map_err(|_| err("model_catalog_invalid", "DeepSeek 返回了无效模型目录"))?;
    let rows = body["data"]
        .as_array()
        .ok_or_else(|| err("model_catalog_invalid", "DeepSeek 目录缺少模型列表"))?;
    let models:Vec<_>=rows.iter().filter_map(|row| {
        let id=row["id"].as_str().filter(|id| !id.is_empty() && id.len()<=160 && !id.chars().any(char::is_control))?;
        Some(json!({"id":id,"name":row["name"],"context_window":row["context_window"],"max_output_tokens":row["max_output_tokens"]}))
    }).collect();
    Ok(
        json!({"provider":"deepseek","source":"provider_endpoint","inference_verified":false,"models":models}),
    )
}

/// Checks metadata connectivity from the selected immutable runtime, without model inference.
/// The supplied Session binding supplies both network permissions and the exact vault reference.
pub fn check_container_connection(
    dataset: &Path,
    binding: &RuntimeBinding,
    project: &str,
    session: &str,
) -> Result<Value> {
    let connection = binding_model_connection(binding)?;
    require(
        binding
            .permissions
            .as_ref()
            .is_none_or(|p| p.sandbox.policy.network == NetworkPermission::Enabled),
        "Sandbox 禁网，无法连接模型服务",
    )?;
    let image = match &binding.installation.artifact {
        RuntimeArtifact::DockerImage { image_id } => image_id,
        _ => return Err(err("unsupported", "连接检查要求固定 Docker 安装")),
    };
    let operation = id();
    let base = dataset.join("model-connection-checks");
    reject_symlink(&base)?;
    fs::create_dir_all(&base).map_err(storage)?;
    let job = base.join(&operation);
    fs::create_dir(&job).map_err(storage)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&job, fs::Permissions::from_mode(0o700)).map_err(storage)?;
    }
    let sandbox = crate::docker::DockerSandbox::new(dataset)?;
    let result = (|| {
        for d in ["work", "state"] {
            fs::create_dir(job.join(d)).map_err(storage)?;
        }
        let mut key = if connection.provider.requires_key() {
            crate::credentials::CredentialVault::new(dataset)?.read_model_reference(
                connection.provider,
                binding
                    .credential_ref
                    .as_deref()
                    .ok_or_else(|| err("credentials_required", "缺少所选服务商凭据"))?,
            )?
        } else {
            b"ollama".to_vec()
        };
        require(
            !key.contains(&b'\n') && !key.contains(&b'\r'),
            "凭据包含换行",
        )?;
        let env = job.join("provider.env");
        let mut bytes = format!("{}=", connection.provider.api_key_env()).into_bytes();
        bytes.extend_from_slice(&key);
        bytes.push(b'\n');
        let mut file = OpenOptions::new();
        file.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            file.mode(0o600);
        }
        use std::io::Write;
        let written = file
            .open(&env)
            .map_err(storage)
            .and_then(|mut f| f.write_all(&bytes).map_err(storage));
        key.fill(0);
        bytes.fill(0);
        written?;
        let suffix = if connection.protocol == "anthropic-messages" {
            "/v1/models?limit=1000"
        } else {
            "/models"
        };
        let script = format!(
            r#"const endpoint={endpoint};const env={env};const model={model};const anthropic={anthropic};
try{{const key=process.env[env];const headers=anthropic?{{'x-api-key':key,'anthropic-version':'2023-06-01'}}:{{Authorization:'Bearer '+key}};
const r=await fetch(endpoint,{{headers,redirect:'error',signal:AbortSignal.timeout(12000)}});
if(!r.ok){{console.log(JSON.stringify({{connected:false,status:r.status}}));process.exit(2)}}
let text='';for await(const chunk of r.body){{text+=Buffer.from(chunk).toString();if(text.length>1048576)throw Error('large')}}
const b=JSON.parse(text);if(!Array.isArray(b.data))throw Error('catalog');console.log(JSON.stringify({{connected:true,model_present:b.data.some(m=>m.id===model)}}));
}}catch{{console.log(JSON.stringify({{connected:false,reason:'endpoint_unavailable_or_invalid_catalog'}}));process.exit(2)}}"#,
            endpoint = json!(format!("{}{suffix}", connection.worker_endpoint)),
            env = json!(connection.provider.api_key_env()),
            model = json!(binding.model),
            anthropic = connection.protocol == "anthropic-messages"
        );
        let receipt = sandbox.create(&crate::docker::ContainerSpec {
            skill_directory: None,
            sensitive_output: false,
            host_gateway: connection.host_gateway,
            permissions: binding.permissions.clone(),
            purpose: crate::docker::ContainerPurpose::Harness,
            project_id: project.into(),
            session_id: session.into(),
            operation_id: operation.clone(),
            image_id: image.clone(),
            worktree: job.join("work"),
            state: job.join("state"),
            writable: false,
            writable_subdirectories: vec![],
            read_only_subdirectories: vec![],
            network: true,
            command: vec![
                "node".into(),
                "--input-type=module".into(),
                "-e".into(),
                script,
            ],
            nested_sandbox: binding
                .permissions
                .as_ref()
                .is_some_and(|p| p.isolation.nested_sandbox),
            credential_env_file: Some(env.clone()),
            profile_file: None,
            runner_file: None,
            profile_target: None,
        })?;
        fs::remove_file(env).map_err(storage)?;
        require(!receipt.running, "连接检查容器不应提前启动")?;
        let output = process::capture_command(
            sandbox.attach_command(&operation)?,
            std::time::Duration::from_secs(20),
            None,
        )?;
        let detail: Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| err("model_endpoint_unavailable", "容器连接检查未返回结果"))?;
        Ok(
            json!({"provider":connection.provider,"endpoint":connection.endpoint,"worker_endpoint":connection.worker_endpoint,"installation_id":binding.installation.id,
            "container_verified":output.code==0 && detail["connected"]==true,"inference_verified":false,"detail":detail}),
        )
    })();
    let cleanup = sandbox
        .stop(&operation)
        .and_then(|_| sandbox.remove(&operation));
    if cleanup.is_ok() {
        fs::remove_dir_all(&job).map_err(storage)?;
    }
    cleanup?;
    result
}
