use crate::*;
pub fn template_permission_defaults(
    s: &mut EngineeringState,
    contract: &dyn Fn(&EngineeringState, &PermissionTargetRef) -> Result<String>,
) -> Result<()> {
    let defaults = current_template_defaults(s)
        .ok_or_else(|| err("configuration_required", "默认模板目录未初始化"))?;
    let mut targets = std::collections::BTreeSet::new();
    for r in s.governance.iter().filter(|r| {
        r.r#type == "agent"
            && template_record_current(s, r)
            && template_catalog_member(s, r).is_some_and(|m| m.data["template_key"].is_string())
    }) {
        let agent = s.resolve_agent(&r.id)?;
        targets.insert(agent.harness_ref);
    }
    for r in s
        .governance
        .iter()
        .filter(|r| r.r#type == "harness" && template_record_current(s, r))
    {
        if template_catalog_member(s, r).is_some_and(|m| m.data["template_key"].is_string())
            && !s.governance.iter().any(|n| {
                n.r#type == r.r#type && n.definition_id == r.definition_id && n.version > r.version
            })
        {
            targets.insert(r.id.clone());
        }
    }
    let mut changes = Vec::new();
    for (kind, id, profile) in targets
        .into_iter()
        .map(|id| {
            (
                PermissionTargetKind::Harness,
                id,
                defaults["harness_permission_ref"].as_str().unwrap(),
            )
        })
        .chain(std::iter::once((
            PermissionTargetKind::Sandbox,
            defaults["sandbox_ref"]
                .as_str()
                .ok_or_else(|| err("invalid_template", "默认 Sandbox 缺失"))?
                .to_owned(),
            defaults["sandbox_permission_ref"].as_str().unwrap(),
        )))
    {
        let target = permission_target(s, kind, &id)?;
        if active_permission_binding(s, &target).is_none() {
            changes.push(json!({"target":target,"permission_revision_id":profile,"expected_revision":0,"enforcement_contract":contract(s,&target)?}));
        }
    }
    if !changes.is_empty() {
        apply_permission_bindings(s, &changes)?;
    }
    Ok(())
}

impl Engine {
    pub fn ensure_default_permissions(
        &self,
        contract: &dyn Fn(&EngineeringState, &PermissionTargetRef) -> Result<String>,
    ) -> Result<()> {
        self.store.transact(None, &mut |s| {
            template_permission_defaults(s, contract)?;
            Ok(json!({"ok":true}))
        })?;
        Ok(())
    }
}

impl Engine {
    pub fn reset_factory_catalog(
        &self,
        c: &Command,
        fingerprint: &str,
        preview: &Value,
        contract: &dyn Fn(&EngineeringState, &PermissionTargetRef) -> Result<String>,
    ) -> Result<Value> {
        self.store.transact(Some((&c.command_id,fingerprint)),&mut |s| {
            crate::configuration_transfers::configuration_transfer_guard(s,"configuration.templates.reset")?;
            require(json!(template_catalog_generation(s))==preview["generation"],"目录代次已变化，请重新预演")?;
            if preview["pristine"]==true{return Ok(json!({"generation":preview["generation"],"changed":false,"counts":preview["current_counts"],"bundle_version":preview["bundle_version"]}));}
            let result=restore_template_catalog(s,&c.command_id)?;
            template_permission_defaults(s,contract)?;
            Ok(result)
        })
    }
}
