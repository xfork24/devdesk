//! Shared runtime identities. Registering an artifact does not certify authentication or execution.
use crate::*;

/// Public runtime scratch/cache mount; source files remain under /work.
pub const WORKSPACE_RUNTIME_DIRECTORY: &str = "/tmp/devdesk-runtime";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessProvider {
    Codex,
    Dsh,
    Pi,
}

/// Compiled providers. Factory sources and the host driver registry validate availability.
pub const HARNESS_PROVIDERS: &[HarnessProvider] = &[
    HarnessProvider::Codex,
    HarnessProvider::Dsh,
    HarnessProvider::Pi,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputIdentity {
    ProviderTurn,
    ProviderMessage,
    HostTurn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolTransport {
    ProtocolRequest,
    UiDialog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessTraits {
    pub identity: InputIdentity,
    pub owns_history: bool,
    pub streams: bool,
    pub protocol_interrupt: bool,
    pub host_model_route: bool,
    pub tool_transport: ToolTransport,
    pub command_output: bool,
    pub orchestration: bool,
}

pub fn known_harness_provider(name: &str) -> bool {
    HarnessProvider::from_name(name).is_some()
}

impl HarnessProvider {
    pub fn from_name(name: &str) -> Option<Self> {
        HARNESS_PROVIDERS
            .iter()
            .copied()
            .find(|p| p.as_str() == name)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Dsh => "dsh",
            Self::Pi => "pi",
        }
    }

    pub const fn traits(self) -> HarnessTraits {
        match self {
            Self::Codex => HarnessTraits {
                identity: InputIdentity::ProviderTurn,
                owns_history: true,
                streams: true,
                protocol_interrupt: true,
                host_model_route: false,
                tool_transport: ToolTransport::ProtocolRequest,
                command_output: true,
                orchestration: true,
            },
            Self::Dsh => HarnessTraits {
                identity: InputIdentity::ProviderMessage,
                owns_history: false,
                streams: false,
                protocol_interrupt: false,
                host_model_route: true,
                tool_transport: ToolTransport::ProtocolRequest,
                command_output: false,
                orchestration: false,
            },
            Self::Pi => HarnessTraits {
                identity: InputIdentity::HostTurn,
                owns_history: true,
                streams: true,
                protocol_interrupt: true,
                host_model_route: true,
                tool_transport: ToolTransport::UiDialog,
                command_output: false,
                orchestration: false,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeKey {
    pub provider: HarnessProvider,
    /// Display/selection metadata only; content provenance uses recipe_digest/artifact.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_digest: Option<String>,
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
