//! Explicit repository preparation contexts. Public dispatch remains disabled
//! until repository callbacks and their shared business constraints cut over.
use crate::{concurrency::*, *};

#[derive(Debug)]
pub struct RepositoryRegistrationFacts {
    pub project: ResourceSnapshot,
    pub repository_scope: ResourceSnapshot,
    pub feature_scope: ResourceSnapshot,
    pub has_features: bool,
    pub has_registration: bool,
    pub has_mainline: bool,
}
#[derive(Debug)]
pub struct RepositoryRefreshFacts {
    pub project: ResourceSnapshot,
    pub repository_scope: ResourceSnapshot,
    pub mainline: ResourceSnapshot,
    pub head: ResourceSnapshot,
    pub has_pending_operation: bool,
    pub has_git_call: bool,
}
fn project_id(store: &dyn EngineeringStore) -> Result<Id> {
    match store.namespace() {
        StoreNamespace::Engineering {
            project_id: Some(id),
        } => Ok(id),
        _ => Err(err("invalid_scope", "仓库操作需要明确项目库")),
    }
}
fn token(row: &ResourceSnapshot, key: ResourceKey) -> Result<ResourceVersion> {
    require(row.resource == key, "仓库读取资源与作用域不一致")?;
    Ok(ResourceVersion {
        resource: key,
        revision: row.revision,
    })
}
fn project(
    row: ResourceSnapshot,
    id: &str,
    command: &Command,
) -> Result<(Project, ResourceVersion)> {
    let version = token(&row, ResourceKey::new("project", id))?;
    let value: Project = serde_json::from_value(
        row.value
            .ok_or_else(|| err("not_found", "Project 不存在"))?,
    )
    .map_err(|e| err("storage", e.to_string()))?;
    require(
        value.id == id && value.execution_mode == "live",
        "Project 归属不符或非真实执行项目",
    )?;
    if command.expected_revision.is_none() {
        return Err(err("precondition_required", "缺少原始 Project revision"));
    }
    if command.target.as_deref() != Some(id) {
        return Err(err("invalid_scope", "仓库目标与项目库不一致"));
    }
    if command.expected_revision != Some(value.revision) {
        return Err(err("revision_conflict", "Project revision 已变化"));
    }
    Ok((value, version))
}
fn request(command: &Command, context: &InvocationContext, project: &Project) -> Result<()> {
    require(
        !command.command_id.is_empty() && command.command_id.len() <= 1024,
        "命令身份无效",
    )?;
    context.require_project(&project.id)?;
    if !matches!(context.actor(), WorkflowActor::Human) {
        return Err(err(
            "permission_denied",
            "仓库登记与主干同步需要可信人类入口",
        ));
    }
    if command.target.as_deref() != Some(project.id.as_str()) {
        return Err(err("invalid_scope", "仓库目标与项目上下文不符"));
    }
    if command.expected_revision.is_none() {
        return Err(err("precondition_required", "缺少原始 Project revision"));
    }
    if command.expected_revision != Some(project.revision) {
        return Err(err(
            "revision_conflict",
            "仓库上下文与请求的原始 revision 不符",
        ));
    }
    require(
        command
            .payload
            .as_object()
            .is_some_and(|fields| fields.values().all(|value| !value.is_null())),
        "仓库请求字段不能为 null",
    )
}

fn operation_id(method: &str, command: &Command, project: &str) -> String {
    let hash = configuration_hash(&(method, &command.command_id, project));
    format!(
        "{}-{}-{}-{}-{}",
        &hash[..8],
        &hash[8..12],
        &hash[12..16],
        &hash[16..20],
        &hash[20..32]
    )
}
fn prepared(
    method: &str,
    command: &Command,
    context: &InvocationContext,
    project: &Project,
    action: RepositoryAction,
    mut guards: Vec<ResourceVersion>,
) -> Result<PreparedChangeSet> {
    request(command, context, project)?;
    let id = operation_id(method, command, &project.id);
    let operation = RepositoryOperation {
        id: id.clone(),
        project_id: project.id.clone(),
        action,
        status: RepositoryOperationStatus::Prepared,
        receipt: None,
        reason: String::new(),
        revision: 1,
    };
    let provenance = context.prepare_record(method, command);
    let mut intent = crate::governance::record(
        "repository_intent",
        id.clone(),
        Scope::project(&project.id),
        method.into(),
        1,
        None,
        json!({"operation_id":id,"command_id":command.command_id,"command_context_id":provenance.resource.id,"action_digest":configuration_hash(&operation.action),"project_workspace_digest":configuration_hash(&(&project.workspace,&project.workspace_definition,&project.execution_mode)),"expected_project_revision":project.revision}),
        true,
    );
    intent.id = format!("repository-intent:{id}");
    let changes = vec![
        ResourceChange {
            resource: ResourceKey::new("repository_operation", &id),
            value: json!(operation),
        },
        ResourceChange {
            resource: ResourceKey::new("governance", &intent.id),
            value: intent.to_storage(),
        },
        provenance,
    ];
    guards.extend(changes.iter().map(|change| ResourceVersion {
        resource: change.resource.clone(),
        revision: 0,
    }));
    let result = PreparedChangeSet {
        command_id: command.command_id.clone(),
        fingerprint: configuration_hash(&(method, command, context)),
        read_set: ReadSet::new(guards)?,
        changes,
        result: json!({"id":id,"operation_id":id,"accepted":true}),
        time: now() as i64,
    };
    result.validate()?;
    Ok(result)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    repository: String,
    mainline: Option<String>,
}
pub struct RepositoryRegistrationContext {
    project: Project,
    guards: Vec<ResourceVersion>,
}
impl RepositoryRegistrationContext {
    pub fn read(store: &dyn EngineeringStore, command: &Command) -> Result<Self> {
        let id = project_id(store)?;
        let facts = store.repository_registration_facts()?;
        let (project, project_token) = project(facts.project, &id, command)?;
        require(!facts.has_features, "请在尚未创建 Feature 的项目中登记仓库")?;
        require(
            !facts.has_registration && !facts.has_mainline,
            "仓库登记已存在，请核对原操作",
        )?;
        Ok(Self {
            project,
            guards: vec![
                project_token,
                token(&facts.repository_scope, repository_collection(&id))?,
                token(&facts.feature_scope, feature_collection(&id))?,
            ],
        })
    }
    pub fn prepare(
        self,
        command: &Command,
        context: &InvocationContext,
    ) -> Result<PreparedChangeSet> {
        request(command, context, &self.project)?;
        let registration: Registration = serde_json::from_value(command.payload.clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        let parts = registration.repository.split('/').collect::<Vec<_>>();
        require(
            parts.len() == 2
                && parts.iter().all(|part| {
                    !part.is_empty()
                        && part
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
                }),
            "仓库应为 GitHub owner/repo",
        )?;
        let mainline = registration.mainline.unwrap_or_else(|| "main".into());
        require(!mainline.trim().is_empty(), "mainline 不可为空")?;
        prepared(
            "repository.connect",
            command,
            context,
            &self.project,
            RepositoryAction::Connect {
                repository: registration.repository,
                mainline,
            },
            self.guards,
        )
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Refresh {
    revision: String,
}
pub struct RepositoryRefreshContext {
    project: Project,
    worktree: RepositoryWorktree,
    head: GitHead,
    guards: Vec<ResourceVersion>,
}
impl RepositoryRefreshContext {
    pub fn read(store: &dyn EngineeringStore, command: &Command) -> Result<Self> {
        let id = project_id(store)?;
        let facts = store.repository_refresh_facts()?;
        let (project, project_token) = project(facts.project, &id, command)?;
        require(
            !facts.has_pending_operation && !facts.has_git_call,
            "仓库操作仍在进行或等待核对",
        )?;
        let worktree_token = token(
            &facts.mainline,
            ResourceKey::new("repository_worktree", &id),
        )?;
        let worktree: RepositoryWorktree = serde_json::from_value(
            facts
                .mainline
                .value
                .ok_or_else(|| err("blocked", "主干 worktree 缺失"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        require(
            worktree.id == id
                && worktree.project_id == id
                && worktree.owner_id == id
                && worktree.kind == WorktreeKind::Mainline,
            "主干 worktree 归属不符",
        )?;
        let head_token = token(&facts.head, ResourceKey::new("head", format!("main:{id}")))?;
        let head: GitHead = serde_json::from_value(
            facts
                .head
                .value
                .ok_or_else(|| err("blocked", "主干版本缺失"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        require(
            head.project_id == id && head.id == format!("main:{id}"),
            "主干版本归属不符",
        )?;
        Ok(Self {
            project,
            worktree,
            head,
            guards: vec![
                project_token,
                worktree_token,
                head_token,
                token(&facts.repository_scope, repository_collection(&id))?,
            ],
        })
    }
    pub fn prepare(
        self,
        command: &Command,
        context: &InvocationContext,
    ) -> Result<PreparedChangeSet> {
        request(command, context, &self.project)?;
        let refresh: Refresh = serde_json::from_value(command.payload.clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        require(
            refresh.revision.len() == 40
                && refresh
                    .revision
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit()),
            "需要已观察的远端精确 SHA",
        )?;
        let workspace = self
            .project
            .workspace_definition
            .clone()
            .ok_or_else(|| err("blocked", "仓库定位缺失"))?;
        let location = CodeLocation {
            owner: self.project.id.clone(),
            branch: self.worktree.branch,
            worktree: self.worktree.path,
        };
        let action = RepositoryAction::RefreshMainline {
            context: GitContext {
                project: self.project.id.clone(),
                workspace,
                source: location.clone(),
                target: location,
                origin: Origin::External,
            },
            expected: self.head.revision,
            revision: refresh.revision,
        };
        prepared(
            "repository.refresh",
            command,
            context,
            &self.project,
            action,
            self.guards,
        )
    }
}
