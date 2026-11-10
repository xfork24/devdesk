//! Shared runtime identities. Registering an artifact does not certify authentication or execution.
use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessProvider {
    Codex,
    Dsh,
}

impl HarnessProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Dsh => "dsh",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeKey {
    pub provider: HarnessProvider,
    /// Includes a build identity when two artifacts have the same upstream version.
    pub version: String,
    pub os: String,
    pub arch: String,
}

impl RuntimeKey {
    pub fn validate(&self) -> Result<()> {
        for field in [&self.version, &self.os, &self.arch] {
            require(
                !field.is_empty()
                    && field.len() <= 160
                    && field
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c)),
                "运行时版本、系统与架构必须是明确的标识符",
            )?;
        }
        require(
            ["linux", "macos", "windows"].contains(&self.os.as_str()),
            "不支持的运行时系统",
        )?;
        require(
            ["aarch64", "x86_64"].contains(&self.arch.as_str()),
            "不支持的运行时架构",
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeArtifact {
    /// A single native executable; a launcher script is not a complete installation.
    Executable {
        canonical_path: String,
        sha256: String,
    },
    /// A complete immutable runtime layer, identified by registry manifest digest.
    OciImage { reference: String },
    /// Immutable local Docker manifest/config identity. No mutable tag is stored in a Session lock.
    DockerImage { image_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeInstallation {
    pub id: Id,
    pub key: RuntimeKey,
    pub artifact: RuntimeArtifact,
    pub source: String,
    pub license: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeRegistration {
    pub key: RuntimeKey,
    pub artifact: RuntimeArtifact,
    pub source: String,
    pub license: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeIntegrity {
    pub installation_id: Id,
    pub verified: bool,
    pub reason: String,
    /// Integrity alone can never satisfy the live execution gate.
    pub inference_verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessPreference {
    pub provider: HarnessProvider,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_ref: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_ref: Option<Id>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub use_system_default: bool,
    pub installation_id: Option<Id>,
    pub model: Option<String>,
    /// Legacy raw-model preferences only; ignored when model_ref selects a Model revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleHarnessPreference {
    pub role: AgentRole,
    pub preference: HarnessPreference,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectRuntimePreferences {
    pub default: Option<HarnessPreference>,
    pub roles: Vec<RoleHarnessPreference>,
}
impl ProjectRuntimePreferences {
    pub fn validate(&self) -> Result<()> {
        require(self.roles.len() <= 5, "项目角色绑定超出范围")?;
        for (index, role) in self.roles.iter().enumerate() {
            require(
                !self.roles[..index].iter().any(|r| r.role == role.role),
                "角色 Harness 绑定不能重复",
            )?;
        }
        for preference in self
            .default
            .iter()
            .chain(self.roles.iter().map(|r| &r.preference))
        {
            require(
                preference.model_ref.is_some() == preference.harness_ref.is_some(),
                "Model 与 Harness 配置须一起选择",
            )?;
            if preference.use_system_default {
                require(
                    preference
                        .installation_id
                        .as_ref()
                        .is_some_and(|id| !id.trim().is_empty())
                        && preference.model.is_some(),
                    "系统默认 Harness 的安装和模型仍须明确选择",
                )?;
            }
            require(
                preference
                    .model
                    .as_ref()
                    .is_none_or(|m| !m.trim().is_empty() && m.len() <= 160),
                "模型 ID 无效",
            )?;
            if let Some(effort) = &preference.reasoning_effort {
                require(
                    preference.provider == HarnessProvider::Codex && preference.model.is_some(),
                    "思考深度需要明确选择 Codex 模型",
                )?;
                validate_reasoning_effort(effort)?;
            }
            if let Some(provider) = &preference.model_provider {
                require(
                    (preference.provider == HarnessProvider::Codex && provider == "openai")
                        || (preference.provider == HarnessProvider::Dsh
                            && DshModelProvider::parse(provider).is_ok()),
                    "模型服务商与 Harness 不兼容",
                )?;
                if let Some(model) = &preference.model {
                    let definition: ModelDefinition = serde_json::from_value(serde_json::json!({"provider":provider,"model":model,"endpoint":preference.endpoint,"context_window":preference.context_window,"max_output_tokens":preference.max_output_tokens}))
                        .map_err(|e|err("invalid_params",e.to_string()))?;
                    validate_model_definition(&definition)?;
                }
            }
        }
        Ok(())
    }

    pub fn validate_references(&self, state: &EngineeringState) -> Result<()> {
        for preference in self
            .default
            .iter()
            .chain(self.roles.iter().map(|r| &r.preference))
        {
            if let (Some(model_ref), Some(harness_ref)) =
                (&preference.model_ref, &preference.harness_ref)
            {
                let model = state.configuration(model_ref)?;
                let harness = state.configuration(harness_ref)?;
                require(
                    model.kind == "model" && harness.kind == "harness",
                    "运行时偏好必须引用 Model 和 Harness 修订",
                )?;
                let model: ModelDefinition = model.decode()?;
                let harness: HarnessDefinition = harness.decode()?;
                require(
                    harness.provider == preference.provider.as_str(),
                    "Harness 配置与偏好后端不一致",
                )?;
                require(
                    preference.model.as_deref().is_none_or(|id| id == model.model),
                    "Model 配置与偏好模型不一致",
                )?;
                resolve_model_connection(&model, &harness)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn runtime_preferences_accept_catalog_efforts_and_keep_old_defaults() {
        let mut value = json!({"default":{"provider":"codex","installation_id":null,"model":"catalog-model","model_provider":"openai"},"roles":[]});
        let old: ProjectRuntimePreferences = serde_json::from_value(value.clone()).unwrap();
        old.validate().unwrap();
        assert!(old.default.unwrap().reasoning_effort.is_none());
        for effort in ["high", "ultra", "future-effort"] {
            value["default"]["reasoning_effort"] = json!(effort);
            let preferences: ProjectRuntimePreferences =
                serde_json::from_value(value.clone()).unwrap();
            preferences.validate().unwrap();
            assert_eq!(json!(preferences), value);
        }
    }

    #[test]
    fn runtime_preferences_reject_efforts_without_a_codex_model() {
        let base = json!({"default":{"provider":"codex","model":"catalog-model","reasoning_effort":"high"},"roles":[]});
        for invalid in [json!(""), json!("high\n"), json!("a".repeat(65))] {
            let mut value = base.clone();
            value["default"]["reasoning_effort"] = invalid;
            let preferences: ProjectRuntimePreferences = serde_json::from_value(value).unwrap();
            assert!(preferences.validate().is_err());
        }
        for (field, invalid) in [("provider", json!("dsh")), ("model", Value::Null)] {
            let mut value = base.clone();
            value["default"][field] = invalid;
            let preferences: ProjectRuntimePreferences = serde_json::from_value(value).unwrap();
            assert!(preferences.validate().is_err());
        }
    }
}
