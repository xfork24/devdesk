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
        let before = self.store.configuration_snapshot()?;
        let transfer = configuration_transfer_witness(self.store.as_ref())?;
        let mut next = before.state.as_ref().clone();
        template_permission_defaults(&mut next, contract)?;
        let candidate =
            ConfigurationCatalogSnapshot::new(next, before.revision, before.repository_ready)?;
        let publication =
            prepare_configuration_publication(self.store.as_ref(), &before, &candidate)?;
        if !publication.changes.is_empty() {
            let digest = configuration_hash(&(
                "default-permissions/v1",
                self.store.namespace(),
                self.store.root(),
                before.revision,
                &candidate.fingerprint,
            ));
            let mut guards = publication.read_set.versions().to_vec();
            guards.push(transfer);
            self.store
                .commit_changes(&crate::concurrency::PreparedChangeSet {
                    command_id: format!("default-permissions:{digest}"),
                    fingerprint: digest,
                    read_set: crate::concurrency::ReadSet::new(guards)?,
                    changes: publication.changes,
                    result: json!({"ok":true}),
                    time: now() as i64,
                })?;
        }
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
        if let Some(receipt) = self.store.replay_commit(&c.command_id, fingerprint)? {
            return Ok(receipt.result);
        }
        let before = self.store.configuration_snapshot()?;
        let transfer = configuration_transfer_witness(self.store.as_ref())?;
        require(
            json!(template_catalog_generation(&before.state)) == preview["generation"],
            "目录代次已变化，请重新预演",
        )?;
        let mut next = before.state.as_ref().clone();
        let result = if preview["pristine"] == true {
            json!({"generation":preview["generation"],"changed":false,"counts":preview["current_counts"],"bundle_version":preview["bundle_version"]})
        } else {
            let result = restore_template_catalog(&mut next, &c.command_id)?;
            template_permission_defaults(&mut next, contract)?;
            result
        };
        let candidate =
            ConfigurationCatalogSnapshot::new(next, before.revision, before.repository_ready)?;
        let publication =
            prepare_configuration_publication(self.store.as_ref(), &before, &candidate)?;
        let ctx = InvocationContext::human(self.store.project_id())
            .prepare_record("configuration.templates.reset", c);
        let mut guards = publication.read_set.versions().to_vec();
        guards.push(transfer);
        guards.push(crate::concurrency::ResourceVersion {
            resource: ctx.resource.clone(),
            revision: 0,
        });
        let mut changes = publication.changes;
        changes.push(ctx);
        self.store
            .commit_changes(&crate::concurrency::PreparedChangeSet {
                command_id: c.command_id.clone(),
                fingerprint: fingerprint.into(),
                read_set: crate::concurrency::ReadSet::new(guards)?,
                changes,
                result,
                time: now() as i64,
            })
            .map(|r| r.result)
    }
}
