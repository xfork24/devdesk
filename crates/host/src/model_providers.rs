use super::*;

fn selected_provider(params: &Value) -> Result<DshModelProvider> {
    match params.get("model_provider") {
        None => Ok(DshModelProvider::Deepseek),
        Some(Value::String(p)) => DshModelProvider::parse(p),
        _ => Err(err("invalid_params", "model_provider 必须为服务商标识")),
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
        assert!(
            w.global
                .model_credential_reference(DshModelProvider::Ollama)
                .unwrap()
                .is_none()
        );
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
                return Ok(model_provider_catalog());
            }
            let p = selected_provider(params)?;
            if method == "models.list" {
                return if p == DshModelProvider::Ollama {
                    devdesk_adapters::model_providers::discover_ollama_models(
                        match params.get("endpoint") {
                            None => p.endpoint(),
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
                    if p == DshModelProvider::Deepseek {
                        let reference = self.global.model_credential_reference(p)?;
                        let vault =
                            credentials::CredentialVault::new(Path::new(&self.global.root()))?;
                        let status = vault.status_model_reference(p, reference.as_deref())?;
                        require(
                            status.configured,
                            "请先配置 DeepSeek API key，再读取账号模型目录",
                        )?;
                        return devdesk_adapters::model_providers::discover_deepseek_models(
                            Path::new(&self.global.root()),
                            &status.reference,
                        );
                    }
                    Ok(
                        json!({"provider":p,"source":"pinned_catalog","inference_verified":false,"models":dsh_model_catalog(p)}),
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
                let status = vault.configure_model(p, secret)?;
                self.global
                    .set_model_credential_reference(p, &status.reference)?;
                return Ok(json!(status));
            }
            let status = vault
                .status_model_reference(p, self.global.model_credential_reference(p)?.as_deref())?;
            if method == "credentials.status" {
                return Ok(json!(status));
            }
            require(
                p.requires_key(),
                "Ollama 免密连接请检查模型服务，无需授权凭据",
            )?;
            require(status.configured, "请先保存所选服务商凭据")?;
            vault.validate_model_reference(p, &status.reference)?;
            if method == "credentials.authorize" {
                vault.authorize_interactively(&status.reference)?;
            }
            vault.verify_model_reference(p, &status.reference)?;
            Ok(
                json!({"authorized":true,"model_provider":p,"storage":status.storage,"inference_verified":false}),
            )
        })())
    }
}
