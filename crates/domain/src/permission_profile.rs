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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<SandboxBuild>,
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "supplied"
    )]
    pub platform: Option<PlatformPermission>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "supplied"
    )]
    pub tool_refs: Option<Vec<Id>>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionProfile {
    pub scope: Scope,
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
        if self.version == 0
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
                if self.policy.tools.is_some()
                    || self.policy.approval.is_some()
                    || self.policy.platform.is_some()
                    || self.policy.tool_refs.is_some() =>
            {
                return Err(err(
                    "permission_inapplicable",
                    "Sandbox profile 不接受 tools 或 approval",
                ));
            }
            _ => {}
        }
        if let Some(tools) = &self.policy.tool_refs {
            require(tools.len() <= 256, "Profile 工具引用超过数量边界")?;
            let mut unique = std::collections::BTreeSet::new();
            for tool in tools {
                parse_source_reference(tool)?;
                require(unique.insert(tool), "Profile 工具引用重复")?;
            }
        }
        if let Some(platform) = &self.policy.platform {
            WorkflowControlPolicy {
                allowed_operations: platform.allowed_operations.clone(),
                human_operations: platform.human_operations.clone(),
                waivable_requirements: platform.waivable_requirements.clone(),
                allow_task_revision: platform.allow_task_revision,
                max_turns_per_wakeup: 8,
                max_tool_calls_per_turn: 64,
                timeout_seconds: 600,
            }
            .validate()?;
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
    Profile { permission_revision_id: Id },
    Pinned { binding_revision_id: Id },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPermissionSelection {
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
    if r.r#type != expected || !r.immutable || r.content_hash != content_digest(&r.refs, &r.data) {
        return Err(err(
            "invalid_configuration",
            "权限目标不是匹配的不可变资源修订",
        ));
    }
    if s.governance
        .iter()
        .any(|v| v.r#type == "configuration_retirement" && v.definition_id == id)
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
        .filter(|r| r.r#type == "permission_assignment" && r.data["target"] == json!(target))
        .max_by_key(|r| r.version)
}

pub fn resolve_bound_permission(
    s: &EngineeringState,
    target: PermissionTargetRef,
    selection: &PermissionSelection,
) -> Result<BoundPermission> {
    if let PermissionSelection::Profile {
        permission_revision_id,
    } = selection
    {
        let row = s.configuration(permission_revision_id)?;
        let profile = row.permission_profile()?;
        // Explicit selection is checked against the consuming Workflow/project
        // scope at admission. A project Profile may restrict a global target
        // without changing that target's global default binding.
        require(
            row.immutable && profile.target_kind == target.kind,
            "Profile 类型或作用域不匹配",
        )?;
        let contract = format!("direct-profile:{}", target.content_hash);
        let binding = VersionedPermissionBinding {
            target,
            permission_revision_id: row.id.clone(),
            enforcement_contract: contract,
        };
        return Ok(BoundPermission {
            binding_revision_id: row.id.clone(),
            binding_hash: content_digest(&References::new(), &json!(binding)),
            binding,
            profile,
            profile_hash: row.content_hash.clone(),
        });
    }
    let id = match selection {
        PermissionSelection::Profile { .. } => unreachable!(),
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
    if r.r#type != "permission_binding" || !r.immutable {
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
    if p.r#type != "permission_profile" || !p.immutable {
        return Err(err(
            "invalid_configuration",
            "需要确切 Permission profile 修订",
        ));
    }
    let mut profile = p.permission_profile()?;
    profile.normalize()?;
    if profile.id != p.id
        || profile.target_kind != target.kind
        || content_digest(&p.refs, &permission_profile_data(&profile)) != p.content_hash
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
    selection: &ExecutionPermissionSelection,
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

pub fn selected_harness_profile(s: &EngineeringState) -> Option<&ConfigurationRecord> {
    let project = s
        .governance
        .iter()
        .filter(|r| r.r#type == "project_configuration")
        .max_by_key(|r| r.version);
    let defaults = template_catalog_state(s).map(|r| &r.data["defaults"]);
    let explicit = project
        .and_then(|r| r.data["harness_permission_ref"].as_str())
        .filter(|id| !id.is_empty());
    let followed = defaults
        .and_then(|d| d["harness_ref"].as_str())
        .and_then(|id| permission_target(s, PermissionTargetKind::Harness, id).ok())
        .and_then(|target| active_permission_binding(s, &target))
        .and_then(|r| r.data["binding_revision_id"].as_str())
        .and_then(|id| s.configuration(id).ok())
        .and_then(|r| r.data["permission_revision_id"].as_str());
    explicit
        .or(followed)
        .or_else(|| defaults.and_then(|d| d["harness_permission_ref"].as_str()))
        .and_then(|id| s.configuration(id).ok())
}
pub fn permission_selection(s: &EngineeringState) -> Result<ExecutionPermissionSelection> {
    let project = s
        .governance
        .iter()
        .filter(|r| r.r#type == "project_configuration")
        .max_by_key(|r| r.version);
    let defaults = template_catalog_state(s).map(|r| &r.data["defaults"]);
    let selected = |key: &str| {
        project
            .and_then(|r| r.data[key].as_str())
            .or_else(|| defaults.and_then(|d| d[key].as_str()))
    };
    Ok(ExecutionPermissionSelection {
        sandbox_ref: selected("sandbox_ref")
            .ok_or_else(|| err("permission_unconfigured", "请选择 Sandbox"))?
            .into(),
        harness: project
            .and_then(|r| r.data["harness_permission_ref"].as_str())
            .map_or(PermissionSelection::FollowTarget, |id| {
                PermissionSelection::Profile {
                    permission_revision_id: id.into(),
                }
            }),
        sandbox: project
            .and_then(|r| r.data["sandbox_permission_ref"].as_str())
            .map_or(PermissionSelection::FollowTarget, |id| {
                PermissionSelection::Profile {
                    permission_revision_id: id.into(),
                }
            }),
    })
}
pub fn permission_selection_with_profile(
    s: &EngineeringState,
    reference: &str,
) -> Result<ExecutionPermissionSelection> {
    let mut selection = permission_selection(s)?;
    require(
        s.configuration(reference)?
            .permission_profile()?
            .target_kind
            == PermissionTargetKind::Harness,
        "需要 Harness Profile",
    )?;
    selection.harness = PermissionSelection::Profile {
        permission_revision_id: reference.into(),
    };
    Ok(selection)
}
pub fn step_permission_selection(
    s: &EngineeringState,
    step: &StepDefinition,
) -> Result<ExecutionPermissionSelection> {
    let mut selection = permission_selection(s)?;
    if let Some(id) = &step.harness_permission_ref {
        selection.harness = PermissionSelection::Profile {
            permission_revision_id: id.clone(),
        };
    }
    if let Some(id) = &step.sandbox_permission_ref {
        selection.sandbox = PermissionSelection::Profile {
            permission_revision_id: id.clone(),
        };
    }
    Ok(selection)
}
/// Derived display/mount scope; no caller supplied second permission policy.
pub fn profile_workspace_access(profile: &PermissionProfile) -> (Vec<String>, Vec<String>) {
    match &profile.policy.filesystem {
        FilesystemPermission::FullAccess => (vec![".".into()], vec![]),
        FilesystemPermission::ReadOnly => (vec![], vec![".".into()]),
        FilesystemPermission::Scoped { rules } => {
            let mut write = vec![];
            let mut read = vec![];
            for rule in rules {
                if let Some(path) = if rule.path == "/work" {
                    Some(".")
                } else {
                    rule.path.strip_prefix("/work/")
                } {
                    if rule.access == FileAccess::ReadWrite {
                        write.push(path.into());
                    } else if rule.access == FileAccess::Read {
                        read.push(path.into());
                    }
                }
            }
            (write, read)
        }
    }
}

/// Mounts are a deterministic projection of both Profile policies.
pub fn profile_pair_workspace_access(
    harness: &PermissionProfile,
    sandbox: &PermissionProfile,
) -> (Vec<String>, Vec<String>) {
    let (mut candidates, mut readonly) = profile_workspace_access(harness);
    let (other, read) = profile_workspace_access(sandbox);
    candidates.extend(other);
    readonly.extend(read);
    let absolute = |p: &str| {
        if p == "." {
            "/work".into()
        } else {
            format!("/work/{p}")
        }
    };
    candidates.retain(|p| {
        harness.policy.portable().file_access(&absolute(p)) == FileAccess::ReadWrite
            && sandbox.policy.portable().file_access(&absolute(p)) == FileAccess::ReadWrite
    });
    candidates.sort();
    candidates.dedup();
    readonly.sort();
    readonly.dedup();
    if !candidates.is_empty() {
        readonly.retain(|r| {
            candidates.iter().any(|w| {
                w == "."
                    || r == w
                    || r.strip_prefix(w)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            })
        });
    }
    (candidates, readonly)
}

pub fn validate_frozen_permissions(s: &EngineeringState, p: &ExecutionPermissions) -> Result<()> {
    let lock = p
        .binding_lock
        .as_ref()
        .ok_or_else(|| err("permission_unconfigured", "执行必须冻结 v2 权限绑定"))?;
    validate_frozen_permission_lock(s, lock)?;
    require(
        p.harness.reference.revision_id == lock.harness.profile.id
            && p.sandbox.reference.revision_id == lock.sandbox.profile.id
            && p.harness.policy == lock.harness.profile.policy.portable()
            && p.sandbox.policy == lock.sandbox.profile.policy.portable()
            && Some(p.approval) == lock.harness.profile.policy.approval,
        "执行权限与冻结 profile 不一致",
    )
}

pub fn validate_frozen_permission_lock(
    s: &EngineeringState,
    lock: &VersionedPermissionLock,
) -> Result<()> {
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
                && (r.r#type == "permission_binding" || r.id == profile.id)
                && r.immutable
                && profile.r#type == "permission_profile"
                && profile.immutable
                && (r.id == profile.id
                    || (r.data == serde_json::to_value(&bound.binding).unwrap()
                        && r.content_hash == bound.binding_hash))
                && bound.binding_hash == content_digest(&References::new(), &json!(bound.binding))
                && r.content_hash == content_digest(&r.refs, &r.data)
                && profile.data == permission_profile_data(&bound.profile)
                && profile.content_hash == bound.profile_hash
                && profile.content_hash
                    == content_digest(&profile.refs, &permission_profile_data(&bound.profile))
                && bound.profile.target_kind == kind
                && bound.binding.target.kind == kind
                && bound.binding.permission_revision_id == bound.profile.id
                && target.immutable
                && target.content_hash == bound.binding.target.content_hash
                && target.content_hash == content_digest(&target.refs, &target.data)
                && target.r#type
                    == if kind == PermissionTargetKind::Harness {
                        "harness"
                    } else {
                        "sandbox"
                    },
            "冻结 v2 权限引用或摘要不一致",
        )?;
    }
    Ok(())
}

impl ConfigurationRecord {
    pub fn permission_profile(&self) -> Result<PermissionProfile> {
        require(self.r#type == "permission_profile", "需要权限源修订")?;
        let mut profile = PermissionProfile {
            scope: self.scope.clone(),
            schema_version: self.data["schema_version"]
                .as_u64()
                .ok_or_else(|| err("invalid_source", "权限缺少协议版本"))?
                as u32,
            id: self.id.clone(),
            identity: self.definition_id.clone(),
            name: self.name.clone(),
            version: self.version,
            supersedes: self.supersedes.clone(),
            target_kind: serde_json::from_value(self.data["target_kind"].clone())
                .map_err(|e| err("invalid_source", e.to_string()))?,
            policy: serde_json::from_value(self.data["policy"].clone())
                .map_err(|e| err("invalid_source", e.to_string()))?,
        };
        profile.normalize()?;
        Ok(profile)
    }
}
pub fn permission_profile_data(p: &PermissionProfile) -> Value {
    serde_json::json!({"schema_version":p.schema_version,"target_kind":p.target_kind,"policy":p.policy})
}

impl Serialize for PermissionProfile {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        SourceDefinition {
            r#type: "permission_profile".into(),
            identity: self.identity.clone(),
            revision: self.version,
            name: self.name.clone(),
            scope: self.scope.clone(),
            refs: References::new(),
            data: permission_profile_data(self),
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for PermissionProfile {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let source = SourceDefinition::deserialize(deserializer)?;
        source
            .into_record()
            .and_then(|r| r.permission_profile())
            .map_err(serde::de::Error::custom)
    }
}
