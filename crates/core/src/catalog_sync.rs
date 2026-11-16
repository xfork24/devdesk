use crate::*;
impl Engine {
    pub fn sync_shared_configuration(&self, global: &dyn EngineeringStore) -> Result<()> {
        let e = self;

        let existing = e.store.configuration_snapshot()?.state.clone();
        let known = global.configuration_snapshot()?;
        // One global directory indexes project-owned Role identities without changing their owner or revision.
        let missing_roles = existing
            .governance
            .iter()
            .filter(|r| r.r#type == "role" || r.r#type == "role_retirement")
            .any(|r| {
                known
                    .record(&r.id)
                    .is_none_or(|old| old.content_hash != r.content_hash)
            });
        if missing_roles {
            global.transact(None, &mut |directory| {
                for role in existing
                    .governance
                    .iter()
                    .filter(|r| r.r#type == "role" || r.r#type == "role_retirement")
                {
                    if let Some(old) = directory.governance.iter().find(|r| r.id == role.id) {
                        require(old.content_hash == role.content_hash, "Role 目录摘要冲突")?;
                    } else {
                        directory.governance.push(role.clone());
                    }
                }
                Ok(Value::Null)
            })?;
        }
        let shared = global.configuration_snapshot()?.state.clone();
        if existing.projects.iter().any(|p| p.execution_mode != "live") {
            return Ok(());
        }
        if template_catalog_state(&shared) == template_catalog_state(&existing)
            && shared
                .governance
                .iter()
                .filter(|r| {
                    r.immutable
                        && (r.r#type == "role"
                            || r.r#type == "role_retirement"
                            || configuration_visible(&shared, r, e.store.project_id().as_deref()))
                })
                .all(|r| {
                    existing
                        .governance
                        .iter()
                        .any(|v| v.id == r.id && v.content_hash == r.content_hash)
                })
            && shared.definitions.iter().all(|d| {
                existing.definitions.iter().any(|v| {
                    v.id == d.id
                        && serde_json::to_value(v).unwrap() == serde_json::to_value(d).unwrap()
                })
            })
            && shared
                .harnesses
                .iter()
                .all(|h| existing.harnesses.iter().any(|v| v.id == h.id))
            && shared
                .sandboxes
                .iter()
                .all(|b| existing.sandboxes.iter().any(|v| v.id == b.id))
        {
            return Ok(());
        }
        e.store.transact(None, &mut |s| {
            for r in shared.governance.iter().filter(|r| {
                r.immutable
                    && (r.r#type == "role"
                        || r.r#type == "role_retirement"
                        || configuration_visible(&shared, r, e.store.project_id().as_deref()))
            }) {
                if let Some(old) = s.governance.iter().find(|v| v.id == r.id) {
                    require(old.content_hash == r.content_hash, "共享配置摘要冲突")?;
                } else {
                    s.governance.push(r.clone());
                }
            }
            // Revisions are immutable. Copies keep historical foreign keys self-contained.
            for d in &shared.definitions {
                if let Some(old) = s.definitions.iter().find(|old| old.id == d.id) {
                    require(
                        serde_json::to_value(old).unwrap() == serde_json::to_value(d).unwrap(),
                        "共享定义修订发生冲突",
                    )?;
                } else {
                    s.definitions.push(d.clone());
                }
            }
            for h in &shared.harnesses {
                if !s.harnesses.iter().any(|old| old.id == h.id) {
                    s.harnesses.push(h.clone());
                }
            }
            for b in &shared.sandboxes {
                if !s.sandboxes.iter().any(|old| old.id == b.id) {
                    s.sandboxes.push(b.clone());
                }
            }
            copy_template_catalog(&shared, s)?;
            Ok(json!({"ok":true}))
        })?;
        Ok(())
    }
}
