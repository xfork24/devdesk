//! Explicit background publication; ordinary reads only compose disposable catalogs.
use crate::{concurrency::*, *};

fn publish(
    store: &dyn EngineeringStore,
    before: &ConfigurationCatalogSnapshot,
    next: EngineeringState,
    kind: &str,
) -> Result<()> {
    let candidate =
        ConfigurationCatalogSnapshot::new(next, before.revision, before.repository_ready)?;
    // This operation only copies immutable source identities. A stale no-op
    // needs no publication and must not reject an unrelated business command.
    if candidate.fingerprint == before.fingerprint {
        return Ok(());
    }
    let publication = prepare_configuration_publication(store, before, &candidate)?;
    if publication.changes.is_empty() {
        return Ok(());
    }
    let transfer = match configuration_transfer_witness(store) {
        Ok(witness) => witness,
        Err(error) if error.code == "configuration_transfer_pending" => return Ok(()),
        Err(error) => return Err(error),
    };
    let digest = configuration_hash(&(
        kind,
        store.namespace(),
        store.root(),
        before.revision,
        &candidate.fingerprint,
    ));
    store.commit_changes(&PreparedChangeSet {
        command_id: format!("{kind}:{digest}"),
        fingerprint: digest,
        read_set: publication.read_set.merge([transfer])?,
        changes: publication.changes,
        result: json!({"ok":true}),
        time: now() as i64,
    })?;
    Ok(())
}
impl Engine {
    pub fn sync_shared_configuration(
        &self,
        global: &dyn EngineeringStore,
        package_namespace: Option<&str>,
    ) -> Result<()> {
        // Rebuild only this derived copy after a concurrent catalog publication.
        // The caller's original business command/versions are never retried.
        for attempt in 0..3 {
            match self.sync_shared_configuration_once(global, package_namespace) {
                Err(error) if error.code == "revision_conflict" && attempt < 2 => continue,
                result => return result,
            }
        }
        unreachable!()
    }
    fn sync_shared_configuration_once(
        &self,
        global: &dyn EngineeringStore,
        package_namespace: Option<&str>,
    ) -> Result<()> {
        let existing = self.store.configuration_snapshot()?;
        let known = global.configuration_snapshot()?;
        let allowed = |record: &ConfigurationRecord| {
            package_role_identity(record).is_none_or(|identity| {
                package_namespace.is_some_and(|namespace| identity.starts_with(namespace))
            })
        };
        if existing.needs_factory_reset_from(&known) {
            return self.sync_factory_reset(global, allowed);
        }
        let mut directory = known.state.as_ref().clone();
        for role in existing.state.governance.iter().filter(|r| {
            matches!(r.r#type.as_str(), "role" | "role_retirement")
                && package_role_identity(r).is_none()
                && !template_history_retained(&existing.state, r)
        }) {
            if let Some(old) = known.record(&role.id) {
                require(old.content_hash == role.content_hash, "Role 目录摘要冲突")?;
            } else {
                directory.governance.push(role.clone());
            }
        }
        publish(global, &known, directory, "role-directory-sync")?;
        if existing
            .state
            .projects
            .iter()
            .any(|p| p.execution_mode != "live")
        {
            return Ok(());
        }
        let mut next = existing.state.as_ref().clone();
        let mut ids = next
            .governance
            .iter()
            .map(|r| (r.id.clone(), r.content_hash.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        // Use the same global snapshot as the reset check. Role records just
        // published to the directory are already present in this project.
        for record in known.state.governance.iter().filter(|r| {
            r.r#type != "template_catalog_member"
                && r.immutable
                && allowed(r)
                && (matches!(r.r#type.as_str(), "role" | "role_retirement")
                    || configuration_visible(&known.state, r, self.store.project_id().as_deref()))
        }) {
            if let Some(hash) = ids.get(&record.id) {
                require(
                    hash == &record.content_hash,
                    format!("共享配置摘要冲突 {}", record.id),
                )?;
            } else {
                ids.insert(record.id.clone(), record.content_hash.clone());
                next.governance.push(record.clone());
            }
        }
        copy_template_catalog(&known.state, &mut next)?;
        // Definitions/Harness/Sandbox views are rebuilt from source revisions;
        // they are not a second mutable configuration store.
        next.refresh_configuration_views()?;
        publish(self.store.as_ref(), &existing, next, "shared-catalog-sync")
    }

    fn sync_factory_reset(
        &self,
        global: &dyn EngineeringStore,
        allowed: impl Fn(&ConfigurationRecord) -> bool,
    ) -> Result<()> {
        // A foreground reset and the background publisher can see the same
        // generation change. Only one needs to prepare its historical evidence.
        let _reset = self
            .configuration_reset
            .lock()
            .map_err(|error| err("storage", error.to_string()))?;
        let existing = self.store.configuration_snapshot()?;
        let known = global.configuration_snapshot()?;
        if !existing.needs_factory_reset_from(&known) {
            return Ok(());
        }
        let records: Vec<_> = known
            .state
            .governance
            .iter()
            .filter(|r| allowed(r))
            .cloned()
            .collect();
        let publication = self
            .store
            .prepare_configuration_reset(&records, existing.revision)?;
        let transfer = configuration_transfer_witness(self.store.as_ref())?;
        let digest = configuration_hash(&(
            "factory-reset-sync",
            self.store.namespace(),
            known.revision,
            existing.revision,
        ));
        self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("factory-reset-sync:{digest}"),
            fingerprint: digest,
            read_set: publication.read_set.merge([transfer])?,
            changes: publication.changes,
            result: json!({"ok":true}),
            time: now() as i64,
        })?;
        Ok(())
    }
}
