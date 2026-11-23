//! Global preferences use independent resource revisions, never a project revision.
use crate::{concurrency::*, *};

pub const DEFAULT_GLOBAL_CONCURRENCY: usize = 32;
pub(crate) const DEFAULT_PROJECT_CONCURRENCY: u32 = 8;

#[derive(Debug, Clone, Serialize)]
pub struct SettingsView {
    pub global_concurrency: usize,
    pub default_project_concurrency: u32,
    pub default_harness: HarnessProvider,
    pub versions: Vec<ResourceVersion>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SettingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    global_concurrency: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_project_concurrency: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_harness: Option<HarnessProvider>,
}

fn configuration_only(store: &dyn EngineeringStore) -> Result<()> {
    if store.namespace() != StoreNamespace::Configuration {
        return Err(err("invalid_scope", "全局设置需要配置库作用域"));
    }
    Ok(())
}

pub fn read(store: &dyn EngineeringStore) -> Result<SettingsView> {
    configuration_only(store)?;
    let keys = [
        ResourceKey::new("setting", "global_concurrency"),
        ResourceKey::new("setting", "default_harness"),
        ResourceKey::new("setting", "default_project_concurrency"),
    ];
    let rows = store.read_resources(&keys)?;
    require(
        rows.len() == keys.len()
            && rows
                .iter()
                .zip(&keys)
                .all(|(row, key)| &row.resource == key),
        "设置读取不完整",
    )?;
    let concurrency = match &rows[0].value {
        None => DEFAULT_GLOBAL_CONCURRENCY,
        Some(value) => value
            .as_u64()
            .filter(|n| (1..=32).contains(n))
            .ok_or_else(|| err("storage", "已保存的全局并发设置无效"))?
            as usize,
    };
    let provider = match &rows[1].value {
        None => HarnessProvider::Codex,
        Some(value) => {
            serde_json::from_value(value.clone()).map_err(|e| err("storage", e.to_string()))?
        }
    };
    let default_project_concurrency = match &rows[2].value {
        None => DEFAULT_PROJECT_CONCURRENCY,
        Some(value) => value
            .as_u64()
            .filter(|n| (1..=32).contains(n))
            .ok_or_else(|| err("storage", "已保存的单项目默认并发设置无效"))?
            as u32,
    };
    Ok(SettingsView {
        global_concurrency: concurrency,
        default_project_concurrency,
        default_harness: provider,
        versions: rows
            .into_iter()
            .map(|row| ResourceVersion {
                resource: row.resource,
                revision: row.revision,
            })
            .collect(),
    })
}

pub fn save(
    store: &dyn EngineeringStore,
    command: &Command,
    preconditions: &ReadSet,
    context: &InvocationContext,
) -> Result<CommitReceipt> {
    configuration_only(store)?;
    if !matches!(context.actor(), WorkflowActor::Human) {
        return Err(err("permission_denied", "全局设置只接纳可信人类操作"));
    }
    require(
        command.target.is_none() && command.expected_revision.is_none(),
        "设置使用资源前置条件",
    )?;
    require(
        command
            .payload
            .as_object()
            .is_some_and(|fields| fields.values().all(|value| !value.is_null())),
        "设置字段不能为 null",
    )?;
    let patch: SettingsPatch = serde_json::from_value(command.payload.clone())
        .map_err(|e| err("invalid_params", e.to_string()))?;
    if let Some(limit) = patch.global_concurrency {
        require((1..=32).contains(&limit), "全局并发额度需为 1—32")?;
    }
    if let Some(limit) = patch.default_project_concurrency {
        require((1..=32).contains(&limit), "单项目默认并发额度需为 1—32")?;
    }
    let result = serde_json::to_value(&patch).map_err(|e| err("invalid_params", e.to_string()))?;
    let changes: Vec<_> = result
        .as_object()
        .expect("settings object")
        .iter()
        .map(|(name, value)| ResourceChange {
            resource: ResourceKey::new("setting", name),
            value: value.clone(),
        })
        .collect();
    require(!changes.is_empty(), "设置内容不能为空")?;
    preconditions.validate()?;
    for change in &changes {
        preconditions.require(&change.resource)?;
    }
    require(
        preconditions.versions().len() == changes.len(),
        "设置前置条件只能包含本次修改项",
    )?;
    let fingerprint = configuration_hash(&("settings.update", command, preconditions, context));
    store.commit_changes(&PreparedChangeSet {
        command_id: command.command_id.clone(),
        fingerprint,
        read_set: preconditions.clone(),
        changes,
        result,
        time: now() as i64,
    })
}

pub fn preconditions(params: &Value) -> Result<ReadSet> {
    let value = params
        .get("preconditions")
        .ok_or_else(|| err("precondition_required", "缺少资源版本前置条件"))?;
    let read_set: ReadSet =
        serde_json::from_value(value.clone()).map_err(|e| err("invalid_params", e.to_string()))?;
    ReadSet::new(read_set.versions().to_vec())
}
