//! Immutable installation registration has its own identity and absence guard.
use crate::{concurrency::*, *};

pub fn installation_id(key: &RuntimeKey, recipe_digest: Option<&str>) -> String {
    format!("runtime:{}", configuration_hash(&(key, recipe_digest)))
}
fn seal_key(id: &str) -> ResourceKey {
    ResourceKey::new("setting", format!("runtime.seal.{id}"))
}
fn content_seal(registration: &RuntimeRegistration, recipe: Option<&str>) -> String {
    let content = match &registration.artifact {
        RuntimeArtifact::Executable { sha256, .. } => json!({"kind":"executable","sha256":sha256}),
        artifact => json!(artifact),
    };
    configuration_hash(&(&registration.key, recipe, content))
}

pub fn registration_fingerprint(command: &Command, context: &InvocationContext) -> String {
    configuration_hash(&("runtime.register", command, context))
}

pub fn registration_preconditions(command: &Command) -> Result<()> {
    require(!command.command_id.is_empty(), "安装登记需要命令身份")?;
    match command.expected_revision {
        None => Err(err(
            "precondition_required",
            "安装登记需要明确的不存在前置条件 0",
        )),
        Some(0) => Ok(()),
        Some(_) => Err(err("revision_conflict", "安装不可覆盖，请登记新的内容身份")),
    }
}

/// The adapter verifies executable/image contents before calling this use case.
/// No external operation, complete state, or business lock enters the commit.
pub fn register_verified(
    store: &dyn EngineeringStore,
    command: &Command,
    registration: RuntimeRegistration,
    recipe_digest: Option<String>,
    context: &InvocationContext,
) -> Result<CommitReceipt> {
    require(
        store.namespace() == StoreNamespace::Configuration,
        "安装登记需要全局配置库",
    )?;
    let fingerprint = registration_fingerprint(command, context);
    if let Some(saved) = store.replay_commit(&command.command_id, &fingerprint)? {
        return Ok(saved);
    }
    registration_preconditions(command)?;
    registration.key.validate()?;
    require(
        !registration.source.trim().is_empty() && !registration.license.trim().is_empty(),
        "需要记录运行时来源与许可证",
    )?;
    let id = installation_id(&registration.key, recipe_digest.as_deref());
    require(
        command.target.as_ref().is_none_or(|target| target == &id),
        "安装目标与版本/平台身份不一致",
    )?;
    let resource = ResourceKey::new("runtime_installation", &id);
    let seal = seal_key(&id);
    let sealed_content = content_seal(&registration, recipe_digest.as_deref());
    let installation = RuntimeInstallation {
        id,
        key: registration.key,
        artifact: registration.artifact,
        source: registration.source,
        license: registration.license,
        recipe_digest,
    };
    let value = json!({
        "id":installation.id,"provider":installation.key.provider,"version":installation.key.version,
        "os":installation.key.os,"arch":installation.key.arch,"artifact":installation.artifact,
        "source":installation.source,"license":installation.license,"recipe_digest":installation.recipe_digest
    });
    store.commit_changes(&PreparedChangeSet {
        command_id: command.command_id.clone(),
        fingerprint,
        read_set: ReadSet::new(vec![
            ResourceVersion {
                resource: resource.clone(),
                revision: 0,
            },
            ResourceVersion {
                resource: seal.clone(),
                revision: 0,
            },
        ])?,
        changes: vec![
            ResourceChange { resource, value },
            ResourceChange {
                resource: seal,
                value: json!(sealed_content),
            },
        ],
        result: json!(installation),
        time: now() as i64,
    })
}

/// Repair a missing registry row only from verified content matching its original
/// immutable seal. Its tombstone version and current absence both remain guarded.
/// This port is used by the installer, never by the public registration command.
pub fn recover_verified_missing(
    store: &dyn EngineeringStore,
    registration: RuntimeRegistration,
    recipe: Option<String>,
) -> Result<Option<RuntimeInstallation>> {
    require(
        store.namespace() == StoreNamespace::Configuration,
        "安装恢复需要全局配置库",
    )?;
    let id = installation_id(&registration.key, recipe.as_deref());
    let resource = ResourceKey::new("runtime_installation", &id);
    let seal = seal_key(&id);
    let presence = ResourceKey::new("runtime_presence", &id);
    let rows = store.read_resources(&[resource.clone(), seal.clone(), presence.clone()])?;
    require(rows.len() == 3, "安装恢复读取不完整")?;
    if rows[0].value.is_some() || rows[0].revision == 0 {
        return Ok(None);
    }
    require(
        rows[1].value == Some(json!(content_seal(&registration, recipe.as_deref()))),
        "缺失安装与原封存内容不一致，禁止替换历史身份",
    )?;
    let installation = RuntimeInstallation {
        id,
        key: registration.key,
        artifact: registration.artifact,
        source: registration.source,
        license: registration.license,
        recipe_digest: recipe,
    };
    let value = json!({"id":installation.id,"provider":installation.key.provider,"version":installation.key.version,"os":installation.key.os,"arch":installation.key.arch,"artifact":installation.artifact,"source":installation.source,"license":installation.license,"recipe_digest":installation.recipe_digest});
    let prepared = PreparedChangeSet {
        command_id: crate::id(),
        fingerprint: configuration_hash(&(
            "runtime-registry-restoration/v1",
            &installation,
            rows[0].revision,
        )),
        read_set: ReadSet::new(
            rows.into_iter()
                .map(|row| ResourceVersion {
                    resource: row.resource,
                    revision: row.revision,
                })
                .collect(),
        )?,
        changes: vec![ResourceChange { resource, value }],
        result: json!(installation),
        time: now() as i64,
    };
    let receipt = store.commit_changes(&prepared)?;
    serde_json::from_value(receipt.result)
        .map(Some)
        .map_err(|error| err("storage", error.to_string()))
}

pub fn read_installation(
    store: &dyn EngineeringStore,
    id: &str,
) -> Result<Option<RuntimeInstallation>> {
    require(
        store.namespace() == StoreNamespace::Configuration,
        "安装读取需要全局配置库",
    )?;
    let rows = store.read_resources(&[ResourceKey::new("runtime_installation", id)])?;
    let row = rows
        .first()
        .ok_or_else(|| err("storage", "安装读取不完整"))?;
    row.value.as_ref().map(|value| {
        serde_json::from_value(json!({
            "id":value["id"],"key":{"provider":value["provider"],"version":value["version"],"os":value["os"],"arch":value["arch"]},
            "artifact":value["artifact"],"source":value["source"],"license":value["license"],"recipe_digest":value["recipe_digest"]
        })).map_err(|error|err("storage",error.to_string()))
    }).transpose()
}
