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
    for value in values {
        let source: SourceDefinition = serde_json::from_value(value.clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        let mut p = source.into_record()?.permission_profile()?;
        p.normalize()?;
        profiles.push(p);
    }
    profiles.sort_by_key(|p| p.version);
    let mut added = Vec::new();
    let mut unchanged = Vec::new();
    for mut p in profiles {
        if p.version > 1 {
            p.supersedes = s
                .governance
                .iter()
                .find(|r| {
                    r.r#type == "permission_profile"
                        && r.definition_id == p.identity
                        && r.version + 1 == p.version
                })
                .map(|r| r.id.clone());
        }
        let data = permission_profile_data(&p);
        if let Some(old) = s.governance.iter().find(|r| r.id == p.id) {
            if old.r#type != "permission_profile"
                || old.data != data
                || old.name != p.name
                || old.scope != p.scope
            {
                return Err(err(
                    "revision_conflict",
                    format!("权限修订内容冲突: {}", p.id),
                ));
            }
            unchanged.push(p.id);
            continue;
        }
        crate::configuration_transfers::configuration_transfer_guard(
            s,
            "permission.profile.import",
        )?;
        if s.governance.iter().any(|r| {
            r.r#type == "permission_profile"
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
                    r.r#type == "permission_profile"
                        && r.definition_id == p.identity
                        && r.version + 1 == p.version
                })
                .ok_or_else(|| err("revision_conflict", "权限缺少连续前序修订"))?;
            require(parent.scope == p.scope, "权限修订不能改变所属范围")?;
            if parent.permission_profile()?.target_kind != p.target_kind {
                return Err(err(
                    "permission_inapplicable",
                    "修订不能改变 profile 目标类型",
                ));
            }
            if s.governance.iter().any(|r| {
                r.r#type == "permission_profile" && r.supersedes.as_ref() == Some(&parent.id)
            }) {
                return Err(err("revision_conflict", "权限已有后继修订"));
            }
        }
        let mut r = record(
            "permission_profile",
            p.identity.clone(),
            p.scope.clone(),
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
            Scope::global(),
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
        if r.r#type != "permission_profile" {
            return Err(err("invalid_params", "需要 v2 权限修订"));
        }
        next = r.supersedes.clone();
        result.push(json!(r.source()));
    }
    result.reverse();
    Ok(json!({"profiles":result}))
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
                    if r.r#type != "permission_profile" || c.expected_revision != Some(r.version) {
                        return Err(err("revision_conflict", "权限版本已改变"));
                    }
                    Some(r.permission_profile()?)
                } else {
                    None
                };
                let mut profile = PermissionProfile {
                    scope: old
                        .as_ref()
                        .map(|p| p.scope.clone())
                        .unwrap_or(Scope::global()),
                    schema_version: 2,
                    id: String::new(),
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
                profile.id = source_reference(&profile.identity, profile.version)?;
                let source = SourceDefinition {
                    r#type: "permission_profile".into(),
                    identity: profile.identity.clone(),
                    revision: profile.version,
                    name: profile.name.clone(),
                    scope: profile.scope.clone(),
                    refs: References::new(),
                    data: permission_profile_data(&profile),
                };
                insert_permission_profiles(s, &[json!(source)], "ui")?;
                json!(s.configuration(&profile.id)?.permission_profile()?)
            }
            "permission.bindings.apply" => {
                let changes = p["bindings"]
                    .as_array()
                    .filter(|v| !v.is_empty() && v.len() <= 64)
                    .ok_or_else(|| err("invalid_params", "需要 1—64 项绑定"))?;
                apply_permission_bindings(s, changes)?
            }
            _ => return Ok(None),
        };
        Ok(Some(result))
    }
}

/// Apply prevalidated target contracts in the caller's transaction.
pub fn apply_permission_bindings(s: &mut EngineeringState, changes: &[Value]) -> Result<Value> {
    if !changes.is_empty() {
        crate::configuration_transfers::configuration_transfer_guard(
            s,
            "permission.bindings.apply",
        )?;
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut result = Vec::new();
    for change in changes {
        let target: PermissionTargetRef = serde_json::from_value(change["target"].clone())
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
        let mut profile = pr.permission_profile()?;
        profile.normalize()?;
        if pr.r#type != "permission_profile" || profile.target_kind != target.kind {
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
            .filter(|r| r.r#type == "permission_binding" && r.definition_id == identity)
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
        result.push(
            json!({"target":target,"binding_revision_id":r.id,"revision":assignment.version}),
        );
        s.governance.push(r);
        s.governance.push(assignment);
    }
    Ok(json!({"bindings":result}))
}
