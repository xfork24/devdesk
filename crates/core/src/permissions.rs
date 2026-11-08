//! Permission revisions use the same idempotent command ledger as other definitions.
use crate::*;
impl Engine {
    pub(crate) fn permission_command(
        &self,
        method: &str,
        c: &Command,
        s: &mut EngineeringState,
    ) -> Option<Result<Value>> {
        if !["permission.create", "permission.revise"].contains(&method) {
            return None;
        }
        Some((|| {
            let p = &c.payload;
            let mut permission = if method == "permission.revise" {
                let old = s
                    .permissions
                    .iter()
                    .find(|p| Some(p.id.as_str()) == c.target.as_deref())
                    .ok_or_else(|| err("not_found", "权限修订不存在"))?;
                if c.expected_revision != Some(old.version) {
                    return Err(err("revision_conflict", "Permission 版本冲突"));
                }
                require(
                    !s.permissions
                        .iter()
                        .any(|p| p.supersedes.as_ref() == Some(&old.id)),
                    "请基于最新权限修订继续修改",
                )?;
                let mut next = old.clone();
                next.id = id();
                next.version += 1;
                next.supersedes = Some(old.id.clone());
                next
            } else {
                let identity = text(p, "identity")?;
                require(
                    !s.permissions.iter().any(|p| p.identity == identity),
                    "权限身份已存在，请创建修订",
                )?;
                Permission {
                    id: id(),
                    identity,
                    name: text(p, "name")?,
                    version: 1,
                    supersedes: None,
                    schema_version: 1,
                    policy: PermissionPolicy::full_access(),
                }
            };
            permission.policy = serde_json::from_value(p["policy"].clone())
                .map_err(|e| err("invalid_params", e.to_string()))?;
            if let Some(name) = p["name"].as_str() {
                permission.name = name.into();
            }
            permission.policy.canonicalize();
            permission.validate()?;
            let result = json!(permission);
            s.permissions.push(permission);
            Ok(result)
        })())
    }
}
