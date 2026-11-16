//! Finite Project metadata preparation. This use case is kept off the legacy
//! dispatcher until every writer sharing Project invariants has cut over.
use crate::{concurrency::*, *};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectPatch {
    title: Option<String>,
    workspace: Option<String>,
    concurrency: Option<u32>,
    workspace_definition: Option<Workspace>,
}
/// A complete native Project row and its storage token; no synthetic EngineeringState.
pub struct ProjectUpdateContext {
    project: Project,
    version: ResourceVersion,
    project_id: Id,
    workspace_frozen: bool,
    constraint_versions: Vec<ResourceVersion>,
    request_digest: String,
}
impl ProjectUpdateContext {
    pub fn read(store: &dyn EngineeringStore, command: &Command) -> Result<Self> {
        let project_id = match store.namespace() {
            StoreNamespace::Engineering {
                project_id: Some(project),
            } => project,
            _ => return Err(err("invalid_scope", "Project 元数据需要明确项目库")),
        };
        let target = command
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "缺少 Project 标识"))?;
        if target != project_id {
            return Err(err("invalid_scope", "Project 不属于本次请求的库"));
        }
        let key = ResourceKey::new("project", target);
        let mut constraint_versions = Vec::new();
        let mut workspace_frozen = false;
        let row = if command.payload.get("workspace").is_some()
            || command.payload.get("workspace_definition").is_some()
        {
            let facts = store.repository_registration_facts()?;
            workspace_frozen = facts.has_features || facts.has_mainline;
            require(
                facts.repository_scope.resource == repository_collection(&project_id)
                    && facts.feature_scope.resource == feature_collection(&project_id),
                "Project 仓库约束读取不完整",
            )?;
            constraint_versions.extend(
                [facts.repository_scope, facts.feature_scope]
                    .into_iter()
                    .map(|row| ResourceVersion {
                        resource: row.resource,
                        revision: row.revision,
                    }),
            );
            facts.project
        } else {
            store
                .read_resources(std::slice::from_ref(&key))?
                .into_iter()
                .next()
                .ok_or_else(|| err("storage", "Project 上下文读取不完整"))?
        };
        require(row.resource == key, "Project 上下文资源不符")?;
        let project: Project = serde_json::from_value(
            row.value
                .ok_or_else(|| err("not_found", "Project 不存在"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        require(project.id == project_id, "Project 元数据归属不一致")?;
        workspace_frozen &= project.execution_mode == "live";
        if command.expected_revision.is_none() {
            return Err(err("precondition_required", "缺少原始 Project revision"));
        }
        if command.expected_revision != Some(project.revision) {
            return Err(err("revision_conflict", "Project revision 已变化"));
        }
        Ok(Self {
            project,
            project_id,
            workspace_frozen,
            constraint_versions,
            request_digest: command_fingerprint("project.update", command),
            version: ResourceVersion {
                resource: key,
                revision: row.revision,
            },
        })
    }
    pub fn prepare(
        self,
        command: &Command,
        context: &InvocationContext,
    ) -> Result<PreparedChangeSet> {
        if !matches!(context.actor(), WorkflowActor::Human) {
            return Err(err(
                "permission_denied",
                "Project 元数据更新需要可信人类入口",
            ));
        }
        context.require_project(&self.project_id)?;
        require(
            self.request_digest == command_fingerprint("project.update", command),
            "Project 准备上下文不能替换原请求",
        )?;
        require(
            command.target.as_deref() == Some(self.project_id.as_str()),
            "Project 上下文与命令不符",
        )?;
        if command.expected_revision != Some(self.project.revision) {
            return Err(err("revision_conflict", "Project revision 已变化"));
        }
        require(
            command.payload.as_object().is_some_and(|fields| {
                !fields.is_empty() && fields.values().all(|value| !value.is_null())
            }),
            "Project 变更不能为空或 null",
        )?;
        let patch: ProjectPatch = serde_json::from_value(command.payload.clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        let original_workspace =
            configuration_hash(&(&self.project.workspace, &self.project.workspace_definition));
        let mut project = self.project;
        if let Some(title) = patch.title {
            require(!title.trim().is_empty(), "Project 名称不能为空")?;
            project.title = title;
        }
        if let Some(workspace) = patch.workspace {
            project.workspace = workspace.clone();
            if let Some(definition) = project.workspace_definition.as_mut() {
                definition.path = workspace;
            }
        }
        if let Some(concurrency) = patch.concurrency {
            require((1..=32).contains(&concurrency), "并发额度需为 1—32")?;
            project.concurrency = concurrency;
        }
        if let Some(definition) = patch.workspace_definition {
            require(!definition.mainline.trim().is_empty(), "mainline 不可为空")?;
            project.workspace = definition.path.clone();
            project.workspace_definition = Some(definition);
        }
        require(
            !self.workspace_frozen
                || original_workspace
                    == configuration_hash(&(&project.workspace, &project.workspace_definition)),
            "真实项目的仓库绑定不能原地切换",
        )?;
        project.revision = project
            .revision
            .checked_add(1)
            .filter(|revision| *revision <= i64::MAX as u64)
            .ok_or_else(|| err("resource_exhausted", "Project revision 越界"))?;
        let provenance = context.prepare_record("project.update", command);
        let mut guards = self.constraint_versions;
        guards.push(self.version.clone());
        guards.push(ResourceVersion {
            resource: provenance.resource.clone(),
            revision: 0,
        });
        let prepared = PreparedChangeSet {
            command_id: command.command_id.clone(),
            fingerprint: configuration_hash(&("project.update", command, context)),
            read_set: ReadSet::new(guards)?,
            changes: vec![
                ResourceChange {
                    resource: self.version.resource,
                    value: serde_json::to_value(project)
                        .map_err(|e| err("invalid_params", e.to_string()))?,
                },
                provenance,
            ],
            result: json!({"id":self.project_id}),
            time: now() as i64,
        };
        prepared.validate()?;
        Ok(prepared)
    }
}
