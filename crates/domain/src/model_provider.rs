//! Model routing is independent of the Harness identity. No credentials or I/O live here.
//! Provider rosters, endpoints and capacities are configuration instances resolved from
//! state; this module only defines the concepts and the pure rules over supplied facts.
use crate::*;
use std::collections::BTreeMap;

/// Provider archetype. Concrete providers (DeepSeek, Ollama, ...) are configuration
/// instances; only the execution shape of a provider is a domain concept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind {
    /// The owning harness authenticates and routes this provider itself (OAuth account).
    #[serde(rename = "oauth_managed")]
    OAuthManaged,
    /// Fixed subscription endpoint; the account key never selects a different base URL.
    #[serde(rename = "cloud_subscription")]
    CloudSubscription,
    /// Operator-managed local server; the endpoint is part of the model definition.
    #[serde(rename = "local_server")]
    LocalServer,
}

/// Instance facts for one model provider. Assembled from `model_provider` sources plus
/// the model catalog referenced by the selected Harness; never compiled into code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderFacts {
    /// Public provider identifier used by model definitions and credential references.
    pub id: String,
    pub label: String,
    pub kind: ProviderKind,
    /// Owning harness provider ("codex" / "dsh").
    pub harness: String,
    /// Default endpoint; empty for OAuth-managed providers.
    #[serde(default)]
    pub endpoint: String,
    pub route: String,
    pub protocol: String,
    #[serde(default)]
    pub api_key_env: String,
    #[serde(default)]
    pub requires_key: bool,
    /// The harness already routes this provider natively; no custom route block is emitted.
    #[serde(default)]
    pub native: bool,
    /// Models absent from the catalog must declare capacities explicitly.
    #[serde(default)]
    pub capacity_required: bool,
    /// Bounds for the `max_tokens` model parameter.
    #[serde(default = "one")]
    pub min_output_request: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_request: Option<u64>,
    /// Keychain account prefix for stored API keys; empty when no key is stored.
    #[serde(default)]
    pub credential_account: String,
    /// Catalog capacities attached by the Harness resolution; sources must not set them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<ModelCapacity>,
}
fn one() -> u64 {
    1
}

impl ProviderFacts {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.id.trim().is_empty()
                && !self.id.chars().any(|c| c.is_whitespace() || c.is_control())
                && self.id.len() <= 64,
            "服务商标识无效",
        )?;
        require(!self.label.trim().is_empty(), "服务商需要名称")?;
        require(
            !self.harness.trim().is_empty()
                && !self.harness.chars().any(|c| c.is_whitespace() || c.is_control()),
            "服务商必须归属一个 Harness",
        )?;
        match self.kind {
            ProviderKind::OAuthManaged => {
                require(self.endpoint.is_empty(), "OAuth 服务商不声明固定端点")?;
            }
            ProviderKind::CloudSubscription | ProviderKind::LocalServer => {
                require(
                    !self.route.trim().is_empty() && !self.protocol.trim().is_empty(),
                    "订阅或本地服务商需要路由与协议",
                )?;
                require(!self.endpoint.is_empty(), "订阅或本地服务商需要默认端点")?;
                let url = url::Url::parse(&self.endpoint)
                    .map_err(|_| err("invalid_source", "服务商端点无效"))?;
                require(
                    ["http", "https"].contains(&url.scheme())
                        && url.host_str().is_some()
                        && url.username().is_empty()
                        && url.password().is_none()
                        && url.query().is_none()
                        && url.fragment().is_none(),
                    "服务商端点只允许无内嵌凭据的 HTTP(S) 地址",
                )?;
            }
        }
        if self.requires_key {
            require(
                !self.api_key_env.is_empty()
                    && self.api_key_env
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c == '_')
                    && !self.credential_account.is_empty(),
                "需要密钥的服务商必须声明环境变量名与凭据账户前缀",
            )?;
        } else {
            require(
                self.api_key_env.is_empty() && self.credential_account.is_empty(),
                "免密服务商不声明环境变量名或凭据账户",
            )?;
        }
        validate_capacities(&self.models)?;
        Ok(())
    }
}

fn validate_capacities(models: &[ModelCapacity]) -> Result<()> {
    let mut ids = std::collections::BTreeSet::new();
    for model in models {
        require(
            !model.id.is_empty() && model.id.len() <= 160 && ids.insert(model.id.clone()),
            "模型目录条目无效或重复",
        )?;
        require(
            (1..=2_000_000).contains(&model.context_window)
                && (1..=1_000_000).contains(&model.max_output_tokens)
                && model.max_output_tokens <= model.context_window,
            "模型目录容量无效",
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCapacity {
    pub id: String,
    pub context_window: u64,
    pub max_output_tokens: u64,
}

/// Catalog of verified model capacities for one Harness build, referenced by the Harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCatalogDefinition {
    /// Managed build inputs whose first verified image this catalog was validated against.
    pub recipe_digest: String,
    #[serde(default)]
    pub providers: Vec<CatalogProviderModels>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogProviderModels {
    pub provider_ref: Id,
    #[serde(default)]
    pub models: Vec<ModelCapacity>,
}
impl ModelCatalogDefinition {
    pub fn validate(&self) -> Result<()> {
        require(
            self.recipe_digest.len() == 64
                && self
                    .recipe_digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "模型目录需要关联确切的构建配方摘要",
        )?;
        let mut refs = std::collections::BTreeSet::new();
        for provider in &self.providers {
            parse_source_reference(&provider.provider_ref)?;
            require(refs.insert(provider.provider_ref.clone()), "目录服务商引用重复")?;
            validate_capacities(&provider.models)?;
        }
        Ok(())
    }
    pub fn capacities(&self, provider_ref: &str) -> &[ModelCapacity] {
        self.providers
            .iter()
            .find(|p| p.provider_ref == provider_ref)
            .map(|p| p.models.as_slice())
            .unwrap_or(&[])
    }
}

impl EngineeringState {
    /// Facts for a provider as reachable through one Harness, with catalog capacities.
    pub fn provider_facts(&self, harness: &HarnessDefinition, provider_id: &str) -> Result<ProviderFacts> {
        let mut matched: Option<(String, ProviderFacts)> = None;
        for reference in &harness.model_providers {
            let record = self.configuration(reference)?;
            require(
                record.kind == "model_provider",
                "Harness 的服务商引用类型不匹配",
            )?;
            let candidate: ProviderFacts = record.decode()?;
            if candidate.id == provider_id {
                require(matched.is_none(), "Harness 重复引用同一 Model 服务商")?;
                matched = Some((reference.clone(), candidate));
            }
        }
        let (reference, mut facts) = match matched {
            Some(found) => found,
            // An empty provider list keeps the Harness open to any published provider.
            None if harness.model_providers.is_empty() => {
                let record = self
                    .governance
                    .iter()
                    .find(|r| r.kind == "model_provider" && r.data["id"] == provider_id)
                    .ok_or_else(|| err("blocked", "Harness 不支持该 Model 服务商"))?;
                (record.id.clone(), record.decode()?)
            }
            None => return Err(err("blocked", "Harness 不支持该 Model 服务商")),
        };
        require(facts.harness == harness.provider, "Model 服务商与 Harness 不兼容")?;
        if let Some(catalog_ref) = &harness.model_catalog_ref {
            let catalog: ModelCatalogDefinition = self.configuration(catalog_ref)?.decode()?;
            facts.models = catalog.capacities(&reference).to_vec();
        }
        Ok(facts)
    }

    /// Facts for a provider from any Harness that declares it; bare record otherwise.
    /// Used to validate model sources before a Harness is selected.
    pub fn provider_facts_any(&self, provider_id: &str) -> Result<ProviderFacts> {
        for r in self.governance.iter().filter(|r| r.kind == "harness") {
            let harness: HarnessDefinition = r.decode()?;
            if harness
                .model_providers
                .iter()
                .any(|reference| self.configuration(reference).is_ok_and(|c| c.kind == "model_provider" && c.data["id"] == provider_id))
            {
                return self.provider_facts(&harness, provider_id);
            }
        }
        self.governance
            .iter()
            .find(|r| r.kind == "model_provider" && r.data["id"] == provider_id)
            .map(|r| r.decode())
            .transpose()?
            .ok_or_else(|| err("unsupported_model_provider", "未知 Model 服务商"))
    }

    pub fn resolve_model_connection(
        &self,
        m: &ModelDefinition,
        h: &HarnessDefinition,
    ) -> Result<Option<ModelConnection>> {
        let facts = self.provider_facts(h, &m.provider)?;
        resolve_model_connection(m, &facts)
    }
}

/// Declared URL and effective worker URL are both frozen; credentials remain in RuntimeBinding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConnection {
    pub provider: String,
    pub kind: ProviderKind,
    pub route: String,
    pub protocol: String,
    pub endpoint: String,
    pub worker_endpoint: String,
    pub host_gateway: bool,
    pub api_key_env: String,
    /// Keychain account prefix this provider's stored API key is addressed by.
    #[serde(default)]
    pub credential_account: String,
    /// The harness routes this provider natively; no custom route block is emitted.
    #[serde(default)]
    pub native: bool,
    pub context_window: u64,
    pub max_output_tokens: u64,
    /// Frozen parameter bounds; the snapshot validates itself without the catalog.
    #[serde(default = "one")]
    pub min_output_request: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_request: Option<u64>,
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

/// Validates model fields against the provider's published facts; no I/O, no state.
pub fn validate_model_definition(m: &ModelDefinition, facts: &ProviderFacts) -> Result<()> {
    require(
        !m.model.trim().is_empty()
            && m.model.len() <= 160
            && !m.model.chars().any(char::is_control),
        "模型 ID 无效",
    )?;
    match facts.kind {
        ProviderKind::OAuthManaged => {
            require(
                m.endpoint.is_none() && m.context_window.is_none() && m.max_output_tokens.is_none(),
                "此 Harness 尚不支持自定义 endpoint 或模型容量",
            )?;
            for (key, value) in &m.parameters {
                require(
                    key == "reasoning_effort"
                        && value
                            .as_str()
                            .is_some_and(|v| validate_reasoning_effort(v).is_ok()),
                    "模型参数无效",
                )?;
            }
            return Ok(());
        }
        ProviderKind::CloudSubscription => {
            require(
                m.endpoint.is_none(),
                "云服务商使用固定订阅端点，不支持自定义 endpoint",
            )?;
        }
        ProviderKind::LocalServer => {
            ollama_endpoint(m.endpoint.as_deref().unwrap_or(&facts.endpoint))?;
        }
    }
    let known = facts.models.iter().find(|v| v.id == m.model);
    let context = m
        .context_window
        .or(known.as_ref().map(|v| v.context_window));
    let output = m
        .max_output_tokens
        .or(known.as_ref().map(|v| v.max_output_tokens));
    if facts.capacity_required {
        require(
            context.is_some() && output.is_some(),
            "未收录的模型需要明确 context_window 和 max_output_tokens",
        )?;
    }
    let context = context.unwrap_or(1_000_000);
    let output = output.unwrap_or(131_072);
    require(
        (1..=2_000_000).contains(&context)
            && (1..=1_000_000).contains(&output)
            && output <= context,
        "模型上下文或输出容量无效",
    )?;
    for (key, value) in &m.parameters {
        let max = facts.max_output_request.unwrap_or(u64::MAX).min(output);
        require(
            key == "max_tokens"
                && value.as_u64().is_some_and(|v| v >= facts.min_output_request && v <= max),
            format!("Model 参数 {key} 无法由此 Harness 精确执行"),
        )?;
    }
    Ok(())
}

/// Resolves the frozen connection from a model and its provider facts. OAuth-managed
/// providers return None: the owning harness holds the account and the route itself.
pub fn resolve_model_connection(
    m: &ModelDefinition,
    facts: &ProviderFacts,
) -> Result<Option<ModelConnection>> {
    validate_model_definition(m, facts)?;
    if facts.kind == ProviderKind::OAuthManaged {
        return Ok(None);
    }
    let known = facts.models.iter().find(|v| v.id == m.model);
    let (endpoint, worker_endpoint, host_gateway) = if facts.kind == ProviderKind::LocalServer {
        ollama_endpoint(m.endpoint.as_deref().unwrap_or(&facts.endpoint))?
    } else {
        (facts.endpoint.clone(), facts.endpoint.clone(), false)
    };
    Ok(Some(ModelConnection {
        provider: facts.id.clone(),
        kind: facts.kind,
        route: facts.route.clone(),
        protocol: facts.protocol.clone(),
        endpoint,
        worker_endpoint,
        host_gateway,
        api_key_env: facts.api_key_env.clone(),
        credential_account: facts.credential_account.clone(),
        native: facts.native,
        context_window: m
            .context_window
            .or(known.as_ref().map(|v| v.context_window))
            .unwrap_or(1_000_000),
        max_output_tokens: m
            .max_output_tokens
            .or(known.as_ref().map(|v| v.max_output_tokens))
            .unwrap_or(131_072),
        min_output_request: facts.min_output_request,
        max_output_request: facts.max_output_request,
    }))
}

impl ModelConnection {
    /// Rejects tampered or incomplete snapshots before any credential is read.
    /// Self-contained: the snapshot freezes the parameter bounds it was resolved under.
    pub fn validate(&self, model: &str, parameters: &BTreeMap<String, Value>) -> Result<()> {
        require(
            !model.trim().is_empty() && model.len() <= 160 && !model.chars().any(char::is_control),
            "模型 ID 无效",
        )?;
        require(
            self.provider.len() <= 64
                && !self.provider.chars().any(|c| c.is_whitespace() || c.is_control()),
            "冻结模型连接的服务商无效",
        )?;
        // Self-consistency: the frozen worker view must still derive from the declared
        // endpoint; tampering either side invalidates the snapshot without any catalog.
        match self.kind {
            ProviderKind::LocalServer => {
                let (endpoint, worker, gateway) = ollama_endpoint(&self.endpoint)?;
                require(
                    endpoint == self.endpoint
                        && worker == self.worker_endpoint
                        && gateway == self.host_gateway,
                    "冻结模型连接的端点不一致",
                )?;
            }
            ProviderKind::CloudSubscription => {
                require(
                    self.endpoint == self.worker_endpoint && !self.host_gateway,
                    "冻结模型连接的端点不一致",
                )?;
            }
            ProviderKind::OAuthManaged => {
                require(
                    self.endpoint.is_empty() && self.worker_endpoint.is_empty(),
                    "OAuth 托管连接不声明端点",
                )?;
            }
        }
        let max = self.max_output_request.unwrap_or(u64::MAX).min(self.max_output_tokens);
        for (key, value) in parameters {
            require(
                key == "max_tokens"
                    && value.as_u64().is_some_and(|v| v >= self.min_output_request && v <= max),
                format!("Model 参数 {key} 无法由此 Harness 精确执行"),
            )?;
        }
        Ok(())
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
    fn facts(id: &str, kind: &str, requires_key: bool) -> ProviderFacts {
        serde_json::from_value(json!({
            "id": id, "label": id, "kind": kind, "harness": "dsh",
            "endpoint": if kind == "local_server" { json!("http://127.0.0.1:11434/v1") } else { json!("https://provider.invalid") },
            "route": id, "protocol": "openai-completions",
            "requires_key": requires_key,
            "api_key_env": if requires_key { "TEST_PROVIDER_KEY" } else { "" },
            "credential_account": if requires_key { format!("dsh-{id}-api-key-") } else { "".to_string() },
            "capacity_required": true
        }))
        .unwrap()
    }
    fn model(provider: &str) -> ModelDefinition {
        serde_json::from_value(json!({"provider":provider,"model":"test-model","context_window":32768,"max_output_tokens":4096,"parameters":{"max_tokens":2048}})).unwrap()
    }
    #[test]
    fn cloud_routes_freeze_the_published_facts() {
        for requires_key in [true, false] {
            let f = facts("zai-coding-cn", "cloud_subscription", requires_key);
            let c = resolve_model_connection(&model("zai-coding-cn"), &f)
                .unwrap()
                .unwrap();
            assert_eq!(c.route, "zai-coding-cn");
            assert_eq!(c.worker_endpoint, f.endpoint);
            assert_eq!(c.host_gateway, false);
            assert_eq!(c.api_key_env.is_empty(), !requires_key);
            c.validate("test-model", &model("zai-coding-cn").parameters).unwrap();
            assert_eq!(c.output_budget(true, &model("zai-coding-cn").parameters).unwrap(), 2048);
        }
    }
    #[test]
    fn unknown_models_need_declared_capacities_and_parameters_are_bounded() {
        let f = facts("minimax-cn", "cloud_subscription", true);
        let mut m = model("minimax-cn");
        m.context_window = None;
        m.max_output_tokens = None;
        assert!(resolve_model_connection(&m, &f).is_err());
        m.context_window = Some(65536);
        m.max_output_tokens = Some(8192);
        m.parameters.insert("max_tokens".into(), json!(1));
        let c = resolve_model_connection(&m, &f).unwrap().unwrap();
        // Frozen bounds reject out-of-range parameters at use time without the catalog.
        m.parameters.insert("max_tokens".into(), json!(999_999));
        assert!(c.validate("test-model", &m.parameters).is_err());
        m.parameters.clear();
        m.parameters.insert("temperature".into(), json!(0.5));
        assert!(resolve_model_connection(&m, &f).is_err());
    }
    #[test]
    fn catalog_capacities_fill_model_defaults_and_survive_freezing() {
        let mut f = facts("minimax-cn", "cloud_subscription", true);
        f.models = vec![ModelCapacity {
            id: "known-model".into(),
            context_window: 204_800,
            max_output_tokens: 131_072,
        }];
        let m: ModelDefinition = serde_json::from_value(json!({
            "provider":"minimax-cn","model":"known-model","parameters":{}
        }))
        .unwrap();
        let c = resolve_model_connection(&m, &f).unwrap().unwrap();
        assert_eq!(c.context_window, 204_800);
        assert_eq!(c.max_output_tokens, 131_072);
    }
    #[test]
    fn oauth_managed_providers_have_no_connection_and_strict_fields() {
        let f: ProviderFacts = serde_json::from_value(json!({
            "id":"openai","label":"OpenAI","kind":"oauth_managed","harness":"codex",
            "endpoint":"","route":"","protocol":"","requires_key":false
        }))
        .unwrap();
        f.validate().unwrap();
        let m: ModelDefinition = serde_json::from_value(json!({
            "provider":"openai","model":"gpt","parameters":{"reasoning_effort":"high"}
        }))
        .unwrap();
        assert!(resolve_model_connection(&m, &f).unwrap().is_none());
        let mut invalid = m.clone();
        invalid.endpoint = Some("https://other.invalid".into());
        assert!(resolve_model_connection(&invalid, &f).is_err());
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
    fn tampered_snapshots_fail_self_validation_before_dispatch() {
        let f = facts("ollama", "local_server", false);
        let m = model("ollama");
        let mut c = resolve_model_connection(&m, &f).unwrap().unwrap();
        c.worker_endpoint = "https://other.invalid/v1".into();
        assert!(c.validate("test-model", &m.parameters).is_err());
    }
}
