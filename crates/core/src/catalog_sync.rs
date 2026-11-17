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
    pub fn sync_shared_configuration(&self, global: &dyn EngineeringStore) -> Result<()> {
        let existing = self.store.configuration_snapshot()?;
        let known = global.configuration_snapshot()?;
        let mut directory = known.state.as_ref().clone();
        for role in existing
            .state
            .governance
            .iter()
            .filter(|r| matches!(r.r#type.as_str(), "role" | "role_retirement"))
        {
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
        let shared = global.configuration_snapshot()?;
        let mut next = existing.state.as_ref().clone();
        let mut ids = next
            .governance
            .iter()
            .map(|r| (r.id.clone(), r.content_hash.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        for record in shared.state.governance.iter().filter(|r| {
            r.immutable
                && (matches!(r.r#type.as_str(), "role" | "role_retirement")
                    || configuration_visible(&shared.state, r, self.store.project_id().as_deref()))
        }) {
            if let Some(hash) = ids.get(&record.id) {
                require(hash == &record.content_hash, "共享配置摘要冲突")?;
            } else {
                ids.insert(record.id.clone(), record.content_hash.clone());
                next.governance.push(record.clone());
            }
        }
        copy_template_catalog(&shared.state, &mut next)?;
        // Definitions/Harness/Sandbox views are rebuilt from source revisions;
        // they are not a second mutable configuration store.
        next.refresh_configuration_views()?;
        publish(self.store.as_ref(), &existing, next, "shared-catalog-sync")
    }
}
