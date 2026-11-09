//! Versioned access policies. Container isolation and approval are separate concerns.
use crate::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permission {
    pub id: Id,
    pub identity: String,
    pub name: String,
    pub version: u64,
    pub supersedes: Option<Id>,
    pub schema_version: u32,
    pub policy: PermissionPolicy,
}
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
// Missing fields inherit; an explicitly supplied null is invalid.
fn present<'de, D, T>(d: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d).map(Some)
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionPatch {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub filesystem: Option<FilesystemPermission>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub network: Option<NetworkPermission>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub tools: Option<ToolPermission>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionBinding {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub permission: Option<PermissionRef>,
    #[serde(default)]
    pub overrides: PermissionPatch,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionOverrides {
    #[serde(default)]
    pub harness: PermissionBinding,
    #[serde(default)]
    pub sandbox: PermissionBinding,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionConfiguration {
    pub revision: u64,
    pub harnesses: BTreeMap<String, PermissionBinding>,
    pub sandbox: PermissionBinding,
    pub approval: ApprovalPolicy,
}
impl PermissionBinding {
    pub fn preset(name: &str) -> Self {
        Self {
            permission: Some(PermissionRef {
                revision_id: format!("permission-{name}-v1"),
            }),
            overrides: PermissionPatch::default(),
        }
    }
}
impl PermissionConfiguration {
    pub fn defaults(legacy: bool) -> Self {
        let binding = PermissionBinding::preset(if legacy {
            "legacy-restricted"
        } else {
            "container-full-access"
        });
        Self {
            revision: 1,
            harnesses: [
                ("codex".into(), binding.clone()),
                ("dsh".into(), binding.clone()),
            ]
            .into(),
            sandbox: PermissionBinding::preset("container-full-access"),
            approval: ApprovalPolicy::Never,
        }
    }
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
impl Permission {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.id.is_empty()
                && !self.identity.is_empty()
                && !self.name.trim().is_empty()
                && self.version > 0
                && self.schema_version == 1,
            "权限修订身份或 schema 无效",
        )?;
        self.policy.validate()
    }
}
pub fn resolve_permission(
    definitions: &[Permission],
    base: &PermissionBinding,
    layers: &[(&str, &PermissionBinding)],
) -> Result<ResolvedPermission> {
    let mut reference = base
        .permission
        .clone()
        .ok_or_else(|| err("invalid_params", "基础权限绑定缺少修订"))?;
    let lookup = |r: &PermissionRef| -> Result<PermissionPolicy> {
        let d = definitions
            .iter()
            .find(|d| d.id == r.revision_id)
            .ok_or_else(|| err("not_found", "Permission 修订不存在"))?;
        d.validate()?;
        Ok(d.policy.clone())
    };
    let mut policy = lookup(&reference)?;
    let mut sources = vec![format!("binding:{}", reference.revision_id)];
    for (source, binding) in std::iter::once(("binding", base)).chain(layers.iter().copied()) {
        if source != "binding" {
            if let Some(r) = &binding.permission {
                reference = r.clone();
                policy = lookup(r)?;
                sources.push(format!("{source}:{}", r.revision_id));
            }
        }
        let patch = &binding.overrides;
        if let Some(v) = &patch.filesystem {
            policy.filesystem = v.clone();
            sources.push(format!("{source}:filesystem"));
        }
        if let Some(v) = &patch.network {
            policy.network = v.clone();
            sources.push(format!("{source}:network"));
        }
        if let Some(v) = &patch.tools {
            policy.tools = v.clone();
            sources.push(format!("{source}:tools"));
        }
    }
    policy.validate()?;
    policy.canonicalize();
    Ok(ResolvedPermission {
        reference,
        policy,
        sources,
    })
}
pub fn validate_permission_revisions(current: &[Permission], before: &[Permission]) -> Result<()> {
    for old in before {
        require(
            current.iter().find(|p| p.id == old.id) == Some(old),
            "Permission 历史修订不可修改或删除",
        )?;
    }
    for (i, p) in current.iter().enumerate() {
        p.validate()?;
        require(
            !current[..i]
                .iter()
                .any(|x| x.id == p.id || (x.identity == p.identity && x.version == p.version)),
            "Permission 修订重复",
        )?;
        if let Some(old) = &p.supersedes {
            let old = current
                .iter()
                .find(|x| &x.id == old)
                .ok_or_else(|| err("blocked", "权限前序修订不存在"))?;
            require(
                old.identity == p.identity && old.version + 1 == p.version,
                "权限修订链不一致",
            )?;
        } else {
            require(p.version == 1, "首个权限修订版本必须为 1")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn definition() -> Permission {
        Permission {
            id: "full-v1".into(),
            identity: "full".into(),
            name: "Full".into(),
            version: 1,
            supersedes: None,
            schema_version: 1,
            policy: PermissionPolicy::full_access(),
        }
    }
    #[test]
    fn override_order_inherits_unspecified_fields_and_replaces_tool_lists() {
        let d = definition();
        let base = PermissionBinding {
            permission: Some(PermissionRef {
                revision_id: d.id.clone(),
            }),
            ..Default::default()
        };
        let project = PermissionBinding {
            overrides: PermissionPatch {
                network: Some(NetworkPermission::Disabled),
                tools: Some(ToolPermission::Restricted {
                    allow: vec!["bash".into()],
                    deny: vec![],
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let role = PermissionBinding {
            overrides: PermissionPatch {
                filesystem: Some(FilesystemPermission::ReadOnly),
                ..Default::default()
            },
            ..Default::default()
        };
        let explicit = PermissionBinding {
            overrides: PermissionPatch {
                network: Some(NetworkPermission::Enabled),
                tools: Some(ToolPermission::Restricted {
                    allow: vec![],
                    deny: vec![],
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = resolve_permission(
            &[d],
            &base,
            &[
                ("project", &project),
                ("role", &role),
                ("execution", &explicit),
            ],
        )
        .unwrap();
        assert_eq!(result.policy.filesystem, FilesystemPermission::ReadOnly);
        assert_eq!(result.policy.network, NetworkPermission::Enabled);
        assert!(!result.policy.tool_allowed("bash"));
        assert!(result.sources.contains(&"execution:network".into()));
    }
    #[test]
    fn invalid_null_unknown_fields_and_parent_traversal_are_rejected() {
        assert!(
            serde_json::from_value::<PermissionPatch>(serde_json::json!({"network":null})).is_err()
        );
        assert!(
            serde_json::from_value::<PermissionPatch>(serde_json::json!({"netwrok":{}})).is_err()
        );
        let mut policy = PermissionPolicy::full_access();
        policy.filesystem = FilesystemPermission::Scoped {
            rules: vec![PathPermission {
                path: "/work/../state".into(),
                access: FileAccess::ReadWrite,
            }],
        };
        assert!(policy.validate().is_err());
    }
    #[test]
    fn path_boundaries_specificity_and_equal_path_deny_are_deterministic() {
        let mut policy = PermissionPolicy::full_access();
        policy.filesystem = FilesystemPermission::Scoped {
            rules: vec![
                PathPermission {
                    path: "/work".into(),
                    access: FileAccess::ReadWrite,
                },
                PathPermission {
                    path: "/work/tests".into(),
                    access: FileAccess::Read,
                },
                PathPermission {
                    path: "/work/secret".into(),
                    access: FileAccess::ReadWrite,
                },
                PathPermission {
                    path: "/work/secret".into(),
                    access: FileAccess::Deny,
                },
            ],
        };
        policy.canonicalize();
        assert_eq!(policy.file_access("/work/tests/a"), FileAccess::Read);
        assert_eq!(policy.file_access("/work/secret/key"), FileAccess::Deny);
        assert_eq!(policy.file_access("/work-other/a"), FileAccess::Deny);
    }
    #[test]
    fn revisions_cannot_rewrite_or_remove_history() {
        let old = definition();
        let mut next = old.clone();
        next.id = "full-v2".into();
        next.version = 2;
        next.supersedes = Some(old.id.clone());
        next.policy.network = NetworkPermission::Disabled;
        validate_permission_revisions(&[old.clone(), next.clone()], std::slice::from_ref(&old))
            .unwrap();
        assert!(validate_permission_revisions(&[next], &[old]).is_err());
    }
}

fn default_protected_metadata() -> Vec<String> {
    vec![".git".into(), ".codex".into(), ".agents".into()]
}

impl PermissionBinding {
    pub fn frozen(permission: &ResolvedPermission) -> Self {
        Self {
            permission: Some(permission.reference.clone()),
            overrides: PermissionPatch {
                filesystem: Some(permission.policy.filesystem.clone()),
                network: Some(permission.policy.network.clone()),
                tools: Some(permission.policy.tools.clone()),
            },
        }
    }
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
            revision_id: "permission-isolated-tests-v1".into(),
        },
        policy,
        sources: vec![
            "isolated-tests:v1".into(),
            "execution_scope:fixed-read-only-export".into(),
        ],
    };
    ExecutionPermissions {
        binding_lock: None,
        harness: resolved.clone(),
        sandbox: resolved,
        approval: ApprovalPolicy::Never,
        scope: ExecutionScope {
            work_type: WorkType::Testing,
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
            compiler_version: "isolated-tests/v1".into(),
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
