//! Shared runtime identities. Registering an artifact does not certify authentication or execution.
use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessProvider {
    Codex,
    Dsh,
}

/// The complete harness provider registry; provider gates read this, never literals.
pub const HARNESS_PROVIDERS: &[HarnessProvider] = &[HarnessProvider::Codex, HarnessProvider::Dsh];

pub fn known_harness_provider(name: &str) -> bool {
    HARNESS_PROVIDERS.iter().any(|p| p.as_str() == name)
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
