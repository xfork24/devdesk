//! Durable project identity reservation. Provisioning happens after this finite
//! insert; retries and competing submissions return the same reserved database.
use crate::{concurrency::*, *};
pub fn reserve(
    store: &dyn EngineeringStore,
    command: &Command,
    context: &InvocationContext,
) -> Result<String> {
    require(
        store.namespace() == StoreNamespace::Configuration,
        "项目预留需要全局目录库",
    )?;
    context.require_scope(None)?;
    if !matches!(context.actor(), WorkflowActor::Human) {
        return Err(err("permission_denied", "项目预留需要可信人类入口"));
    }
    require(
        !command.command_id.is_empty() && command.command_id.len() <= 512,
        "项目预留需要稳定命令身份",
    )?;
    require(
        command.target.is_none() && command.expected_revision.is_none(),
        "新建项目不能指定 target 或 expected_revision",
    )?;
    require(
        command.payload["title"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty()),
        "缺少 title",
    )?;
    let fingerprint = configuration_hash(&("project-reservation/v1", command, context));
    if let Some(receipt) = store.replay_commit(&command.command_id, &fingerprint)? {
        return read_id(&receipt.result);
    }
    let key = ResourceKey::new("project_reservation", &command.command_id);
    let previous = store.read_resources(std::slice::from_ref(&key))?;
    require(
        previous.len() == 1 && previous[0].resource == key,
        "项目预留读取不完整",
    )?;
    // Earlier compatible datasets already have durable reservations without an
    // operations row. Preserve their exact original request binding and identity.
    let legacy_fingerprint = serde_json::to_string(&(
        "project.create",
        &command.target,
        &command.expected_revision,
        &command.payload,
    ))
    .map_err(|e| err("invalid_params", e.to_string()))?;
    if let Some(value) = &previous[0].value {
        require(
            value["fingerprint"] == legacy_fingerprint,
            "command_id 被不同参数复用",
        )?;
        return read_id(value);
    }
    let project = id();
    let changes = vec![
        ResourceChange {
            resource: ResourceKey::new("project_directory", &project),
            value: json!({"id":project,"command_id":command.command_id,"fingerprint":legacy_fingerprint,"project":null}),
        },
        context.prepare_record("project.reserve", command),
    ];
    let mut guards = changes
        .iter()
        .map(|change| ResourceVersion {
            resource: change.resource.clone(),
            revision: 0,
        })
        .collect::<Vec<_>>();
    guards.push(ResourceVersion {
        resource: key,
        revision: 0,
    });
    let receipt = store.commit_changes(&PreparedChangeSet {
        command_id: command.command_id.clone(),
        fingerprint,
        read_set: ReadSet::new(guards)?,
        changes,
        result: json!({"id":project}),
        time: now() as i64,
    })?;
    read_id(&receipt.result)
}
fn read_id(value: &Value) -> Result<String> {
    value["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| err("storage", "项目预留回执缺少身份"))
}
