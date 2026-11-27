//! Domain-owned object data is scoped to its project across package versions.
use crate::{
    concurrency::*, governance::record, platform_facts::one, workflow_commits::WorkflowCommit,
    workflow_resources::version, *,
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const KIND: &str = "domain_entity";

/// A trusted package freezes this binding when admitting an entity-owned root.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DomainWorkflowBinding {
    pub digest: String,
    pub owner_type: Id,
    pub input: String,
    pub outputs: BTreeMap<String, DomainOutputBinding>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DomainOutputBinding {
    pub type_ref: Id,
    pub schema: Value,
    #[serde(default)]
    pub owner_relations: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainRelation {
    pub target: Id,
    #[serde(default)]
    pub min_items: usize,
    pub max_items: usize,
}

type Relations = BTreeMap<String, Vec<Id>>;
const MAX_RELATION_OBJECTS: usize = 256;

/// The entities a handler asks the kernel for: every object of these types in its project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntitySelection {
    pub types: Vec<Id>,
    #[serde(default = "default_entity_limit")]
    pub max_items: usize,
}

/// One page of a project's entities of certain types. `after` is the id of the last
/// entity of the page before. The order is stable but says nothing about time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityQuery {
    pub types: Vec<Id>,
    pub after: Option<Id>,
    pub limit: usize,
}

/// The most entities one handler receives.
pub const MAX_ENTITY_ITEMS: usize = 200;
/// The most entities one page returns.
pub const MAX_ENTITY_PAGE: usize = 100;
/// A handler's entity input stops growing past this size.
const MAX_ENTITY_INPUT: usize = 512 * 1024;
/// Entities read from the store at a time while a handler's input is assembled.
const ENTITY_READ: usize = 64;

fn default_entity_limit() -> usize {
    100
}

/// Distinct type references, at most the 32 a package can declare.
fn distinct_types(types: &[Id]) -> bool {
    !types.is_empty()
        && types.len() <= 32
        && types.iter().collect::<BTreeSet<_>>().len() == types.len()
}

impl EntitySelection {
    /// Checked when the package that declares the handler is installed.
    pub fn validate(&self) -> Result<()> {
        if !distinct_types(&self.types) {
            return Err(err(
                "invalid_domain_package",
                "领域对象输入需要 1 到 32 个不重复的对象类型",
            ));
        }
        if !(1..=MAX_ENTITY_ITEMS).contains(&self.max_items) {
            return Err(err(
                "invalid_domain_package",
                "领域对象输入的数量上限需要在 1 到 200 之间",
            ));
        }
        Ok(())
    }
}

impl EntityQuery {
    pub fn validate(&self) -> Result<()> {
        if !distinct_types(&self.types) {
            return Err(err(
                "invalid_params",
                "领域对象查询需要 1 到 32 个不重复的对象类型",
            ));
        }
        if self.after.as_deref().is_some_and(|id| {
            uuid::Uuid::parse_str(id).map_or(true, |parsed| parsed.to_string() != id)
        }) {
            return Err(err(
                "invalid_params",
                "领域对象查询的游标需要是上一页最后的对象 ID",
            ));
        }
        if !(1..=MAX_ENTITY_PAGE).contains(&self.limit) {
            return Err(err(
                "invalid_params",
                "领域对象查询的页大小需要在 1 到 100 之间",
            ));
        }
        Ok(())
    }
}

fn relations(value: Option<&Value>) -> Result<Relations> {
    value.map_or_else(
        || Ok(BTreeMap::new()),
        |value| {
            serde_json::from_value(value.clone()).map_err(|e| err("invalid_params", e.to_string()))
        },
    )
}

fn relation_versions(
    store: &dyn EngineeringStore,
    project: &str,
    id: &str,
    rules: &BTreeMap<String, DomainRelation>,
    links: &Relations,
) -> Result<Vec<ResourceVersion>> {
    require(
        links.keys().all(|name| rules.contains_key(name)),
        "领域对象关系未声明",
    )?;
    let mut pending = Vec::new();
    for (name, rule) in rules {
        let targets = links.get(name).map(Vec::as_slice).unwrap_or_default();
        require(
            (rule.min_items..=rule.max_items).contains(&targets.len())
                && targets.iter().collect::<BTreeSet<_>>().len() == targets.len(),
            "领域对象关系违反基数或包含重复对象",
        )?;
        require(
            targets.iter().all(|target| {
                uuid::Uuid::parse_str(target).is_ok_and(|id| id.to_string() == *target)
            }),
            "领域对象关系目标需要规范 UUID",
        )?;
        pending.extend(
            targets
                .iter()
                .map(|target| (target.clone(), Some(rule.target.as_str()))),
        );
    }
    let mut seen = BTreeMap::<Id, Id>::new();
    let mut versions = Vec::new();
    while let Some((target, expected_type)) = pending.pop() {
        require(target != id, "领域对象关系不可成环")?;
        if let Some(kind) = seen.get(&target) {
            require(
                expected_type.is_none_or(|expected| kind == expected),
                "领域对象关系类型不符",
            )?;
            continue;
        }
        require(
            seen.len() < MAX_RELATION_OBJECTS,
            "领域对象关系超出遍历预算",
        )?;
        let row = one(store, "governance", &domain_entity_key(project, &target))?;
        let entity = extract(&row, project)?
            .ok_or_else(|| err("not_found", "领域对象关系目标不在当前项目"))?;
        let kind = text(&entity, "type_ref")?;
        require(
            expected_type.is_none_or(|expected| kind == expected),
            "领域对象关系类型不符",
        )?;
        seen.insert(target, kind);
        versions.push(version(&row));
        pending.extend(
            relations(entity.get("relations"))?
                .into_values()
                .flatten()
                .map(|id| (id, None)),
        );
    }
    Ok(versions)
}

impl Engine {
    /// A page of this project's entities of certain types, each with the state of its workflows.
    pub fn domain_entities(&self, query: EntityQuery) -> Result<Value> {
        let project = self.domain_entity_project()?;
        entity_page(self.store.as_ref(), &project, query)
    }

    pub fn read_domain_entity(&self, id: &str) -> Result<Value> {
        let project = self.domain_entity_project()?;
        let row = one(
            self.store.as_ref(),
            "governance",
            &domain_entity_key(&project, id),
        )?;
        let entity = extract(&row, &project)?;
        entity.ok_or_else(|| err("not_found", "领域对象不存在"))
    }

    pub fn read_domain_submission(&self, root: &str) -> Result<Option<Value>> {
        let row = one(
            self.store.as_ref(),
            "governance",
            &format!("domain-submission:{root}"),
        )?;
        Ok(row.value.map(|value| value["data"].clone()))
    }

    pub fn write_domain_entity(
        &self,
        digest: &str,
        rules: &BTreeMap<String, DomainRelation>,
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
        let key = domain_entity_key(&project, id);
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
        let active = one(self.store.as_ref(), "workflow_active_owner", id)?;
        require(
            active
                .value
                .as_ref()
                .is_some_and(|row| row["active"] == false),
            "领域对象有在途根流程",
        )?;
        let row = one(self.store.as_ref(), "governance", &key)?;
        if command.expected_revision != Some(row.revision) {
            return Err(err("revision_conflict", "领域对象修订已变化"));
        }
        let type_ref = text(&command.payload, "type_ref")?;
        let previous = extract(&row, &project)?;
        require(
            previous
                .as_ref()
                .is_none_or(|entity| entity["type_ref"] == type_ref),
            "领域对象类型不可变更",
        )?;
        require(
            previous
                .as_ref()
                .is_none_or(|entity| entity.get("origin").is_none()),
            "工作流提交的领域产物不可覆盖",
        )?;
        let links = relations(command.payload.get("relations"))?;
        let mut versions = vec![version(&row), version(&live), version(&active)];
        versions.extend(relation_versions(
            self.store.as_ref(),
            &project,
            id,
            rules,
            &links,
        )?);
        let revision = row.revision + 1;
        let entity = json!({
            "id":id,"project_id":project,"type_ref":type_ref,
            "revision":revision,"data":command.payload["data"],"relations":links,
        });
        let stored = storage_record(&project, &key, digest, &entity, revision)?;
        self.store
            .commit_changes(&PreparedChangeSet {
                command_id: receipt_id,
                fingerprint,
                read_set: ReadSet::new(versions)?,
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

/// A page of the project's entities of certain types. Each record is the entity as
/// `read_domain_entity` returns it, with a `workflow` summary: whether a root workflow of
/// its owner is in flight, and the latest root workflow started for it.
pub fn entity_page(
    store: &dyn EngineeringStore,
    project: &str,
    query: EntityQuery,
) -> Result<Value> {
    query.validate()?;
    let page = store.domain_entity_page(&crate::read_models::DomainEntityQuery {
        types: query.types.clone(),
        after: query
            .after
            .as_deref()
            .map(|id| domain_entity_key(project, id)),
        limit: query.limit,
    })?;
    let records = page
        .records
        .into_iter()
        .map(|row| {
            let mut entity: Value = serde_json::from_str(&row.payload)
                .map_err(|error| err("storage", error.to_string()))?;
            require(
                entity["project_id"] == project
                    && entity["revision"].as_u64() == Some(row.revision),
                "领域对象与存储行不符",
            )?;
            let last = row
                .last
                .map(|last| serde_json::from_str::<Value>(&last))
                .transpose()
                .map_err(|error| err("storage", error.to_string()))?;
            entity["workflow"] = json!({"active": row.active, "last": last});
            Ok(entity)
        })
        .collect::<Result<Vec<_>>>()?;
    let next_after = page
        .more
        .then(|| records.last().and_then(|record| record["id"].as_str()))
        .flatten()
        .map(str::to_owned);
    Ok(json!({"records": records, "next_after": next_after}))
}

/// The kernel input `domain.entities`: the entities of the selection's types in the
/// project, at most `max_items` of them and about `MAX_ENTITY_INPUT` bytes in all.
/// `truncated` says that matching entities were left out.
pub fn handler_entities(
    store: &dyn EngineeringStore,
    project: &str,
    selection: &EntitySelection,
) -> Result<Value> {
    selection.validate()?;
    let mut entities = Vec::new();
    let mut bytes = 0usize;
    let mut truncated = false;
    let mut after = None;
    'pages: loop {
        let page = entity_page(
            store,
            project,
            EntityQuery {
                types: selection.types.clone(),
                after,
                limit: selection.max_items.min(ENTITY_READ),
            },
        )?;
        let more = page["next_after"].is_string();
        for record in page["records"].as_array().into_iter().flatten() {
            if entities.len() == selection.max_items || bytes > MAX_ENTITY_INPUT {
                truncated = true;
                break 'pages;
            }
            bytes += record.to_string().len();
            entities.push(record.clone());
        }
        if !more {
            break;
        }
        after = page["next_after"].as_str().map(str::to_owned);
    }
    Ok(json!({"entities": entities, "truncated": truncated}))
}

fn storage_record(
    project: &str,
    key: &str,
    digest: &str,
    entity: &Value,
    revision: u64,
) -> Result<ConfigurationRecord> {
    // Domain payloads remain opaque to the configuration source-reference codec.
    let mut stored = record(
        KIND,
        key.into(),
        Scope::project(project),
        text(entity, "type_ref")?,
        revision,
        None,
        json!({"digest":digest,"payload":entity.to_string()}),
        false,
    );
    stored.id = key.into();
    stored.revision = revision;
    Ok(stored)
}

pub(crate) fn workflow_binding_key(root: &str) -> String {
    format!("domain-workflow:{root}")
}

impl WorkflowCommit<'_> {
    pub(crate) fn record_domain_submission(
        &mut self,
        root: &ConfiguredWorkflowInstance,
        submitted: &BTreeMap<String, Id>,
    ) -> Result<()> {
        if submitted.is_empty() {
            return Ok(());
        }
        let mut row = record(
            "domain_workflow_submission",
            root.root_instance_id.clone(),
            Scope::project(&root.project_id),
            "Domain workflow submission".into(),
            1,
            None,
            json!({"owner_id":root.owner_id,"entities":submitted}),
            true,
        );
        row.id = format!("domain-submission:{}", root.root_instance_id);
        self.insert_record(&row)
    }
    pub(crate) fn commit_domain_outputs(
        &mut self,
        root: &ConfiguredWorkflowInstance,
        facts: &WorkflowFacts,
        decision: &str,
    ) -> Result<BTreeMap<String, Id>> {
        if root.parent_execution_id.is_some() {
            return Ok(BTreeMap::new());
        }
        let store = self.engine.store.as_ref();
        let frozen = one(
            store,
            "governance",
            &workflow_binding_key(&root.root_instance_id),
        )?;
        let Some(row) = &frozen.value else {
            return Ok(BTreeMap::new());
        };
        let binding: DomainWorkflowBinding = serde_json::from_str(&text(&row["data"], "contract")?)
            .map_err(|error| err("storage", error.to_string()))?;
        require(
            row["type"] == "domain_workflow_binding"
                && row["definition_id"] == root.root_instance_id,
            "原领域交付约束缺失",
        )?;
        let owner = one(
            store,
            "governance",
            &domain_entity_key(&root.project_id, &root.owner_id),
        )?;
        let entity =
            extract(&owner, &root.project_id)?.ok_or_else(|| err("not_found", "原领域对象缺失"))?;
        require(
            entity["type_ref"] == binding.owner_type
                && entity["revision"] == row["data"]["owner_revision"],
            "领域工作流必须提交到原对象修订",
        )?;
        self.witness([version(&frozen), version(&owner)])?;
        let mut submitted = BTreeMap::new();
        for (port, target) in &binding.outputs {
            let source = root
                .plan
                .document
                .outputs
                .get(port)
                .ok_or_else(|| err("blocked", "工作流缺少冻结的领域交付端口"))?;
            let value = facts
                .value(source)
                .ok_or_else(|| err("blocked", "领域交付缺少原产物"))?;
            require(value.to_string().len() <= 16 * 1024, "领域对象数据超出预算")?;
            self.engine
                .tool_executor
                .as_ref()
                .ok_or_else(|| err("configuration_required", "缺少领域交付 Schema 校验器"))?
                .validate_arguments(&target.schema, value)?;
            let execution = root
                .steps
                .iter()
                .find(|step| {
                    step.step == source.step && step.status == StepExecutionStatus::Completed
                })
                .ok_or_else(|| err("blocked", "领域交付需要已接纳的实际步骤"))?;
            let output_id = execution
                .outputs
                .get(&source.port)
                .ok_or_else(|| err("blocked", "领域交付需要原始输出身份"))?;
            let original = one(store, "governance", output_id)?;
            let raw = original
                .value
                .as_ref()
                .ok_or_else(|| err("storage", "原始输出缺失"))?;
            let artifact = text(&raw["data"], "artifact_id")?;
            require(
                raw["type"] == "workflow_output" && raw["data"]["execution_id"] == execution.id,
                "领域交付产物与原执行不符",
            )?;
            self.witness([version(&original)])?;
            let step = root
                .plan
                .document
                .steps
                .iter()
                .find(|step| step.key == source.step)
                .ok_or_else(|| err("storage", "原产物步骤定义缺失"))?;
            let mut signature = None;
            if step.confirmation == ConfirmationPolicy::Human {
                let approvals = one(store, "workflow_confirmations", &execution.id)?;
                signature = approvals
                    .value
                    .as_ref()
                    .and_then(Value::as_array)
                    .and_then(|rows| {
                        rows.iter()
                            .find(|row| {
                                row["data"]["actor"]["kind"] == "human"
                                    && row["data"]["input_digest"] == execution.input_digest
                                    && row["data"]["outputs"] == json!(execution.outputs)
                            })
                            .and_then(|row| row["id"].as_str().map(str::to_owned))
                    });
                require(signature.is_some(), "领域交付缺少绑定原产物的人工签署")?;
                self.witness([version(&approvals)])?;
            }
            let id = self.object_id(&format!("domain-output:{port}"))?;
            let key = domain_entity_key(&root.project_id, &id);
            let links = target
                .owner_relations
                .iter()
                .map(|name| (name, [&root.owner_id]))
                .collect::<BTreeMap<_, _>>();
            let entity = json!({
                "relations":links,
                "id":id,"project_id":root.project_id,"type_ref":target.type_ref,"revision":1,"data":value,
                "origin":{"owner_id":root.owner_id,"owner_revision":owner.revision,
                    "root_id":root.root_instance_id,"plan_revision_id":root.active_plan_revision_id,
                    "execution_id":execution.id,"output_id":output_id,"artifact_id":artifact,
                    "input_digest":execution.input_digest,"value_digest":configuration_hash(value),
                    "producer":raw["data"]["provenance"],"decision_id":decision,"human_signature_id":signature}
            });
            let stored = storage_record(&root.project_id, &key, &binding.digest, &entity, 1)?;
            self.insert_record(&stored)?;
            submitted.insert(port.clone(), id);
        }
        Ok(submitted)
    }
}

pub(crate) fn extract(row: &ResourceSnapshot, project: &str) -> Result<Option<Value>> {
    let Some(value) = row.value.as_ref() else {
        return Ok(None);
    };
    let record = ConfigurationRecord::from_storage(value.clone())?;
    require(
        record.r#type == KIND && record.scope == Scope::project(project),
        "领域对象不属于当前项目",
    )?;
    let payload = record.data["payload"]
        .as_str()
        .ok_or_else(|| err("storage", "领域对象数据缺失"))?;
    serde_json::from_str(payload)
        .map(Some)
        .map_err(|e| err("storage", e.to_string()))
}
