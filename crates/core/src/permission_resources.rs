//! Versioned profiles and assignments share the durable configuration command ledger.
use crate::governance::record;
use crate::*;

pub fn insert_permission_profiles(
    s: &mut EngineeringState,
    values: &[Value],
    source: &str,
) -> Result<Value> {
    if values.len() > 512 {
        return Err(err("invalid_params", "一次最多导入 512 个权限修订"));
    }
    let mut profiles = Vec::new();
    let mut legacy = Vec::new();
    for value in values {
        if value["schema_version"] == 1 {
            let p: Permission = serde_json::from_value(value.clone())
                .map_err(|e| err("invalid_params", e.to_string()))?;
            p.validate()?;
            legacy.push(p);
        } else {
            let mut p: PermissionProfile = serde_json::from_value(value.clone())
                .map_err(|e| err("invalid_params", e.to_string()))?;
            p.normalize()?;
            profiles.push(p);
        }
    }
    profiles.sort_by_key(|p| p.version);
    legacy.sort_by_key(|p| p.version);
    let mut added = Vec::new();
    let mut unchanged = Vec::new();
    for p in legacy {
        if let Some(old) = s.permissions.iter().find(|v| v.id == p.id) {
            if old != &p {
                return Err(err("revision_conflict", "旧权限修订内容冲突"));
            }
            unchanged.push(p.id);
            continue;
        }
        if s.permissions
            .iter()
            .any(|v| v.identity == p.identity && v.version == p.version)
        {
            return Err(err("revision_conflict", "旧权限版本已有不同 ID"));
        }
        if p.version > 1
            && !s.permissions.iter().any(|v| {
                Some(&v.id) == p.supersedes.as_ref()
                    && v.identity == p.identity
                    && v.version + 1 == p.version
            })
        {
            return Err(err("revision_conflict", "旧权限缺少连续前序修订"));
        }
        added.push(p.id.clone());
        s.permissions.push(p);
    }
    for p in profiles {
        let data = json!(p);
        if let Some(old) = s.governance.iter().find(|r| r.id == p.id) {
            if old.kind != "permission_profile" || old.data != data {
                return Err(err(
                    "revision_conflict",
                    format!("权限修订内容冲突: {}", p.id),
                ));
            }
            unchanged.push(p.id);
            continue;
        }
        if s.governance.iter().any(|r| {
            r.kind == "permission_profile"
                && r.definition_id == p.identity
                && r.version == p.version
        }) {
            return Err(err("revision_conflict", "同一权限版本已有不同 ID"));
        }
        if p.version == 1 {
            if p.supersedes.is_some() {
                return Err(err("invalid_params", "首个修订不能有 supersedes"));
            }
        } else {
            let parent = p
                .supersedes
                .as_ref()
                .and_then(|id| s.configuration(id).ok())
                .filter(|r| {
                    r.kind == "permission_profile"
                        && r.definition_id == p.identity
                        && r.version + 1 == p.version
                })
                .ok_or_else(|| err("revision_conflict", "权限缺少连续前序修订"))?;
            if parent.decode::<PermissionProfile>()?.target_kind != p.target_kind {
                return Err(err(
                    "permission_inapplicable",
                    "修订不能改变 profile 目标类型",
                ));
            }
            if s.governance.iter().any(|r| {
                r.kind == "permission_profile" && r.supersedes.as_ref() == Some(&parent.id)
            }) {
                return Err(err("revision_conflict", "权限已有后继修订"));
            }
        }
        let mut r = record(
            "permission_profile",
            p.identity.clone(),
            Scope::Global,
            p.name.clone(),
            p.version,
            p.supersedes.clone(),
            data,
            true,
        );
        r.id = p.id.clone();
        if source == "embedded" {
            r.created_at = 0;
        }
        added.push(p.id.clone());
        s.governance.push(r);
        let mut origin = record(
            "permission_origin",
            p.id.clone(),
            Scope::Global,
            source.into(),
            1,
            None,
            json!({"source":source}),
            true,
        );
        origin.id = format!(
            "permission-origin:{}:{}",
            p.id,
            configuration_hash(&json!(source))
        );
        origin.created_at = 0;
        s.governance.push(origin);
    }
    Ok(json!({"added":added,"unchanged":unchanged}))
}

pub fn permission_profile_export(s: &EngineeringState, id: &str) -> Result<Value> {
    let mut result = Vec::new();
    let mut next = Some(id.to_owned());
    while let Some(id) = next {
        if result.len() >= 512 {
            return Err(err("invalid_configuration", "权限前序链过长"));
        }
        let r = s.configuration(&id)?;
        if r.kind != "permission_profile" {
            return Err(err("invalid_params", "需要 v2 权限修订"));
        }
        next = r.supersedes.clone();
        result.push(r.data.clone());
    }
    result.reverse();
    Ok(json!({"profiles":result}))
}

pub(crate) fn seed_permission_resources(s: &mut EngineeringState) {
    if !s
        .governance
        .iter()
        .any(|r| r.id == DEFAULT_PERMISSION_SANDBOX)
    {
        let mut r = record(
            "sandbox",
            "builtin-docker-sandbox".into(),
            Scope::Global,
            "Docker Sandbox".into(),
            1,
            None,
            json!(SandboxDefinition {
                provider: "docker".into(),
                runtime_version: "1".into(),
                enforcement: "docker-worker/v1".into(),
                platforms: vec!["linux/aarch64".into(), "linux/x86_64".into()]
            }),
            true,
        );
        r.id = DEFAULT_PERMISSION_SANDBOX.into();
        r.created_at = 0;
        s.governance.push(r);
    }
    let values: Vec<Value> = [
        include_str!("../../../permissions/presets/harness-full-access-v2.json"),
        include_str!("../../../permissions/presets/harness-read-only-v2.json"),
        include_str!("../../../permissions/presets/sandbox-full-access-v2.json"),
    ]
    .iter()
    .map(|s| serde_json::from_str(s).expect("embedded profile"))
    .collect();
    insert_permission_profiles(s, &values, "embedded").expect("embedded profiles valid");
}

impl Engine {
    pub(crate) fn permission_resource_command(
        &self,
        method: &str,
        c: &Command,
        s: &mut EngineeringState,
    ) -> Result<Option<Value>> {
        let p = &c.payload;
        let result = match method {
            "permission.profile.import" => {
                let values = p["profiles"]
                    .as_array()
                    .ok_or_else(|| err("invalid_params", "需要 profiles 数组"))?;
                insert_permission_profiles(s, values, "import")?
            }
            "permission.profile.create" | "permission.profile.revise" => {
                let allowed = if method.ends_with("create") {
                    &["identity", "name", "target_kind", "policy"][..]
                } else {
                    &["name", "target_kind", "policy"][..]
                };
                if p.as_object()
                    .is_none_or(|o| o.keys().any(|key| !allowed.contains(&key.as_str())))
                {
                    return Err(err("invalid_params", "权限输入包含未知字段"));
                }
                let old = if method.ends_with("revise") {
                    let r = s.configuration(c.target.as_deref().unwrap_or(""))?;
                    if r.kind != "permission_profile" || c.expected_revision != Some(r.version) {
                        return Err(err("revision_conflict", "权限版本已改变"));
                    }
                    Some(r.decode::<PermissionProfile>()?)
                } else {
                    None
                };
                let profile = PermissionProfile {
                    schema_version: 2,
                    id: id(),
                    identity: match &old {
                        Some(r) => r.identity.clone(),
                        None => text(p, "identity")?,
                    },
                    name: text(p, "name")?,
                    version: old.as_ref().map_or(1, |r| r.version + 1),
                    supersedes: old.as_ref().map(|r| r.id.clone()),
                    target_kind: serde_json::from_value(p["target_kind"].clone())
                        .map_err(|e| err("invalid_params", e.to_string()))?,
                    policy: serde_json::from_value(p["policy"].clone())
                        .map_err(|e| err("invalid_params", e.to_string()))?,
                };
                insert_permission_profiles(s, &[json!(profile)], "ui")?;
                s.configuration(&profile.id)?.data.clone()
            }
            "permission.bindings.apply" => {
                let changes = p["bindings"]
                    .as_array()
                    .filter(|v| !v.is_empty() && v.len() <= 64)
                    .ok_or_else(|| err("invalid_params", "需要 1—64 项绑定"))?;
                let mut seen = std::collections::BTreeSet::new();
                let mut result = Vec::new();
                for change in changes {
                    let target: PermissionTargetRef =
                        serde_json::from_value(change["target"].clone())
                            .map_err(|e| err("invalid_params", e.to_string()))?;
                    if !seen.insert(target.revision_id.clone()) {
                        return Err(err("invalid_params", "批次中目标重复"));
                    }
                    if permission_target(s, target.kind, &target.revision_id)? != target {
                        return Err(err("revision_conflict", "目标摘要已变化"));
                    }
                    let current = active_permission_binding(s, &target);
                    let version = current.map_or(0, |r| r.version);
                    if change["expected_revision"].as_u64() != Some(version) {
                        return Err(err("revision_conflict", "目标活动绑定已变化"));
                    }
                    let profile_id = text(change, "permission_revision_id")?;
                    let target_scope = s.configuration(&target.revision_id)?.scope.clone();
                    let pr = s.configuration(&profile_id)?;
                    let mut profile: PermissionProfile = pr.decode()?;
                    profile.normalize()?;
                    if pr.kind != "permission_profile" || profile.target_kind != target.kind {
                        return Err(err("permission_inapplicable", "profile 不适用于目标"));
                    }
                    let contract = text(change, "enforcement_contract")?;
                    let binding = VersionedPermissionBinding {
                        target: target.clone(),
                        permission_revision_id: profile_id,
                        enforcement_contract: contract,
                    };
                    let identity = format!("permission-binding:{}", target.revision_id);
                    let previous = s
                        .governance
                        .iter()
                        .filter(|r| r.kind == "permission_binding" && r.definition_id == identity)
                        .max_by_key(|r| r.version)
                        .cloned();
                    let r = record(
                        "permission_binding",
                        identity,
                        target_scope.clone(),
                        pr.name.clone(),
                        previous.as_ref().map_or(1, |r| r.version + 1),
                        previous.map(|r| r.id),
                        json!(binding),
                        true,
                    );
                    let assignment = record(
                        "permission_assignment",
                        format!("permission-target:{}", target.revision_id),
                        target_scope,
                        r.name.clone(),
                        version + 1,
                        current.map(|r| r.id.clone()),
                        json!({"target":target,"binding_revision_id":r.id}),
                        true,
                    );
                    result.push(json!({"target":target,"binding_revision_id":r.id,"revision":assignment.version}));
                    s.governance.push(r);
                    s.governance.push(assignment);
                }
                json!({"bindings":result})
            }
            _ => return Ok(None),
        };
        Ok(Some(result))
    }
}
