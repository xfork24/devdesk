//! Model routing is independent of the Harness identity. No credentials or I/O live here.
use crate::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DshModelProvider {
    #[serde(rename = "deepseek")]
    Deepseek,
    #[serde(rename = "zai-coding-cn")]
    ZaiCodingCn,
    #[serde(rename = "minimax-cn")]
    MinimaxCn,
    #[serde(rename = "ollama")]
    Ollama,
}

impl DshModelProvider {
    pub const ALL: [Self; 4] = [
        Self::Deepseek,
        Self::ZaiCodingCn,
        Self::MinimaxCn,
        Self::Ollama,
    ];
    pub fn parse(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.as_str() == value)
            .ok_or_else(|| err("unsupported_model_provider", "DSH 不支持此模型服务商"))
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deepseek => "deepseek",
            Self::ZaiCodingCn => "zai-coding-cn",
            Self::MinimaxCn => "minimax-cn",
            Self::Ollama => "ollama",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Deepseek => "DeepSeek 官方 API",
            Self::ZaiCodingCn => "智谱 AI 编程订阅 国内",
            Self::MinimaxCn => "MiniMax 订阅 国内",
            Self::Ollama => "本机 Ollama",
        }
    }
    pub fn route(self) -> &'static str {
        if self == Self::Deepseek {
            "deepseek-official"
        } else {
            self.as_str()
        }
    }
    pub fn api_key_env(self) -> &'static str {
        match self {
            Self::Deepseek => "DEEPSEEK_API_KEY",
            Self::ZaiCodingCn => "ZAI_CODING_CN_API_KEY",
            Self::MinimaxCn => "MINIMAX_CN_API_KEY",
            Self::Ollama => "DEVDESK_OLLAMA_PLACEHOLDER",
        }
    }
    pub fn endpoint(self) -> &'static str {
        match self {
            Self::Deepseek => "https://api.deepseek.com",
            Self::ZaiCodingCn => "https://open.bigmodel.cn/api/coding/paas/v4",
            Self::MinimaxCn => "https://api.minimax.cn/anthropic",
            Self::Ollama => "http://127.0.0.1:11434/v1",
        }
    }
    pub fn protocol(self) -> &'static str {
        match self {
            Self::Deepseek => "deepseek-native",
            Self::MinimaxCn => "anthropic-messages",
            _ => "openai-completions",
        }
    }
    pub fn requires_key(self) -> bool {
        self != Self::Ollama
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelCapacity {
    pub id: String,
    pub context_window: u64,
    pub max_output_tokens: u64,
}

#[derive(Deserialize)]
struct Catalog {
    runtime_version: String,
    models: BTreeMap<String, Vec<ModelCapacity>>,
}

pub fn dsh_model_catalog(provider: DshModelProvider) -> Vec<ModelCapacity> {
    let mut catalog: Catalog = serde_json::from_str(include_str!(
        "../../../resources/configuration/dsh-model-catalog.json"
    ))
    .expect("embedded verified DSH catalog");
    assert_eq!(
        catalog.runtime_version, DEFAULT_DSH_VERSION,
        "embedded model catalog runtime"
    );
    catalog.models.remove(provider.as_str()).unwrap_or_default()
}

pub fn model_provider_catalog() -> Value {
    let mut rows = vec![
        serde_json::json!({"id":"openai","label":"OpenAI / ChatGPT","harness":"codex","auth_kind":"oauth","models":[]}),
    ];
    for p in DshModelProvider::ALL {
        rows.push(serde_json::json!({"id":p.as_str(),"label":p.label(),"harness":"dsh","route":p.route(),"protocol":p.protocol(),"endpoint":p.endpoint(),"auth_kind":if p.requires_key(){"api_key"}else{"none"},"runtime_version":"0.2.2-rc.1","models":dsh_model_catalog(p)}));
    }
    serde_json::json!({"providers":rows})
}

/// Declared URL and effective worker URL are both frozen; credentials remain in RuntimeBinding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConnection {
    pub provider: DshModelProvider,
    pub route: String,
    pub protocol: String,
    pub endpoint: String,
    pub worker_endpoint: String,
    pub host_gateway: bool,
    pub context_window: u64,
    pub max_output_tokens: u64,
}

pub fn ollama_endpoint(value: &str) -> Result<(String, String, bool)> {
    let mut url =
        url::Url::parse(value).map_err(|_| err("invalid_endpoint", "Ollama 服务地址无效"))?;
    require(
        value.len() <= 2048
            && ["http", "https"].contains(&url.scheme())
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Ollama 地址必须为 HTTP/HTTPS，不能包含凭据、查询或片段",
    )?;
    let path = url.path().trim_end_matches('/');
    let path = if path.is_empty() { "/v1" } else { path };
    require(
        path.ends_with("/v1") && !path.contains("//"),
        "Ollama API 根路径必须以 /v1 结尾",
    )?;
    let path = path.to_owned();
    url.set_path(&path);
    let endpoint = url.as_str().trim_end_matches('/').to_owned();
    let loopback = url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if loopback {
        url.set_host(Some("host.docker.internal"))
            .map_err(|_| err("invalid_endpoint", "无法解析容器地址"))?;
    }
    let gateway = loopback || url.host_str() == Some("host.docker.internal");
    Ok((
        endpoint,
        url.to_string().trim_end_matches('/').to_owned(),
        gateway,
    ))
}

/// The pinned Codex protocol advertises effort identifiers in its model catalog.
pub fn validate_reasoning_effort(effort: &str) -> Result<()> {
    require(
        !effort.is_empty()
            && effort.len() <= 64
            && effort
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c)),
        "思考深度标识无效",
    )
}

/// Validates model fields without resolving a mutable account or starting any process.
pub fn validate_model_definition(m: &ModelDefinition) -> Result<()> {
    require(
        !m.model.trim().is_empty()
            && m.model.len() <= 160
            && !m.model.chars().any(char::is_control),
        "模型 ID 无效",
    )?;
    if m.provider == "openai" {
        require(
            m.endpoint.is_none() && m.context_window.is_none() && m.max_output_tokens.is_none(),
            "Codex 尚不支持自定义 endpoint 或模型容量",
        )?;
        for (key, value) in &m.parameters {
            require(
                key == "reasoning_effort"
                    && value
                        .as_str()
                        .is_some_and(|v| validate_reasoning_effort(v).is_ok()),
                "Codex 模型参数无效",
            )?;
        }
        return Ok(());
    }
    let p = DshModelProvider::parse(&m.provider)?;
    require(
        p == DshModelProvider::Ollama || m.endpoint.is_none(),
        "云服务商使用固定订阅端点，不支持自定义 endpoint",
    )?;
    if p == DshModelProvider::Ollama {
        ollama_endpoint(m.endpoint.as_deref().unwrap_or(p.endpoint()))?;
    }
    let known = dsh_model_catalog(p).into_iter().find(|v| v.id == m.model);
    let context = m
        .context_window
        .or(known.as_ref().map(|v| v.context_window));
    let output = m
        .max_output_tokens
        .or(known.as_ref().map(|v| v.max_output_tokens));
    if p != DshModelProvider::Deepseek {
        require(
            context.is_some() && output.is_some(),
            "未收录的模型需要明确 context_window 和 max_output_tokens",
        )?;
    }
    let context = context.unwrap_or(1_000_000);
    let output = output.unwrap_or(131072);
    require(
        (1..=2_000_000).contains(&context)
            && (1..=1_000_000).contains(&output)
            && output <= context,
        "模型上下文或输出容量无效",
    )?;
    for (key, value) in &m.parameters {
        require(
            key == "max_tokens"
                && value.as_u64().is_some_and(|v| {
                    v >= if p == DshModelProvider::Deepseek {
                        1024
                    } else {
                        1
                    } && v <= output
                        && (p != DshModelProvider::Deepseek || v <= 131072)
                }),
            format!("Model 参数 {key} 无法由此 Harness 精确执行"),
        )?;
    }
    Ok(())
}

pub fn resolve_model_connection(
    m: &ModelDefinition,
    h: &HarnessDefinition,
) -> Result<Option<ModelConnection>> {
    validate_model_definition(m)?;
    require(
        h.model_providers.is_empty() || h.model_providers.contains(&m.provider),
        "Harness 不支持该 Model 服务商，请创建兼容的新修订",
    )?;
    if h.provider == "codex" {
        require(m.provider == "openai", "Codex 仅支持 OpenAI Model")?;
        return Ok(None);
    }
    require(h.provider == "dsh", "未知 Harness")?;
    let p = DshModelProvider::parse(&m.provider)?;
    if p != DshModelProvider::Deepseek {
        require(
            h.runtime_version.split('+').next() == Some("0.2.2-rc.1"),
            "该 DSH 版本尚未验证此模型路由",
        )?;
    }
    let known = dsh_model_catalog(p).into_iter().find(|v| v.id == m.model);
    let (endpoint, worker_endpoint, host_gateway) = if p == DshModelProvider::Ollama {
        ollama_endpoint(m.endpoint.as_deref().unwrap_or(p.endpoint()))?
    } else {
        (p.endpoint().into(), p.endpoint().into(), false)
    };
    Ok(Some(ModelConnection {
        provider: p,
        route: p.route().into(),
        protocol: p.protocol().into(),
        endpoint,
        worker_endpoint,
        host_gateway,
        context_window: m
            .context_window
            .or(known.as_ref().map(|v| v.context_window))
            .unwrap_or(1_000_000),
        max_output_tokens: m
            .max_output_tokens
            .or(known.as_ref().map(|v| v.max_output_tokens))
            .unwrap_or(131072),
    }))
}

impl ModelConnection {
    /// Rejects tampered or incomplete snapshots before any credential is read.
    pub fn validate(&self, model: &str, parameters: &BTreeMap<String, Value>) -> Result<()> {
        let m = ModelDefinition {
            provider: self.provider.as_str().into(),
            model: model.into(),
            endpoint: if self.provider == DshModelProvider::Ollama {
                Some(self.endpoint.clone())
            } else {
                None
            },
            context_window: Some(self.context_window),
            max_output_tokens: Some(self.max_output_tokens),
            release: None,
            parameters: parameters.clone(),
            capabilities: vec![],
        };
        let h = HarnessDefinition {
            provider: "dsh".into(),
            runtime_version: "0.2.2-rc.1".into(),
            recipe_digest: None,
            capabilities: vec![],
            model_providers: vec![m.provider.clone()],
        };
        require(
            resolve_model_connection(&m, &h)?.as_ref() == Some(self),
            "冻结模型连接与服务商路由不一致",
        )
    }
    pub fn output_budget(&self, formal: bool, parameters: &BTreeMap<String, Value>) -> Result<u64> {
        self.validate("budget-check", parameters)?;
        Ok(parameters
            .get("max_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(if formal { 32768 } else { 8192 })
            .min(self.max_output_tokens))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn model(provider: &str) -> ModelDefinition {
        serde_json::from_value(json!({"provider":provider,"model":"test-model","context_window":32768,"max_output_tokens":4096,"parameters":{"max_tokens":2048}})).unwrap()
    }
    fn harness(providers: &[&str]) -> HarnessDefinition {
        serde_json::from_value(
            json!({"provider":"dsh","runtime_version":"0.2.2-rc.1","model_providers":providers}),
        )
        .unwrap()
    }
    #[test]
    fn all_dsh_routes_resolve_without_changing_harness_identity() {
        for p in DshModelProvider::ALL {
            let m = model(p.as_str());
            let c = resolve_model_connection(&m, &harness(&[p.as_str()]))
                .unwrap()
                .unwrap();
            assert_eq!(c.route, p.route());
            c.validate(&m.model, &m.parameters).unwrap();
            assert_eq!(c.output_budget(true, &m.parameters).unwrap(), 2048);
            assert!(resolve_model_connection(&m, &harness(&["openai"])).is_err());
        }
    }
    #[test]
    fn fixed_catalog_has_capacities_and_cloud_endpoints_cannot_be_overridden() {
        for p in [DshModelProvider::ZaiCodingCn, DshModelProvider::MinimaxCn] {
            let known = dsh_model_catalog(p).remove(0);
            let m: ModelDefinition =
                serde_json::from_value(json!({"provider":p,"model":known.id})).unwrap();
            assert_eq!(
                resolve_model_connection(&m, &harness(&[p.as_str()]))
                    .unwrap()
                    .unwrap()
                    .context_window,
                known.context_window
            );
            let mut invalid = m;
            invalid.endpoint = Some("https://other.invalid/v1".into());
            assert!(validate_model_definition(&invalid).is_err());
        }
        let mut m = model("ollama");
        m.context_window = None;
        assert!(validate_model_definition(&m).is_err());
    }
    #[test]
    fn endpoint_normalization_preserves_explicit_paths_and_rejects_secrets() {
        for host in ["localhost", "127.0.0.1", "[::1]"] {
            let (declared, worker, gateway) =
                ollama_endpoint(&format!("http://{host}:11434/v1/")).unwrap();
            assert!(!declared.ends_with('/'));
            assert_eq!(worker, "http://host.docker.internal:11434/v1");
            assert!(gateway);
        }
        assert_eq!(
            ollama_endpoint("http://192.168.1.3:11434").unwrap().1,
            "http://192.168.1.3:11434/v1"
        );
        for url in [
            "file:///etc/passwd",
            "http://key@localhost/v1",
            "http://localhost/v1?api_key=x",
            "http://localhost/v1#x",
            "http://localhost/api/chat",
        ] {
            assert!(ollama_endpoint(url).is_err(), "{url}");
        }
    }
    #[test]
    fn explicit_budget_and_tampered_snapshot_fail_before_dispatch() {
        let mut m = model("ollama");
        let mut c = resolve_model_connection(&m, &harness(&["ollama"]))
            .unwrap()
            .unwrap();
        c.worker_endpoint = "https://other.invalid/v1".into();
        assert!(c.validate(&m.model, &m.parameters).is_err());
        m.parameters.insert("max_tokens".into(), json!(8192));
        assert!(validate_model_definition(&m).is_err());
        m.parameters.clear();
        m.parameters.insert("temperature".into(), json!(0.5));
        assert!(validate_model_definition(&m).is_err());
        assert!(validate_model_definition(&model("unknown")).is_err());
    }
}
