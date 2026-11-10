//! Versioned access policies. Container isolation and approval are separate concerns.
use crate::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionRef {
    pub revision_id: Id,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileAccess {
    Deny,
    Read,
    ReadWrite,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathPermission {
    pub path: String,
    pub access: FileAccess,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum FilesystemPermission {
    FullAccess,
    ReadOnly,
    Scoped { rules: Vec<PathPermission> },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkPermission {
    Enabled,
    Disabled,
    Allowlist { destinations: Vec<String> },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolPermission {
    AllAvailable,
    Restricted {
        allow: Vec<String>,
        deny: Vec<String>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionPolicy {
    pub filesystem: FilesystemPermission,
    pub network: NetworkPermission,
    pub tools: ToolPermission,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalPolicy {
    #[default]
    Never,
    OnRequest,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedPermission {
    pub reference: PermissionRef,
    pub policy: PermissionPolicy,
    pub sources: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionScope {
    pub work_type: WorkType,
    pub writable_paths: Vec<String>,
    pub read_only_paths: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxIsolation {
    #[serde(default = "default_protected_metadata")]
    pub protected_metadata: Vec<String>,
    pub read_only_root: bool,
    pub executable_tmp: bool,
    pub nested_sandbox: bool,
    pub backend: String,
    pub credential_boundary: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLimits {
    pub memory_mb: u32,
    pub cpus: u32,
    pub pids: u32,
    pub tmp_mb: u32,
}
impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            memory_mb: 1024,
            cpus: 2,
            pids: 256,
            tmp_mb: 128,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionCapabilities {
    pub backend: String,
    pub version: String,
    pub filesystem: Vec<String>,
    pub network: Vec<String>,
    pub tools: Vec<String>,
    pub limitations: Vec<String>,
}
/// Compiler-owned native options; callers override portable Permission fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePermissionOptions {
    pub provider: HarnessProvider,
    pub version: String,
    pub sandbox_mode: String,
    pub profile: String,
    pub approval_policy: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionEnforcementPlan {
    #[serde(default)]
    pub native_options: Option<NativePermissionOptions>,
    pub compiler_version: String,
    pub harness_backend: String,
    pub sandbox_backend: String,
    pub rules: BTreeMap<String, String>,
    pub asset_digests: BTreeMap<String, String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPermissions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_lock: Option<VersionedPermissionLock>,
    pub harness: ResolvedPermission,
    pub sandbox: ResolvedPermission,
    pub approval: ApprovalPolicy,
    pub scope: ExecutionScope,
    pub isolation: SandboxIsolation,
    pub limits: ResourceLimits,
    pub enforcement: PermissionEnforcementPlan,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionDecision {
    pub allowed: bool,
    pub layer: String,
    pub rule: String,
    pub reason: String,
}
impl PermissionPolicy {
    pub fn full_access() -> Self {
        Self {
            filesystem: FilesystemPermission::FullAccess,
            network: NetworkPermission::Enabled,
            tools: ToolPermission::AllAvailable,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if let FilesystemPermission::Scoped { rules } = &self.filesystem {
            require(rules.len() <= 128, "文件权限规则超出范围")?;
            for rule in rules {
                require(
                    rule.path.starts_with('/')
                        && rule.path.len() <= 4096
                        && !rule.path.contains("//")
                        && !rule.path.contains(['\\', '\n', '\r', '\0'])
                        && !rule.path.split('/').any(|p| p == ".." || p == ".")
                        && (rule.path == "/" || !rule.path.ends_with('/')),
                    "权限路径必须是规范的执行环境绝对目录",
                )?;
            }
        }
        if let NetworkPermission::Allowlist { destinations } = &self.network {
            require(
                !destinations.is_empty() && destinations.len() <= 128,
                "网络 allowlist 必须包含明确目标",
            )?;
            for d in destinations {
                require(
                    !d.is_empty()
                        && d.len() <= 253
                        && d.chars()
                            .all(|c| c.is_ascii_alphanumeric() || ".-:*".contains(c)),
                    "网络目标无效",
                )?;
            }
        }
        if let ToolPermission::Restricted { allow, deny } = &self.tools {
            require(allow.len() + deny.len() <= 256, "工具规则超出范围")?;
            for name in allow.iter().chain(deny) {
                require(
                    !name.is_empty()
                        && name.len() <= 160
                        && name
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c)),
                    "工具标识无效",
                )?;
            }
        }
        Ok(())
    }
    pub fn canonicalize(&mut self) {
        if let FilesystemPermission::Scoped { rules } = &mut self.filesystem {
            rules.sort_by(|a, b| {
                a.path
                    .cmp(&b.path)
                    .then_with(|| access_rank(a.access).cmp(&access_rank(b.access)))
            });
            rules.dedup_by(|a, b| a.path == b.path);
        }
        if let NetworkPermission::Allowlist { destinations } = &mut self.network {
            destinations.sort();
            destinations.dedup();
        }
        if let ToolPermission::Restricted { allow, deny } = &mut self.tools {
            allow.sort();
            allow.dedup();
            deny.sort();
            deny.dedup();
            allow.retain(|n| !deny.contains(n));
        }
    }
    pub fn tool_allowed(&self, name: &str) -> bool {
        match &self.tools {
            ToolPermission::AllAvailable => true,
            ToolPermission::Restricted { allow, deny } => {
                allow.iter().any(|n| n == name) && !deny.iter().any(|n| n == name)
            }
        }
    }
    pub fn file_access(&self, path: &str) -> FileAccess {
        match &self.filesystem {
            FilesystemPermission::FullAccess => FileAccess::ReadWrite,
            FilesystemPermission::ReadOnly => FileAccess::Read,
            FilesystemPermission::Scoped { rules } => rules
                .iter()
                .filter(|r| {
                    r.path == "/"
                        || path == r.path
                        || path
                            .strip_prefix(&r.path)
                            .is_some_and(|s| s.starts_with('/'))
                })
                .max_by(|a, b| {
                    a.path
                        .len()
                        .cmp(&b.path.len())
                        .then_with(|| access_rank(b.access).cmp(&access_rank(a.access)))
                })
                .map(|r| r.access)
                .unwrap_or(FileAccess::Deny),
        }
    }
}
fn access_rank(a: FileAccess) -> u8 {
    match a {
        FileAccess::Deny => 0,
        FileAccess::Read => 1,
        FileAccess::ReadWrite => 2,
    }
}
fn default_protected_metadata() -> Vec<String> {
    vec![".git".into(), ".codex".into(), ".agents".into()]
}

/// A test process has no model connection and receives a fixed read-only code export.
pub fn isolated_test_permissions() -> ExecutionPermissions {
    let policy = PermissionPolicy {
        filesystem: FilesystemPermission::ReadOnly,
        network: NetworkPermission::Disabled,
        tools: ToolPermission::AllAvailable,
    };
    let resolved = ResolvedPermission {
        reference: PermissionRef {
            revision_id: "system-isolated-tests-v2".into(),
        },
        policy,
        sources: vec![
            "isolated-tests:v2".into(),
            "execution_scope:fixed-read-only-export".into(),
        ],
    };
    ExecutionPermissions {
        binding_lock: None,
        harness: resolved.clone(),
        sandbox: resolved,
        approval: ApprovalPolicy::Never,
        scope: ExecutionScope {
            work_type: WorkType::Workflow,
            writable_paths: vec![],
            read_only_paths: vec![".".into()],
        },
        isolation: SandboxIsolation {
            protected_metadata: default_protected_metadata(),
            read_only_root: true,
            executable_tmp: false,
            nested_sandbox: false,
            backend: "docker-default-v1".into(),
            credential_boundary: "credential-free".into(),
        },
        limits: ResourceLimits::default(),
        enforcement: PermissionEnforcementPlan {
            native_options: None,
            compiler_version: "isolated-tests/v2".into(),
            harness_backend: "node-test".into(),
            sandbox_backend: "docker-default-v1".into(),
            rules: [
                ("filesystem".into(), "docker-read-only-export".into()),
                ("network".into(), "docker-none".into()),
            ]
            .into(),
            asset_digests: BTreeMap::new(),
        },
    }
}
