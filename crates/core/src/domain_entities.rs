//! Domain-owned object data is scoped to a single project and package version.
use crate::{concurrency::*, governance::record, platform_facts::one, *};

const KIND: &str = "domain_entity";

impl Engine {
    pub fn read_domain_entity(&self, digest: &str, id: &str) -> Result<Value> {
        let project = self.domain_entity_project()?;
        let row = one(self.store.as_ref(), "governance", &entity_key(&project, id))?;
        let entity = extract(&row, &project, digest)?;
        entity.ok_or_else(|| err("not_found", "领域对象不存在"))
    }

    pub fn write_domain_entity(
        &self,
        digest: &str,
        command: &Command,
        context: &InvocationContext,
    ) -> Result<Value> {
        let project = self.domain_entity_project()?;
        context.require_project(&project)?;
        require(
            matches!(context.actor(), WorkflowActor::Human),
            "领域对象编辑需要人类会话",
        )?;
        let id = command
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "领域对象缺少 ID"))?;
        let key = entity_key(&project, id);
        let receipt_id = format!(
            "domain-entity:{}",
            configuration_hash(&(digest, &project, &command.command_id))
        );
        let fingerprint = configuration_hash(&(digest, &project, command, context));
        if let Some(receipt) = self.store.replay_commit(&receipt_id, &fingerprint)? {
            return Ok(receipt.result);
        }
        let live = one(self.store.as_ref(), "project_live", &project)?;
        require(live.revision == 1, "历史项目只读")?;
        let row = one(self.store.as_ref(), "governance", &key)?;
        if command.expected_revision != Some(row.revision) {
            return Err(err("revision_conflict", "领域对象修订已变化"));
        }
        let type_ref = text(&command.payload, "type_ref")?;
        let previous = extract(&row, &project, digest)?;
        require(
            previous
                .as_ref()
                .is_none_or(|entity| entity["type_ref"] == type_ref),
            "领域对象类型不可变更",
        )?;
        let revision = row.revision + 1;
        let entity = json!({
            "id":id,"project_id":project,"type_ref":type_ref,
            "revision":revision,"data":command.payload["data"],
        });
        // Payload fields belong to the domain and must remain opaque to
        // the configuration source-reference codec.
        let payload = serde_json::to_string(&entity).map_err(|e| err("storage", e.to_string()))?;
        let scope = Scope::project(&project);
        let mut stored = record(
            KIND,
            key.clone(),
            scope,
            type_ref,
            revision,
            None,
            json!({"digest":digest,"payload":payload}),
            false,
        );
        stored.id = key;
        stored.revision = revision;
        self.store
            .commit_changes(&PreparedChangeSet {
                command_id: receipt_id,
                fingerprint,
                read_set: ReadSet::new(vec![
                    ResourceVersion {
                        resource: row.resource.clone(),
                        revision: row.revision,
                    },
                    ResourceVersion {
                        resource: live.resource,
                        revision: live.revision,
                    },
                ])?,
                changes: vec![ResourceChange {
                    resource: row.resource,
                    value: stored.to_storage(),
                }],
                result: entity,
                time: now() as i64,
            })
            .map(|receipt| receipt.result)
    }

    fn domain_entity_project(&self) -> Result<Id> {
        self.store
            .project_id()
            .ok_or_else(|| err("invalid_scope", "领域对象需要项目库"))
    }
}

fn entity_key(project: &str, id: &str) -> String {
    format!("domain-entity:{}", configuration_hash(&(project, id)))
}

fn extract(row: &ResourceSnapshot, project: &str, digest: &str) -> Result<Option<Value>> {
    let Some(value) = row.value.as_ref() else {
        return Ok(None);
    };
    let record = ConfigurationRecord::from_storage(value.clone())?;
    require(
        record.r#type == KIND
            && record.scope == Scope::project(project)
            && record.data["digest"] == digest,
        "领域对象不属于当前项目和包版本",
    )?;
    let payload = record.data["payload"]
        .as_str()
        .ok_or_else(|| err("storage", "领域对象数据缺失"))?;
    serde_json::from_str(payload)
        .map(Some)
        .map_err(|e| err("storage", e.to_string()))
}
