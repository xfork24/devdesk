//! Finite authoritative projections for the new repository protocol. Public
//! dispatch/scheduling remains disabled until shared writers cut over together.
use crate::repository_dispatch::RepositoryObservationContext;
use crate::{concurrency::*, repository_updates::RepositoryRegistrationFacts, *};

#[derive(Debug)]
pub struct RepositoryConnectProjectionFacts {
    pub registration: RepositoryRegistrationFacts,
    pub operation: ResourceSnapshot,
    pub head: ResourceSnapshot,
    pub mainline: ResourceSnapshot,
    pub has_other_pending_operation: bool,
    pub has_git_call: bool,
}

fn project_id(store: &dyn EngineeringStore) -> Result<Id> {
    match store.namespace() {
        StoreNamespace::Engineering {
            project_id: Some(id),
        } => Ok(id),
        _ => Err(err("invalid_scope", "仓库接纳需要明确项目库")),
    }
}
fn version(row: &ResourceSnapshot) -> ResourceVersion {
    ResourceVersion {
        resource: row.resource.clone(),
        revision: row.revision,
    }
}
fn receipt_matches(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(err("adapter_contract", message))
    }
}
fn immutable(row: &ResourceSnapshot, kind: &str, project: &str) -> Result<ConfigurationRecord> {
    let record = ConfigurationRecord::from_storage(
        row.value
            .clone()
            .ok_or_else(|| err("not_found", "仓库观察来源缺失"))?,
    )?;
    require(
        row.revision > 0
            && record.id == row.resource.id
            && record.r#type == kind
            && record.scope == Scope::project(project)
            && record.immutable
            && record.content_hash == content_digest(&record.refs, &record.data),
        "仓库观察来源或内容不符",
    )?;
    Ok(record)
}
struct Observation {
    project: Id,
    row: ConfigurationRecord,
    claim: ConfigurationRecord,
    intent: ConfigurationRecord,
    guards: Vec<ResourceVersion>,
    command_id: String,
    fingerprint: String,
}
impl Observation {
    fn read(store: &dyn EngineeringStore, id: &str) -> Result<Self> {
        let project = project_id(store)?;
        let key = ResourceKey::new("governance", id);
        let rows = store.read_resources(std::slice::from_ref(&key))?;
        require(
            rows.len() == 1 && rows[0].resource == key,
            "仓库观察读取不完整",
        )?;
        let row = immutable(&rows[0], "repository_observation", &project)?;
        let observer = RepositoryObservationContext::read(
            store,
            row.data["claim_id"]
                .as_str()
                .ok_or_else(|| err("provenance_required", "仓库观察缺少派发来源"))?,
        )?;
        let claim = observer.claim().clone();
        for key in [
            "intent_id",
            "command_context_id",
            "operation_id",
            "action_digest",
            "dispatched_operation_revision",
        ] {
            require(row.data[key] == claim.data[key], "仓库观察与原派发不符")?;
        }
        require(
            row.definition_id == claim.definition_id,
            "仓库观察操作身份不符",
        )?;
        let intent_key = ResourceKey::new(
            "governance",
            claim.data["intent_id"]
                .as_str()
                .ok_or_else(|| err("provenance_required", "仓库派发缺少意图"))?,
        );
        let context_key = ResourceKey::new(
            "governance",
            claim.data["command_context_id"]
                .as_str()
                .ok_or_else(|| err("provenance_required", "仓库派发缺少调用来源"))?,
        );
        let inputs = store.read_resources(&[intent_key.clone(), context_key.clone()])?;
        require(
            inputs.len() == 2
                && inputs[0].resource == intent_key
                && inputs[1].resource == context_key,
            "仓库接纳来源读取不完整",
        )?;
        let intent = immutable(&inputs[0], "repository_intent", &project)?;
        let _ = immutable(&inputs[1], "command_context", &project)?;
        let command_id = format!("repository-accept:{id}");
        let fingerprint = configuration_hash(&(
            "repository-connect-observation/v1",
            &project,
            &row.data,
            &claim.data,
            &intent.data,
        ));
        Ok(Self {
            project,
            row,
            claim,
            intent,
            guards: vec![
                version(&rows[0]),
                observer.claim_version(),
                version(&inputs[0]),
                version(&inputs[1]),
            ],
            command_id,
            fingerprint,
        })
    }
    fn prepare(&self, store: &dyn EngineeringStore) -> Result<PreparedChangeSet> {
        let facts = store.repository_connect_projection_facts(&self.row.definition_id)?;
        let operation_key = ResourceKey::new("repository_operation", &self.row.definition_id);
        require(
            facts.operation.resource == operation_key,
            "仓库操作上下文不符",
        )?;
        let mut operation: RepositoryOperation = serde_json::from_value(
            facts
                .operation
                .value
                .clone()
                .ok_or_else(|| err("not_found", "仓库操作不存在"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        require(
            operation.id == self.row.definition_id
                && operation.project_id == self.project
                && configuration_hash(&operation.action) == self.claim.data["action_digest"],
            "仓库接纳与冻结操作不符",
        )?;
        if operation.status != RepositoryOperationStatus::Dispatched
            || Some(operation.revision) != self.claim.data["dispatched_operation_revision"].as_u64()
        {
            return Err(err("revision_conflict", "仓库操作已越过原始派发版本"));
        }
        let RepositoryAction::Connect {
            repository,
            mainline,
        } = &operation.action
        else {
            return Err(err("unsupported", "此接纳仅支持仓库登记"));
        };
        let mut guards = self.guards.clone();
        guards.push(version(&facts.operation));
        let mut changes = Vec::new();
        let status = match self.row.data["observation"]["outcome"].as_str() {
            Some("failed") => {
                let code = self.row.data["observation"]["code"]
                    .as_str()
                    .ok_or_else(|| err("storage", "外部失败缺少错误码"))?;
                let message = self.row.data["observation"]["message"]
                    .as_str()
                    .ok_or_else(|| err("storage", "外部失败缺少错误说明"))?;
                operation.status = RepositoryOperationStatus::NeedsReconciliation;
                operation.reason = format!("{code} · {message}").chars().take(4096).collect();
                "needs_reconciliation"
            }
            Some("received") => {
                let receipt: RepositoryReceipt =
                    serde_json::from_value(self.row.data["observation"]["receipt"].clone())
                        .map_err(|e| err("storage", e.to_string()))?;
                let RepositoryReceipt::Connected {
                    policy,
                    workspace,
                    head,
                    worktree,
                } = &receipt
                else {
                    return Err(err("adapter_contract", "仓库登记回执类型不符"));
                };
                receipt_matches(
                    policy.compatible
                        && policy.blocked_reasons.is_empty()
                        && !policy.repository_id.is_empty()
                        && policy.repository.eq_ignore_ascii_case(repository)
                        && policy.branch == *mainline
                        && policy.head.as_ref() == Some(head),
                    "GitHub 策略与仓库登记回执不符",
                )?;
                receipt_matches(
                    head.len() == 40
                        && head.bytes().all(|byte| byte.is_ascii_hexdigit())
                        && workspace.path == format!("{}/repository", store.root())
                        && workspace.repository == *repository
                        && workspace.mainline == *mainline
                        && worktree.id == self.project
                        && worktree.project_id == self.project
                        && worktree.owner_id == self.project
                        && worktree.kind == WorktreeKind::Mainline
                        && worktree.branch == *mainline
                        && worktree.base == *head
                        && worktree.operation_id == operation.id
                        && worktree.path
                            == format!(
                                "{}/execution/worktrees/mainlines/{}",
                                store.root(),
                                self.project
                            ),
                    "仓库登记回执路径或内容不属于原请求",
                )?;
                let registration = facts.registration;
                require(
                    registration.project.resource == ResourceKey::new("project", &self.project)
                        && registration.repository_scope.resource
                            == repository_collection(&self.project)
                        && registration.feature_scope.resource == feature_collection(&self.project)
                        && facts.head.resource
                            == ResourceKey::new("head", format!("main:{}", self.project))
                        && facts.mainline.resource
                            == ResourceKey::new("repository_worktree", &self.project),
                    "仓库接纳资源与项目不符",
                )?;
                if Some(registration.repository_scope.revision)
                    != self.claim.data["repository_revision"].as_u64()
                    || registration.has_features
                    || registration.has_mainline
                    || facts.mainline.value.is_some()
                    || facts.has_other_pending_operation
                    || facts.has_git_call
                {
                    return Err(err("revision_conflict", "仓库活动或成员在原派发后已变化"));
                }
                let mut project: Project = serde_json::from_value(
                    registration
                        .project
                        .value
                        .clone()
                        .ok_or_else(|| err("not_found", "仓库登记项目不存在"))?,
                )
                .map_err(|e| err("storage", e.to_string()))?;
                if project.id != self.project
                    || project.execution_mode != "live"
                    || configuration_hash(&(
                        &project.workspace,
                        &project.workspace_definition,
                        &project.execution_mode,
                    )) != self.intent.data["project_workspace_digest"]
                {
                    return Err(err("revision_conflict", "仓库登记工作区与原始意图不符"));
                }
                if let Some(value) = &facts.head.value {
                    let old: GitHead = serde_json::from_value(value.clone())
                        .map_err(|e| err("storage", e.to_string()))?;
                    require(
                        old.project_id == self.project && old.id == facts.head.resource.id,
                        "已有主干头不属于项目",
                    )?;
                }
                project.workspace = workspace.path.clone();
                project.workspace_definition = Some(workspace.clone());
                project.revision = project
                    .revision
                    .checked_add(1)
                    .filter(|revision| *revision <= i64::MAX as u64)
                    .ok_or_else(|| err("resource_exhausted", "项目版本越界"))?;
                guards.extend([
                    version(&registration.project),
                    version(&registration.repository_scope),
                    version(&registration.feature_scope),
                    version(&facts.head),
                    version(&facts.mainline),
                ]);
                changes.extend([
                    ResourceChange {
                        resource: registration.project.resource,
                        value: json!(project),
                    },
                    ResourceChange {
                        resource: facts.head.resource,
                        value: json!(GitHead {
                            id: format!("main:{}", self.project),
                            project_id: self.project.clone(),
                            revision: head.clone()
                        }),
                    },
                    ResourceChange {
                        resource: facts.mainline.resource,
                        value: json!(worktree),
                    },
                ]);
                operation.status = RepositoryOperationStatus::Completed;
                operation.receipt = Some(receipt);
                operation.reason.clear();
                "completed"
            }
            _ => return Err(err("storage", "仓库观察结果类型无效")),
        };
        operation.revision = operation
            .revision
            .checked_add(1)
            .filter(|revision| *revision <= i64::MAX as u64)
            .ok_or_else(|| err("resource_exhausted", "仓库操作版本越界"))?;
        changes.insert(
            0,
            ResourceChange {
                resource: operation_key,
                value: json!(operation),
            },
        );
        let mut acceptance = crate::governance::record(
            "repository_acceptance",
            operation.id.clone(),
            Scope::project(&self.project),
            "仓库观察接纳".into(),
            1,
            None,
            json!({"observation_id":self.row.id,"claim_id":self.claim.id,"intent_id":self.intent.id,"command_context_id":self.claim.data["command_context_id"],"status":status}),
            true,
        );
        acceptance.id = format!("repository-acceptance:{}", self.row.id);
        let resource = ResourceKey::new("governance", &acceptance.id);
        guards.push(ResourceVersion {
            resource: resource.clone(),
            revision: 0,
        });
        changes.push(ResourceChange {
            resource,
            value: acceptance.to_storage(),
        });
        Ok(PreparedChangeSet {
            command_id: self.command_id.clone(),
            fingerprint: self.fingerprint.clone(),
            read_set: ReadSet::new(guards)?,
            changes,
            result: json!({"operation_id":operation.id,"observation_id":self.row.id,"status":status}),
            time: now() as i64,
        })
    }
}

/// Background admission of a previously persisted observation. No external I/O
/// occurs here. Only owned workspace fields are changed, after matching their
/// original digest; unrelated title/quota changes are retained from the new read.
pub fn accept_connect_observation(
    store: &dyn EngineeringStore,
    observation: &str,
) -> Result<CommitReceipt> {
    let context = Observation::read(store, observation)?;
    if let Some(receipt) = store.replay_commit(&context.command_id, &context.fingerprint)? {
        return Ok(receipt);
    }
    match context
        .prepare(store)
        .and_then(|prepared| store.commit_changes(&prepared))
    {
        Ok(receipt) => Ok(receipt),
        Err(error) => store
            .replay_commit(&context.command_id, &context.fingerprint)?
            .ok_or(error),
    }
}
