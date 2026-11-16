//! Eventual, version-monotone metadata projection. It never admits execution or
//! replaces authoritative snapshot contents/history at the source namespace.
use crate::{concurrency::*, governance::record, read_models::ExperienceDirectorySource, *};
use std::time::{Duration, Instant};

pub fn resource_key(namespace: &StoreNamespace, source: &str) -> ResourceKey {
    ResourceKey::new(
        "governance",
        format!(
            "experience-directory:{}",
            configuration_hash(&(namespace, source))
        ),
    )
}
fn project(namespace: &StoreNamespace) -> Result<&str> {
    match namespace {
        StoreNamespace::Engineering { project_id } => {
            Ok(project_id.as_deref().unwrap_or(GLOBAL_DISCUSSION_NAMESPACE))
        }
        _ => Err(err("invalid_scope", "经验来源必须为工程或讨论命名空间")),
    }
}
pub fn publish(
    store: &dyn EngineeringStore,
    namespace: &StoreNamespace,
    source: &ExperienceDirectorySource,
) -> Result<()> {
    if store.namespace() != StoreNamespace::Configuration {
        return Err(err("invalid_scope", "经验目录目标必须为全局派生目录"));
    }
    let project = project(namespace)?;
    let expected_scope = if project == GLOBAL_DISCUSSION_NAMESPACE {
        Scope::global()
    } else {
        Scope::project(project)
    };
    require(
        source.revision > 0
            && source.revision <= i64::MAX as u64
            && !source.id.is_empty()
            && source.id.len() <= 1024
            && source.scope == expected_scope
            && source.metadata["project_id"] == project,
        "经验目录来源与原命名空间不符",
    )?;
    require(
        source.content_hash.len() == 64
            && source
                .content_hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()),
        "经验目录来源缺少内容摘要",
    )?;
    require(
        serde_json::to_vec(&source.metadata)
            .map_err(|e| err("invalid_params", e.to_string()))?
            .len()
            <= 64 * 1024,
        "经验目录节选超过 64 KiB",
    )?;
    let agent = text(&source.metadata, "agent_instance_id")?;
    let key = resource_key(namespace, &source.id);
    let fingerprint = configuration_hash(&(
        "experience-directory/v1",
        namespace,
        &source.id,
        source.revision,
        &source.content_hash,
        &source.metadata,
    ));
    let command_id = format!("experience-directory-publish:{fingerprint}");
    if store.replay_commit(&command_id, &fingerprint)?.is_some() {
        return Ok(());
    }
    // Reprepare only this derived observation against a concurrent directory
    // publication; the source version and metadata are never refreshed/rebased.
    for attempt in 0..3 {
        let mut rows = store.read_resources(std::slice::from_ref(&key))?;
        require(
            rows.len() == 1 && rows[0].resource == key,
            "经验目录读取不完整",
        )?;
        let previous = rows.remove(0);
        if let Some(value) = &previous.value {
            let old = ConfigurationRecord::from_storage(value.clone())?;
            require(
                old.r#type == "experience_directory"
                    && old.definition_id == agent
                    && old.data["source_namespace"] == json!(namespace)
                    && old.scope == expected_scope
                    && old.data["source_id"] == source.id,
                "经验目录身份冲突",
            )?;
            let known = old.data["source_revision"]
                .as_u64()
                .ok_or_else(|| err("storage", "经验目录缺少来源版本"))?;
            if known > source.revision {
                return Ok(());
            }
            if known == source.revision {
                require(
                    old.data["source_hash"] == source.content_hash
                        && old.data["value"] == source.metadata,
                    "同一经验来源版本不能有不同目录内容",
                )?;
                return Ok(());
            }
        }
        let mut row = record(
            "experience_directory",
            agent.clone(),
            expected_scope.clone(),
            source.name.clone(),
            1,
            None,
            json!({"source_namespace":namespace,"source_id":source.id,"source_revision":source.revision,"source_hash":source.content_hash,"value":source.metadata,"observed_only":true}),
            false,
        );
        row.id = key.id.clone();
        row.revision = previous
            .revision
            .checked_add(1)
            .filter(|revision| *revision <= i64::MAX as u64)
            .ok_or_else(|| err("resource_exhausted", "经验目录版本已用尽"))?;
        let result = store.commit_changes(&PreparedChangeSet {
            command_id: command_id.clone(),
            fingerprint: fingerprint.clone(),
            read_set: ReadSet::new(vec![ResourceVersion {
                resource: key.clone(),
                revision: previous.revision,
            }])?,
            changes: vec![ResourceChange {
                resource: key.clone(),
                value: row.to_storage(),
            }],
            result: json!({"id":key.id}),
            time: now() as i64,
        });
        match result {
            Err(error) if error.code == "revision_conflict" && attempt < 2 => continue,
            result => return result.map(|_| ()),
        }
    }
    unreachable!()
}
impl Engine {
    pub fn publish_experience_index(&self, global: &dyn EngineeringStore) -> Result<()> {
        let namespace = self.store.namespace();
        project(&namespace)?;
        let after = self
            .experience_cursor
            .lock()
            .map_err(|e| err("storage", e.to_string()))?
            .clone();
        let page = self.store.experience_headers(after.as_deref())?;
        let started = Instant::now();
        let mut last = None;
        let mut processed = 0;
        let mut failure = None;
        for header in &page.records {
            if processed > 0 && started.elapsed() >= Duration::from_millis(5) {
                break;
            }
            let result = (|| {
                let key = resource_key(&namespace, &header.id);
                let rows = global.read_resources(std::slice::from_ref(&key))?;
                require(
                    rows.len() == 1 && rows[0].resource == key,
                    "经验目录读取不完整",
                )?;
                if let Some(value) = &rows[0].value {
                    let old = ConfigurationRecord::from_storage(value.clone())?;
                    require(
                        old.r#type == "experience_directory"
                            && old.data["source_namespace"] == json!(namespace)
                            && old.data["source_id"] == header.id,
                        "经验目录身份冲突",
                    )?;
                    if old.data["source_revision"]
                        .as_u64()
                        .is_some_and(|known| known >= header.revision)
                    {
                        return Ok(());
                    }
                }
                if let Some(source) = self.store.experience_directory_source(&header.id)? {
                    publish(global, &namespace, &source)?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                failure.get_or_insert(error);
            }
            processed += 1;
            last = Some(header.id.clone());
        }
        let next = if processed < page.records.len() {
            last
        } else {
            page.next_after
        };
        *self
            .experience_cursor
            .lock()
            .map_err(|e| err("storage", e.to_string()))? = next;
        failure.map_or(Ok(()), Err)
    }
}
