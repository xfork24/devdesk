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
    pub model_ref: Id,
    pub harness_ref: Id,
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
                !preference.model_ref.is_empty() && !preference.harness_ref.is_empty(),
                "请选择确切的 Model 与 Harness 修订",
            )?;
        }
        Ok(())
    }

    pub fn validate_references(&self, state: &EngineeringState) -> Result<()> {
        for preference in self
            .default
            .iter()
            .chain(self.roles.iter().map(|r| &r.preference))
        {
            {
                let model_ref = &preference.model_ref;
                let harness_ref = &preference.harness_ref;
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
    fn preferences_require_exact_components_and_reject_raw_model_overrides() {
        let current = json!({"default":{"provider":"codex","model_ref":"model-r1","harness_ref":"harness-r1"},"roles":[]});
        let p: ProjectRuntimePreferences = serde_json::from_value(current.clone()).unwrap();
        p.validate().unwrap();
        assert_eq!(json!(p), current);
        let mut old = current.clone();
        old["default"]["model"] = json!("raw-model");
        assert!(serde_json::from_value::<ProjectRuntimePreferences>(old).is_err());
        let mut missing = current;
        missing["default"]
            .as_object_mut()
            .unwrap()
            .remove("model_ref");
        assert!(serde_json::from_value::<ProjectRuntimePreferences>(missing).is_err());
    }
}
