//! Versioned integration contracts. No network, filesystem or provider calls.
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

fn timeout() -> u64 {
    30_000
}
fn limit() -> usize {
    1_048_576
}
fn protocol() -> String {
    "2025-11-25".into()
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum IntegrationSource {
    HttpApi {
        base_url: String,
    },
    Mcp {
        transport: String,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        command: Vec<String>,
        #[serde(default = "protocol")]
        protocol_version: String,
    },
    SkillPackage {
        #[serde(default)]
        source: String,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegrationDefinition {
    pub source: IntegrationSource,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<OAuthConfig>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthConfig {
    pub client_id: String,
    pub authorization_url: String,
    pub token_url: String,
    #[serde(default)]
    pub revocation_url: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}
fn endpoint(value: &str) -> Result<url::Url> {
    let url = url::Url::parse(value).map_err(|_| err("invalid_configuration", "无效服务 URL"))?;
    require(
        ["https", "http"].contains(&url.scheme())
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "服务 URL 只允许无内嵌凭据的 HTTP(S) 地址",
    )?;
    Ok(url)
}
impl IntegrationDefinition {
    pub fn validate(&self) -> Result<()> {
        match &self.source {
            IntegrationSource::HttpApi { base_url } => {
                require(
                    endpoint(base_url)?.query().is_none(),
                    "Base URL 不接受 query，请在动作中映射参数",
                )?;
            }
            IntegrationSource::Mcp {
                transport,
                url,
                command,
                protocol_version,
            } => {
                require(
                    ["2025-11-25", "2026-07-28"].contains(&protocol_version.as_str()),
                    "不支持的 MCP 协议版本",
                )?;
                if transport == "streamable_http" {
                    endpoint(
                        url.as_deref()
                            .ok_or_else(|| err("invalid_configuration", "MCP URL 缺失"))?,
                    )?;
                    require(command.is_empty(), "HTTP MCP 不使用本地命令")?;
                } else {
                    require(
                        transport == "stdio"
                            && url.is_none()
                            && !command.is_empty()
                            && command.len() <= 128
                            && command
                                .iter()
                                .all(|s| s.len() <= 131072 && !s.contains('\0')),
                        "无效 MCP 命令或传输",
                    )?;
                    let executable = command[0].rsplit('/').next().unwrap_or("");
                    require(
                        !["npx", "npm", "pnpm", "yarn", "uvx", "pip", "pip3", "bunx"]
                            .contains(&executable),
                        "请将服务包固定到 Harness 镜像后运行，不能在调用时安装包",
                    )?;
                }
            }
            IntegrationSource::SkillPackage { .. } => {}
        }
        if let Some(oauth) = &self.oauth {
            require(!oauth.client_id.trim().is_empty(), "缺少 OAuth client_id")?;
            for value in std::iter::once(&oauth.authorization_url)
                .chain(std::iter::once(&oauth.token_url))
                .chain(oauth.revocation_url.iter())
            {
                endpoint(value)?;
            }
        }
        Ok(())
    }
}
impl IntegrationBinding {
    pub fn validate_origins(&self) -> Result<()> {
        for value in &self.allowed_origins {
            let url = endpoint(value)?;
            require(
                url.origin().ascii_serialization() == *value,
                "允许目标必须是规范化的 origin，不能包含 path 或 query",
            )?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum IntegrationAuth {
    None,
    #[serde(rename = "oauth_pending")]
    OAuthPending {
        account: String,
        scopes: Vec<String>,
    },
    Bearer {
        credential_ref: String,
    },
    ApiKey {
        credential_ref: String,
        header: String,
    },
    #[serde(rename = "oauth")]
    OAuth {
        credential_ref: String,
        account: String,
        scopes: Vec<String>,
    },
}
impl Default for IntegrationAuth {
    fn default() -> Self {
        Self::None
    }
}
impl IntegrationAuth {
    pub fn credential_ref(&self) -> Option<&str> {
        match self {
            Self::None | Self::OAuthPending { .. } => None,
            Self::Bearer { credential_ref }
            | Self::ApiKey { credential_ref, .. }
            | Self::OAuth { credential_ref, .. } => Some(credential_ref),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegrationBinding {
    pub integration_ref: Id,
    #[serde(default)]
    pub auth: IntegrationAuth,
    pub harness_ref: Id,
    pub sandbox_ref: Id,
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    #[serde(default)]
    pub allow_private_network: bool,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default = "timeout")]
    pub timeout_ms: u64,
    #[serde(default = "limit")]
    pub max_result_bytes: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpParameter {
    pub pointer: String,
    pub location: String,
    pub name: String,
    #[serde(default)]
    pub required: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolImplementation {
    HttpApi {
        method: String,
        path: String,
        #[serde(default)]
        parameters: Vec<HttpParameter>,
        #[serde(default)]
        body_pointer: Option<String>,
        #[serde(default)]
        idempotency_header: Option<String>,
    },
    Mcp {
        name: String,
        descriptor_hash: String,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalTool {
    pub schema_version: u32,
    pub qualified_name: String,
    pub description: String,
    pub integration_ref: Id,
    pub implementation: ToolImplementation,
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    pub effect: String,
    #[serde(default)]
    pub constraints: BTreeMap<String, Value>,
    #[serde(default)]
    pub sensitive_parameters: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_auth: Option<ToolAuthRequirement>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolAuthRequirement {
    None,
    Bearer,
    ApiKey {
        header: String,
    },
    #[serde(rename = "oauth")]
    OAuth {
        scopes: Vec<String>,
    },
}
impl ToolAuthRequirement {
    pub fn satisfied_by(&self, auth: &IntegrationAuth) -> bool {
        match (self, auth) {
            (Self::None, _) => true,
            (Self::Bearer, IntegrationAuth::Bearer { .. } | IntegrationAuth::OAuth { .. }) => true,
            (Self::ApiKey { header: a }, IntegrationAuth::ApiKey { header: b, .. }) => {
                a.eq_ignore_ascii_case(b)
            }
            (
                Self::OAuth { scopes },
                IntegrationAuth::OAuth {
                    scopes: granted, ..
                },
            ) => scopes.iter().all(|s| granted.contains(s)),
            _ => false,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillFile {
    pub path: String,
    pub content: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillPackage {
    pub body: String,
    pub description: String,
    #[serde(default)]
    pub files: Vec<SkillFile>,
    #[serde(default)]
    pub requires_tools: Vec<Id>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub scripts_disabled: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundTool {
    pub revision_id: Id,
    pub content_hash: String,
    pub definition: ExternalTool,
    pub integration: IntegrationDefinition,
    pub integration_hash: String,
    pub binding_ref: Id,
    pub binding: IntegrationBinding,
    pub binding_hash: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationLock {
    #[serde(default)]
    pub tools: Vec<BoundTool>,
    #[serde(default)]
    pub skills: BTreeMap<Id, SkillPackage>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub id: Id,
    pub project_id: Option<Id>,
    pub feature_id: Option<Id>,
    pub task_id: Option<Id>,
    pub agent_instance_id: Option<Id>,
    pub session_id: Option<Id>,
    pub turn_id: Option<Id>,
    pub run_id: Option<Id>,
    pub operation_id: Option<Id>,
    pub request_id: Option<Value>,
    pub call_id: String,
    pub tool: BoundTool,
    pub arguments: Value,
    pub permissions: ExecutionPermissions,
    pub image_id: String,
    pub worktree: String,
    pub input_hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolReceipt {
    pub invocation_id: Id,
    pub success: bool,
    pub content: Vec<Value>,
    #[serde(default)]
    pub structured_content: Option<Value>,
    pub remote_effect: String,
    #[serde(default)]
    pub error: Option<Value>,
    pub origin: Origin,
}
impl ExternalTool {
    pub fn validate(&self) -> Result<()> {
        require(self.schema_version == 2, "工具需要 schema_version=2")?;
        require(
            !self.qualified_name.is_empty()
                && self.qualified_name.len() <= 64
                && self
                    .qualified_name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
            "工具名称只能包含字母、数字、下划线和连字符，最多 64 字符",
        )?;
        require(
            !self.description.trim().is_empty() && !self.integration_ref.is_empty(),
            "工具需要说明与确切接入源",
        )?;
        require(self.input_schema.is_object(), "工具输入需要对象 Schema")?;
        require(
            ["read_only", "write", "destructive", "unknown"].contains(&self.effect.as_str()),
            "无效工具影响类型",
        )?;
        if let ToolImplementation::HttpApi {
            method,
            path,
            parameters,
            idempotency_header,
            ..
        } = &self.implementation
        {
            require(
                ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"].contains(&method.as_str())
                    && path.starts_with('/')
                    && !path.starts_with("//")
                    && !path.contains(['#', '?', '\\']),
                "无效 HTTP method 或相对 path",
            )?;
            require(
                self.effect != "read_only" || ["GET", "HEAD"].contains(&method.as_str()),
                "写请求不能声明为只读动作",
            )?;
            let mut keys = BTreeSet::new();
            for p in parameters {
                require(
                    p.pointer.starts_with('/')
                        && ["path", "query", "header"].contains(&p.location.as_str())
                        && !p.name.is_empty()
                        && keys.insert((p.location.clone(), p.name.to_lowercase())),
                    "参数映射无效或重复",
                )?;
                require(
                    ![
                        "authorization",
                        "host",
                        "cookie",
                        "connection",
                        "content-length",
                        "transfer-encoding",
                        "proxy-authorization",
                    ]
                    .contains(&p.name.to_lowercase().as_str()),
                    "参数不得覆盖认证或协议 Header",
                )?;
            }
            if let Some(h) = idempotency_header {
                require(
                    !h.is_empty() && h.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
                    "无效幂等 Header",
                )?;
            }
        }
        Ok(())
    }
}
impl SkillPackage {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.body.trim().is_empty() && self.body.len() <= 262_144 && self.files.len() <= 128,
            "Skill 正文为空或包过大",
        )?;
        let mut paths = BTreeSet::new();
        let mut size = self.body.len();
        for f in &self.files {
            require(
                !f.path.contains('\0')
                    && !f.path.starts_with('/')
                    && !f.path.contains('\\')
                    && f.path
                        .split('/')
                        .all(|s| !s.is_empty() && s != "." && s != "..")
                    && paths.insert(&f.path),
                "Skill 文件越界或重复",
            )?;
            require(
                !f.path.to_ascii_lowercase().starts_with("scripts/")
                    && ![
                        "sh", "py", "js", "mjs", "cjs", "ts", "ps1", "bat", "cmd", "exe",
                    ]
                    .contains(
                        &f.path
                            .rsplit('.')
                            .next()
                            .unwrap_or("")
                            .to_ascii_lowercase()
                            .as_str(),
                    ),
                "脚本不能作为可执行 Skill 材料导入；请注册受控工具",
            )?;
            size += f.content.len();
        }
        require(size <= 2_097_152, "Skill 材料总量超限")
    }
}
