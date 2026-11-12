//! Deterministic DSH route configuration. Profiles contain references, never key values.
//! Routing authority was frozen at bind time against the Harness catalog; here the
//! frozen snapshot only re-validates itself (content, never runtime version strings).
use super::*;

pub fn binding_model_connection(binding: &RuntimeBinding) -> Result<ModelConnection> {
    require(
        binding.installation.key.provider == HarnessProvider::Dsh,
        "需要 DSH 绑定",
    )?;
    let parameters = binding.effective_model_parameters();
    let Some(connection) = &binding.model_connection else {
        return Err(err(
            "invalid_configuration",
            "DSH 绑定缺少冻结模型连接；请通过 Agent 配置重新解析",
        ));
    };
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
            .is_none_or(|a| a.model_provider == connection.provider),
        "Agent 服务商与模型连接不一致",
    )?;
    Ok(connection.clone())
}

pub fn dsh_model_patch(connection: &ModelConnection, model: &str) -> Result<String> {
    connection.validate(model, &Default::default())?;
    if connection.native {
        return Ok("\n- id: llm-pi-ai\n  disabled: true\n".into());
    }
    let mut route = json!({"apiKeyEnv":connection.api_key_env,"api":connection.protocol,"baseURL":connection.worker_endpoint,
        "models":[{"id":model,"contextWindow":connection.context_window,"maxTokens":connection.max_output_tokens}]});
    if connection.kind == ProviderKind::LocalServer {
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
pub fn discover_deepseek_models(dataset: &Path, facts: &ProviderFacts, reference: &str) -> Result<Value> {
    let mut key = crate::credentials::CredentialVault::new(dataset)?
        .read_model_reference(facts, reference)?;
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
        json!({"provider":facts.id,"source":"provider_endpoint","inference_verified":false,"models":models}),
    )
}
