//! Creation after the Host has durably reserved this project's identity. The
//! registration/provisioning lifecycle is separate from this atomic metadata.
use crate::{concurrency::*, *};

pub const INITIAL_PROJECT_REVISION: u64 = 1;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateProject {
    title: String,
    workspace: Option<String>,
    repository: Option<String>,
    mainline: Option<String>,
    use_builtin_defaults: Option<bool>,
}
fn context_id(project: &str, command: &str) -> String {
    let digest = configuration_hash(&("project-context/v1", project, command));
    format!(
        "{}-{}-{}-{}-{}",
        &digest[..8],
        &digest[8..12],
        &digest[12..16],
        &digest[16..20],
        &digest[20..32]
    )
}
pub fn create(
    store: &dyn EngineeringStore,
    command: &Command,
    context: &InvocationContext,
) -> Result<CommitReceipt> {
    create_with_concurrency(
        store,
        command,
        context,
        settings::DEFAULT_PROJECT_CONCURRENCY,
    )
}

pub fn create_with_concurrency(
    store: &dyn EngineeringStore,
    command: &Command,
    context: &InvocationContext,
    concurrency: u32,
) -> Result<CommitReceipt> {
    let project = match store.namespace() {
        StoreNamespace::Engineering {
            project_id: Some(project),
        } => project,
        _ => return Err(err("invalid_scope", "项目创建需要已经预留的明确项目库")),
    };
    context.require_project(&project)?;
    if !matches!(context.actor(), WorkflowActor::Human) {
        return Err(err("permission_denied", "项目创建需要可信人类入口"));
    }
    require(
        !command.command_id.is_empty() && command.command_id.len() <= 1024,
        "项目创建需要稳定命令身份",
    )?;
    let fingerprint = configuration_hash(&("project.create", command, context));
    if let Some(receipt) = store.replay_commit(&command.command_id, &fingerprint)? {
        return Ok(receipt);
    }
    require(
        command.target.is_none() && command.expected_revision.is_none(),
        "项目创建不能覆盖现有 target 或 revision",
    )?;
    require(
        command
            .payload
            .as_object()
            .is_some_and(|fields| fields.values().all(|value| !value.is_null())),
        "项目创建字段不能为 null",
    )?;
    let input: CreateProject = serde_json::from_value(command.payload.clone())
        .map_err(|e| err("invalid_params", e.to_string()))?;
    require(!input.title.trim().is_empty(), "项目名称不能为空")?;
    require(
        input.use_builtin_defaults != Some(true),
        "请显式选择 Workflow 并配置项目角色，不再自动采用套件",
    )?;
    let mainline = input.mainline.unwrap_or_else(|| "main".into());
    require(!mainline.trim().is_empty(), "mainline 不可为空")?;
    require((1..=32).contains(&concurrency), "并发额度需为 1—32")?;
    let workspace = input.workspace.unwrap_or_default();
    let value = Project {
        id: project.clone(),
        title: input.title,
        workspace: workspace.clone(),
        workspace_definition: Some(Workspace {
            repository: input.repository.unwrap_or_default(),
            path: workspace,
            mainline,
        }),
        execution_mode: "live".into(),
        concurrency,
        revision: INITIAL_PROJECT_REVISION,
    };
    let initial_context = Context {
        id: context_id(&project, &command.command_id),
        owner_id: project.clone(),
        content: String::new(),
        revision: 1,
    };
    let changes = vec![
        ResourceChange {
            resource: ResourceKey::new("project", &project),
            value: json!(value),
        },
        ResourceChange {
            resource: ResourceKey::new("context", &initial_context.id),
            value: json!(initial_context),
        },
        context.prepare_record("project.create", command),
    ];
    store.commit_changes(&PreparedChangeSet {
        command_id: command.command_id.clone(),
        fingerprint,
        read_set: ReadSet::new(
            changes
                .iter()
                .map(|change| ResourceVersion {
                    resource: change.resource.clone(),
                    revision: 0,
                })
                .collect(),
        )?,
        changes,
        result: json!({"id":project}),
        time: now() as i64,
    })
}
