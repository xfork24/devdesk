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
        create("capability.create",json!({"model_ref":model["id"],"harness_ref":"builtin-harness-dsh-multi-provider-v1","tools":[DEFAULT_NATIVE_TOOL_ID],"skills":[]})).unwrap();
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
                | "agent.connection.check"
                | "agent.model.verify"
        ) {
            return None;
        }
        Some((|| {
            let allowed: &[&str] = match method {
                "query.model_providers" => &["project_id"],
                "models.list" => &["project_id", "model_provider", "endpoint"],
                "agent.connection.check" | "agent.model.verify" => {
                    &["project_id", "agent_revision_id"]
                }
                "credentials.configure" => &["project_id", "provider", "model_provider", "api_key"],
                _ => &["project_id", "provider", "model_provider"],
            };
            require(
                params
                    .as_object()
                    .is_some_and(|p| p.keys().all(|k| allowed.contains(&k.as_str()))),
                "模型服务商请求包含未知字段",
            )?;
            if method.starts_with("agent.") {
                let e = self.active_engine(params["project_id"].as_str())?;
                self.sync_configuration(&e)?;
                let state = e.store.snapshot()?;
                let project = state
                    .projects
                    .first()
                    .ok_or_else(|| err("project_required", "请先打开项目"))?;
                let agent = params["agent_revision_id"]
                    .as_str()
                    .ok_or_else(|| err("invalid_params", "需要完整 Agent 修订"))?;
                if method == "agent.connection.check" {
                    let a: AgentComposition = state.configuration(agent)?.decode()?;
                    let h: HarnessDefinition = state.configuration(&a.harness_ref)?.decode()?;
                    require(
                        h.provider == "dsh",
                        "此元数据检查仅用于 DSH；Codex 请使用现有认证或实际 Agent 验证",
                    )?;
                }
                let opened = self.dispatch("session.open", json!({"project_id":project.id,"command_id":id(),"target":project.id,"expected_revision":project.revision,
                    "payload":{"agent":agent,"work_type":"discussion","force_new":true}}))?;
                let session = opened["id"]
                    .as_str()
                    .ok_or_else(|| err("invalid_configuration", "缺少验证会话"))?;
                let snapshot = e.store.snapshot()?;
                let s = snapshot
                    .sessions
                    .iter()
                    .find(|s| s.id == session)
                    .ok_or_else(|| err("not_found", "验证会话不存在"))?;
                if method == "agent.connection.check" {
                    let binding = s
                        .runtime_binding
                        .as_ref()
                        .ok_or_else(|| err("invalid_configuration", "验证会话缺少绑定"))?;
                    let mut checked =
                        devdesk_adapters::model_providers::check_container_connection(
                            Path::new(&self.global.root()),
                            binding,
                            &project.id,
                            session,
                        )?;
                    checked["session_id"] = json!(session);
                    return Ok(checked);
                }
                let marker = format!("devdesk-model-check-{}", id());
                let turn = self.dispatch("conversation.send", json!({"project_id":project.id,"command_id":id(),"target":session,"expected_revision":s.revision,
                    "payload":{"client_message_id":id(),"input":format!("Verify this configured model through the native Harness. Use the bash tool once to print the marker {marker}, then reply with that exact marker. Do not modify files.")}}))?;
                return Ok(
                    json!({"session_id":session,"turn_id":turn["id"],"marker":marker,"status":"submitted","inference_verified":false}),
                );
            }
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
