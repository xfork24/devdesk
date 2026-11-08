use super::*;
impl Workspace {
    pub(super) fn permission_api(&self, method: &str, params: &Value) -> Option<Result<Value>> {
        if method == "query.permissions" {
            return Some((|| {
                let shared = self.global.snapshot()?;
                let roles: Vec<_> = shared
                    .definitions
                    .iter()
                    .filter(|d| {
                        d.agent_profile.is_some()
                            && !shared
                                .definitions
                                .iter()
                                .any(|v| v.supersedes.as_ref() == Some(&d.id))
                    })
                    .collect();
                Ok(
                    json!({"permissions":shared.permissions,"configuration":self.global.permission_configuration()?,"roles":roles,"capabilities":[permissions::capabilities(HarnessProvider::Codex),permissions::capabilities(HarnessProvider::Dsh)]}),
                )
            })());
        }

        if ["permission.create", "permission.revise"].contains(&method) {
            return Some((|| {
                self.configuration.execute(
                    method,
                    serde_json::from_value(params.clone())
                        .map_err(|e| err("invalid_params", e.to_string()))?,
                )
            })());
        }
        if method == "permission.bind" {
            return Some((|| {
                self.global.bind_permissions(
                    &serde_json::from_value(params.clone())
                        .map_err(|e| err("invalid_params", e.to_string()))?,
                )
            })());
        }
        if method == "permission.preflight" {
            return Some((|| {
                let provider: HarnessProvider = serde_json::from_value(
                    params.get("provider").cloned().unwrap_or(json!("codex")),
                )
                .map_err(|e| err("invalid_params", e.to_string()))?;
                let overrides: Option<PermissionOverrides> = params
                    .get("overrides")
                    .map(|v| {
                        serde_json::from_value(v.clone())
                            .map_err(|e| err("invalid_params", e.to_string()))
                    })
                    .transpose()?;
                let work_type = params
                    .get("work_type")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|e| err("invalid_params", e.to_string()))?
                    .unwrap_or(WorkType::Discussion);
                let scope = ExecutionScope {
                    work_type,
                    writable_paths: vec![],
                    read_only_paths: vec![],
                };
                let project = if let Some(id) = params["project_id"].as_str() {
                    Some(self.active_engine(Some(id))?.store.snapshot()?)
                } else {
                    None
                };
                let project = project
                    .as_ref()
                    .and_then(|s| s.projects.first())
                    .and_then(|p| p.permission_overrides.as_ref());
                let configuration: PermissionConfiguration =
                    if let Some(c) = params.get("configuration") {
                        serde_json::from_value(c.clone())
                            .map_err(|e| err("invalid_params", e.to_string()))?
                    } else {
                        self.global.permission_configuration()?
                    };
                let value = permissions::resolve_execution_permissions(
                    &self.global.snapshot()?.permissions,
                    &configuration,
                    provider,
                    scope,
                    project,
                    None,
                    overrides.as_ref(),
                )?;
                Ok(
                    json!({"resolved":value,"configuration_digest":configuration_digest(&json!(value))?,"side_effects":false}),
                )
            })());
        }
        None
    }
}
