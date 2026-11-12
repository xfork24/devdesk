//! Model provider RPC surface: assembled from published provider sources, never a
//! compiled-in roster. Discovery I/O delegates to adapters.
use super::*;

impl Workspace {
    /// All published model providers as `(source reference, facts)` pairs, with
    /// catalog capacities attached when a harness covers them.
    pub(super) fn model_provider_facts(&self) -> Result<Vec<(Id, ProviderFacts)>> {
        let state = self.configuration.store.snapshot()?;
        let mut facts = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for r in state.governance.iter().filter(|r| r.kind == "model_provider") {
            let mut p: ProviderFacts = r.decode()?;
            if seen.insert(p.id.clone()) && p.models.is_empty() {
                for h in state.governance.iter().filter(|h| h.kind == "harness") {
                    if let Ok(harness) = h.decode::<HarnessDefinition>() {
                        if let Ok(covered) = state.provider_facts(&harness, &p.id) {
                            if !covered.models.is_empty() {
                                p.models = covered.models;
                                break;
                            }
                        }
                    }
                }
            }
            facts.push((r.id.clone(), p));
        }
        Ok(facts)
    }

    fn selected_provider(&self, params: &Value) -> Result<ProviderFacts> {
        let id = match params.get("model_provider") {
            None => "deepseek",
            Some(Value::String(p)) => p,
            _ => return Err(err("invalid_params", "model_provider 必须为服务商标识")),
        };
        self.model_provider_facts()?
            .into_iter()
            .find(|(_, p)| p.id == id)
            .map(|(_, p)| p)
            .ok_or_else(|| err("unsupported_model_provider", "未知模型服务商"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_configuration_and_keyless_status_do_not_start_a_model() {
        let root = tempfile::tempdir().unwrap();
        let w = Workspace::open(root.path()).unwrap();
        let catalog = w.dispatch("query.model_providers", json!({})).unwrap();
        assert_eq!(catalog["providers"].as_array().unwrap().len(), 5);
        let status = w
            .dispatch("credentials.status", json!({"model_provider":"ollama"}))
            .unwrap();
        assert_eq!(status["auth_kind"], "none");
        assert_eq!(status["available"], true);
        assert_eq!(status["reference"], "");
        for params in [
            json!({"model_provider":"unknown"}),
            json!({"model_provider":7}),
            json!({"model_provider":"ollama","endpoint":7}),
            json!({"model_provider":"minimax-cn","endpoint":"https://other.invalid"}),
        ] {
            assert!(w.dispatch("models.list", params).is_err());
        }
        let models = w
            .dispatch("models.list", json!({"model_provider":"minimax-cn"}))
            .unwrap();
        assert_eq!(models["inference_verified"], false);
        assert_eq!(models["source"], "pinned_catalog");
        let create = |method: &str, data: Value| {
            w.dispatch(
                method,
                json!({"command_id":id(),"payload":{"name":"provider test","data":data}}),
            )
        };
        let model=create("model.create",json!({"provider":"ollama","model":"fixture-local","context_window":32768,"max_output_tokens":4096})).unwrap();
        assert!(create("capability.create",json!({"model_ref":model["id"],"harness_ref":DEFAULT_HARNESS_ID,"tools":[DEFAULT_NATIVE_TOOL_ID],"skills":[]})).is_err());
        create("capability.create",json!({"model_ref":model["id"],"harness_ref":"builtin-harness-dsh-multi-provider:1","tools":[DEFAULT_NATIVE_TOOL_ID],"skills":[]})).unwrap();
        assert!(
            w.dispatch("query.runtime_installations", json!({}))
                .unwrap()["installations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let ollama = w
            .model_provider_facts()
            .unwrap()
            .into_iter()
            .find(|(_, p)| p.id == "ollama")
            .map(|(_, p)| p)
            .unwrap();
        assert!(w.global.model_credential_reference(&ollama).unwrap().is_none());
    }
    #[test]
    fn removed_agent_diagnostics_have_no_dispatch_or_runtime_side_effects() {
        let root = tempfile::tempdir().unwrap();
        let w =
            Workspace::open_with_adapters(root.path(), crate::test_runtime::adapters()).unwrap();
        let project = w
            .dispatch(
                "project.create",
                json!({"command_id":id(),"payload":{"title":"Agent source editing"}}),
            )
            .unwrap();
        let engine = w.active_engine(project["id"].as_str()).unwrap();
        let before = engine.store.snapshot().unwrap();
        for method in ["agent.connection.check", "agent.model.verify"] {
            let params = json!({"project_id":project["id"],"agent_revision_id":default_configured_agent_id("analyst")});
            assert!(w.model_provider_api(method, &params).is_none());
            assert!(w.dispatch(method, params.clone()).is_err());
            let mut command = params;
            command["command_id"] = json!(id());
            command["payload"] = json!({});
            assert_eq!(
                w.dispatch(method, command).unwrap_err().code,
                "method_not_found"
            );
        }
        let after = engine.store.snapshot().unwrap();
        assert_eq!(after.sessions.len(), before.sessions.len());
        assert_eq!(after.turns.len(), before.turns.len());
        assert!(w.global.runtime_installations().unwrap().is_empty());
    }
}

impl Workspace {
    pub(super) fn model_provider_api(&self, method: &str, params: &Value) -> Option<Result<Value>> {
        if !matches!(
            method,
            "query.model_providers"
                | "models.list"
                | "credentials.status"
                | "credentials.configure"
                | "credentials.check"
                | "credentials.authorize"
        ) {
            return None;
        }
        Some((|| {
            let allowed: &[&str] = match method {
                "query.model_providers" => &["project_id"],
                "models.list" => &["project_id", "model_provider", "endpoint"],
                "credentials.configure" => &["project_id", "provider", "model_provider", "api_key"],
                _ => &["project_id", "provider", "model_provider"],
            };
            require(
                params
                    .as_object()
                    .is_some_and(|p| p.keys().all(|k| allowed.contains(&k.as_str()))),
                "模型服务商请求包含未知字段",
            )?;
            if method == "query.model_providers" {
                let providers: Vec<_> = self
                    .model_provider_facts()?
                    .into_iter()
                    .map(|(reference, p)| {
                        json!({
                            "id": p.id, "reference": reference, "label": p.label,
                            "kind": p.kind, "harness": p.harness, "route": p.route,
                            "protocol": p.protocol, "endpoint": p.endpoint,
                            "auth_kind": if p.kind == ProviderKind::OAuthManaged { "oauth" }
                                else if p.requires_key { "api_key" } else { "none" },
                            "models": p.models,
                        })
                    })
                    .collect();
                return Ok(json!({"providers": providers}));
            }
            let p = self.selected_provider(params)?;
            if method == "models.list" {
                return if p.kind == ProviderKind::LocalServer {
                    devdesk_adapters::model_providers::discover_ollama_models(
                        match params.get("endpoint") {
                            None => p.endpoint.as_str(),
                            Some(Value::String(s)) => s,
                            _ => {
                                return Err(err("invalid_params", "endpoint 必须为服务地址字符串"));
                            }
                        },
                    )
                } else {
                    require(
                        params.get("endpoint").is_none(),
                        "云服务商目录不接受自定义端点",
                    )?;
                    if p.kind == ProviderKind::CloudSubscription && p.native && p.requires_key {
                        // The harness account's own provider exposes an account model directory.
                        let reference = self.global.model_credential_reference(&p)?;
                        let vault =
                            credentials::CredentialVault::new(Path::new(&self.global.root()))?;
                        let status = vault.status_model_reference(&p, reference.as_deref())?;
                        require(
                            status.configured,
                            "请先配置 DeepSeek API key，再读取账号模型目录",
                        )?;
                        return devdesk_adapters::model_providers::discover_deepseek_models(
                            Path::new(&self.global.root()),
                            &p,
                            &status.reference,
                        );
                    }
                    Ok(
                        json!({"provider":p.id,"source":"pinned_catalog","inference_verified":false,"models":p.models}),
                    )
                };
            }
            if let Some(h) = params.get("provider") {
                require(h == "dsh", "Codex 使用现有 OAuth 接口")?;
            }
            let root = PathBuf::from(self.global.root());
            let vault = credentials::CredentialVault::new(&root)?;
            if method == "credentials.configure" {
                require(params["provider"] == "dsh", "需要 provider: dsh")?;
                let secret = params["api_key"]
                    .as_str()
                    .ok_or_else(|| err("invalid_params", "需要 API key"))?;
                let status = vault.configure_model(&p, secret)?;
                self.global
                    .set_model_credential_reference(&p, &status.reference)?;
                return Ok(json!(status));
            }
            let status = vault
                .status_model_reference(&p, self.global.model_credential_reference(&p)?.as_deref())?;
            if method == "credentials.status" {
                return Ok(json!(status));
            }
            require(
                p.requires_key,
                "Ollama 免密连接请检查模型服务，无需授权凭据",
            )?;
            require(status.configured, "请先保存所选服务商凭据")?;
            vault.validate_model_reference(&p, &status.reference)?;
            if method == "credentials.authorize" {
                vault.authorize_interactively(&status.reference)?;
            }
            vault.verify_model_reference(&p, &status.reference)?;
            Ok(
                json!({"authorized":true,"model_provider":p.id,"storage":status.storage,"inference_verified":false}),
            )
        })())
    }
}
