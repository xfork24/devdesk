//! Versioned permission profiles and exact runtime bindings.
use crate::*;
use serde_json::json;

/// JSON objects are ordered identically for typed values, imported files and stored records.
pub fn permission_content_hash<T: Serialize>(value: &T) -> String {
    configuration_hash(&serde_json::to_value(value).expect("serializable permission"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionTargetKind {
    Harness,
    Sandbox,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionTargetRef {
    pub kind: PermissionTargetKind,
    pub revision_id: Id,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxDefinition {
    pub provider: String,
    pub runtime_version: String,
    pub enforcement: String,
    pub platforms: Vec<String>,
}

fn supplied<'de, D, T: Deserialize<'de>>(d: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    T::deserialize(d).map(Some)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfilePolicy {
    pub filesystem: FilesystemPermission,
    pub network: NetworkPermission,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "supplied"
    )]
    pub tools: Option<ToolPermission>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "supplied"
    )]
    pub approval: Option<ApprovalPolicy>,
}
impl ProfilePolicy {
    pub fn portable(&self) -> PermissionPolicy {
        PermissionPolicy {
            filesystem: self.filesystem.clone(),
            network: self.network.clone(),
            tools: self.tools.clone().unwrap_or(ToolPermission::AllAvailable),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionProfile {
    pub schema_version: u32,
    pub id: Id,
    pub identity: String,
    pub name: String,
    pub version: u64,
    pub supersedes: Option<Id>,
    pub target_kind: PermissionTargetKind,
    pub policy: ProfilePolicy,
}
impl PermissionProfile {
    pub fn normalize(&mut self) -> Result<()> {
        if self.schema_version != 2
            || self.version == 0
            || self.id.is_empty()
            || self.id.len() > 200
            || self.identity.is_empty()
            || self.identity.len() > 200
            || self.name.trim().is_empty()
            || self.name.len() > 500
        {
            return Err(err("invalid_params", "无效 Permission v2 身份或版本"));
        }
        match self.target_kind {
            PermissionTargetKind::Harness
                if self.policy.tools.is_none() || self.policy.approval.is_none() =>
            {
                return Err(err(
                    "invalid_params",
                    "Harness profile 必须包含 tools 和 approval",
                ));
            }
            PermissionTargetKind::Sandbox
                if self.policy.tools.is_some() || self.policy.approval.is_some() =>
            {
                return Err(err(
                    "permission_inapplicable",
                    "Sandbox profile 不接受 tools 或 approval",
                ));
            }
            _ => {}
        }
        let mut policy = self.policy.portable();
        policy.validate()?;
        policy.canonicalize();
        self.policy.filesystem = policy.filesystem;
        self.policy.network = policy.network;
        if self.policy.tools.is_some() {
            self.policy.tools = Some(policy.tools);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum PermissionSelection {
    FollowTarget,
    Pinned { binding_revision_id: Id },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentPermissionSelection {
    pub sandbox_ref: Id,
    pub harness: PermissionSelection,
    pub sandbox: PermissionSelection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionedPermissionBinding {
    pub target: PermissionTargetRef,
    pub permission_revision_id: Id,
    pub enforcement_contract: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundPermission {
    pub binding_revision_id: Id,
    pub binding_hash: String,
    pub binding: VersionedPermissionBinding,
    pub profile: PermissionProfile,
    pub profile_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionedPermissionLock {
    pub harness: BoundPermission,
    pub sandbox: BoundPermission,
}

pub fn permission_target(
    s: &EngineeringState,
    kind: PermissionTargetKind,
    id: &str,
) -> Result<PermissionTargetRef> {
    let r = s.configuration(id)?;
    let expected = match kind {
        PermissionTargetKind::Harness => "harness",
        PermissionTargetKind::Sandbox => "sandbox",
    };
    if r.kind != expected || !r.immutable || r.content_hash != configuration_hash(&r.data) {
        return Err(err(
            "invalid_configuration",
            "权限目标不是匹配的不可变资源修订",
        ));
    }
    if s.governance
        .iter()
        .any(|v| v.kind == "configuration_retirement" && v.definition_id == id)
    {
        return Err(err("invalid_configuration", "权限目标已停用"));
    }
    Ok(PermissionTargetRef {
        kind,
        revision_id: id.into(),
        content_hash: r.content_hash.clone(),
    })
}

pub fn active_permission_binding<'a>(
    s: &'a EngineeringState,
    target: &PermissionTargetRef,
) -> Option<&'a ConfigurationRecord> {
    s.governance
        .iter()
        .filter(|r| r.kind == "permission_assignment" && r.data["target"] == json!(target))
        .max_by_key(|r| r.version)
}

pub fn resolve_bound_permission(
    s: &EngineeringState,
    target: PermissionTargetRef,
    selection: &PermissionSelection,
) -> Result<BoundPermission> {
    let id = match selection {
        PermissionSelection::Pinned {
            binding_revision_id,
        } => binding_revision_id.clone(),
        PermissionSelection::FollowTarget => active_permission_binding(s, &target)
            .and_then(|r| r.data["binding_revision_id"].as_str())
            .map(str::to_owned)
            .ok_or_else(|| {
                err(
                    "permission_unconfigured",
                    format!("目标 {} 未分配权限", target.revision_id),
                )
            })?,
    };
    let r = s.configuration(&id)?;
    if r.kind != "permission_binding" || !r.immutable {
        return Err(err("invalid_configuration", "需要确切权限绑定修订"));
    }
    let binding: VersionedPermissionBinding = r.decode()?;
    if binding.target != target {
        return Err(err("permission_conflict", "固定绑定与目标修订或摘要不匹配"));
    }
    let p = s.configuration(&binding.permission_revision_id)?;
    let scope = &s.configuration(&target.revision_id)?.scope;
    if !r.scope.visible_from(scope) || !p.scope.visible_from(scope) {
        return Err(err("invalid_scope", "权限绑定或 profile 对目标不可见"));
    }
    if p.kind != "permission_profile" || !p.immutable {
        return Err(err(
            "invalid_configuration",
            "需要确切 Permission profile 修订",
        ));
    }
    let mut profile: PermissionProfile = p.decode()?;
    profile.normalize()?;
    if profile.id != p.id
        || profile.target_kind != target.kind
        || permission_content_hash(&profile) != p.content_hash
    {
        return Err(err("permission_conflict", "profile 目标或内容摘要不匹配"));
    }
    Ok(BoundPermission {
        binding_revision_id: id,
        binding_hash: r.content_hash.clone(),
        binding,
        profile,
        profile_hash: p.content_hash.clone(),
    })
}

pub fn resolve_permission_lock(
    s: &EngineeringState,
    harness: &str,
    selection: &AgentPermissionSelection,
) -> Result<VersionedPermissionLock> {
    Ok(VersionedPermissionLock {
        harness: resolve_bound_permission(
            s,
            permission_target(s, PermissionTargetKind::Harness, harness)?,
            &selection.harness,
        )?,
        sandbox: resolve_bound_permission(
            s,
            permission_target(s, PermissionTargetKind::Sandbox, &selection.sandbox_ref)?,
            &selection.sandbox,
        )?,
    })
}

pub fn agent_permission_selection(
    _s: &EngineeringState,
    _agent_id: &str,
    agent: &AgentComposition,
) -> Option<AgentPermissionSelection> {
    agent.permission_selection.clone()
}

pub fn validate_frozen_permissions(s: &EngineeringState, p: &ExecutionPermissions) -> Result<()> {
    let lock = p
        .binding_lock
        .as_ref()
        .ok_or_else(|| err("permission_unconfigured", "执行必须冻结 v2 权限绑定"))?;
    for (bound, kind) in [
        (&lock.harness, PermissionTargetKind::Harness),
        (&lock.sandbox, PermissionTargetKind::Sandbox),
    ] {
        let r = s.configuration(&bound.binding_revision_id)?;
        let profile = s.configuration(&bound.profile.id)?;
        let target = s.configuration(&bound.binding.target.revision_id)?;
        let mut checked = bound.profile.clone();
        checked.normalize()?;
        require(
            checked == bound.profile
                && r.kind == "permission_binding"
                && r.immutable
                && profile.kind == "permission_profile"
                && profile.immutable
                && r.data == serde_json::to_value(&bound.binding).unwrap()
                && r.content_hash == bound.binding_hash
                && r.content_hash == permission_content_hash(&bound.binding)
                && profile.data == serde_json::to_value(&bound.profile).unwrap()
                && profile.content_hash == bound.profile_hash
                && profile.content_hash == permission_content_hash(&bound.profile)
                && bound.profile.target_kind == kind
                && bound.binding.target.kind == kind
                && bound.binding.permission_revision_id == bound.profile.id
                && target.immutable
                && target.content_hash == bound.binding.target.content_hash
                && target.content_hash == configuration_hash(&target.data)
                && target.kind
                    == if kind == PermissionTargetKind::Harness {
                        "harness"
                    } else {
                        "sandbox"
                    },
            "冻结 v2 权限引用或摘要不一致",
        )?;
    }
    require(
        p.harness.reference.revision_id == lock.harness.profile.id
            && p.sandbox.reference.revision_id == lock.sandbox.profile.id
            && p.harness.policy == lock.harness.profile.policy.portable()
            && p.sandbox.policy == lock.sandbox.profile.policy.portable()
            && Some(p.approval) == lock.harness.profile.policy.approval,
        "执行权限与冻结 profile 不一致",
    )
}
