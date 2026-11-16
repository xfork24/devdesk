//! Versioned dispatch and immutable observations for new repository intents.
//! This protocol is not wired into the legacy scheduler: its authoritative
//! projection and every writer sharing repository constraints must cut over too.
use crate::{concurrency::*, *};

fn namespace(store: &dyn EngineeringStore) -> Result<Id> {
    match store.namespace() {
        StoreNamespace::Engineering {
            project_id: Some(project),
        } => Ok(project),
        _ => Err(err("invalid_scope", "仓库派发需要明确项目库")),
    }
}
fn version(row: &ResourceSnapshot) -> ResourceVersion {
    ResourceVersion {
        resource: row.resource.clone(),
        revision: row.revision,
    }
}
fn immutable_record(
    row: &ResourceSnapshot,
    kind: &str,
    project: &str,
) -> Result<ConfigurationRecord> {
    let record = ConfigurationRecord::from_storage(
        row.value
            .clone()
            .ok_or_else(|| err("provenance_required", "仓库意图缺少不可变来源记录"))?,
    )?;
    require(
        row.revision > 0
            && record.id == row.resource.id
            && record.r#type == kind
            && record.scope == Scope::project(project)
            && record.immutable
            && record.content_hash == content_digest(&record.refs, &record.data),
        "仓库意图来源身份或内容不符",
    )?;
    Ok(record)
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DispatchMode {
    Execute,
    Reconcile,
    ProjectObservation,
}

/// Complete facts for one operation and its immutable provenance, not a partial
/// EngineeringState. Legacy operations without provenance cannot enter here.
#[derive(Clone)]
pub struct RepositoryDispatchContext {
    operation: RepositoryOperation,
    intent: ConfigurationRecord,
    provenance: ConfigurationRecord,
    actor: WorkflowActor,
    guards: Vec<ResourceVersion>,
    mode: DispatchMode,
}
impl RepositoryDispatchContext {
    pub fn read(store: &dyn EngineeringStore, operation_id: &str) -> Result<Self> {
        let project = namespace(store)?;
        let operation_key = ResourceKey::new("repository_operation", operation_id);
        let intent_key =
            ResourceKey::new("governance", format!("repository-intent:{operation_id}"));
        let project_key = ResourceKey::new("project", &project);
        let rows = store.read_resources(&[
            operation_key.clone(),
            intent_key.clone(),
            project_key.clone(),
        ])?;
        require(
            rows.len() == 3
                && rows[0].resource == operation_key
                && rows[1].resource == intent_key
                && rows[2].resource == project_key,
            "仓库派发读取不完整",
        )?;
        let operation: RepositoryOperation = serde_json::from_value(
            rows[0]
                .value
                .clone()
                .ok_or_else(|| err("not_found", "仓库操作不存在"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        require(
            operation.id == operation_id && operation.project_id == project,
            "仓库派发归属不符",
        )?;
        let intent = immutable_record(&rows[1], "repository_intent", &project)?;
        require(
            intent.definition_id == operation_id
                && intent.data["operation_id"] == operation_id
                && intent.data["action_digest"] == configuration_hash(&operation.action),
            "仓库操作与原始意图不符",
        )?;
        let current_project: Project = serde_json::from_value(
            rows[2]
                .value
                .clone()
                .ok_or_else(|| err("not_found", "仓库派发项目不存在"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        if current_project.id != project
            || current_project.execution_mode != "live"
            || intent.data["project_workspace_digest"]
                != configuration_hash(&(
                    &current_project.workspace,
                    &current_project.workspace_definition,
                    &current_project.execution_mode,
                ))
        {
            return Err(err("revision_conflict", "仓库派发工作区与原始意图不符"));
        }
        let context_id = intent.data["command_context_id"]
            .as_str()
            .ok_or_else(|| err("provenance_required", "仓库意图缺少原始调用来源"))?;
        let context_key = ResourceKey::new("governance", context_id);
        let context_rows = store.read_resources(std::slice::from_ref(&context_key))?;
        require(
            context_rows.len() == 1 && context_rows[0].resource == context_key,
            "仓库来源读取不完整",
        )?;
        // Intent and provenance are immutable. Reading the referenced provenance
        // separately cannot change the original operation snapshot; all versions
        // remain guarded in the final claim transaction.
        let provenance = immutable_record(&context_rows[0], "command_context", &project)?;
        require(
            intent.data["command_id"] == provenance.definition_id
                && provenance.name == intent.name
                && provenance.data["target"] == project
                && provenance.data["context"]["project_id"] == project
                && provenance.data["request_digest"].is_string(),
            "仓库原始调用来源与意图不符",
        )?;
        let actor: WorkflowActor =
            serde_json::from_value(provenance.data["context"]["actor"].clone())
                .map_err(|e| err("provenance_required", e.to_string()))?;
        let mode = match operation.status {
            RepositoryOperationStatus::Prepared => {
                if let Some(time) = operation.reason.strip_prefix("poll_after:") {
                    let time = time
                        .parse::<u64>()
                        .map_err(|_| err("storage", "仓库重试时间无效"))?;
                    require(now() >= time, "仓库操作尚未到观察时间")?;
                }
                DispatchMode::Execute
            }
            RepositoryOperationStatus::Observed => {
                require(operation.receipt.is_some(), "已观察操作缺少外部回执")?;
                DispatchMode::ProjectObservation
            }
            RepositoryOperationStatus::NeedsReconciliation
                if operation.reason == "reconcile_requested" =>
            {
                DispatchMode::Reconcile
            }
            _ => return Err(err("revision_conflict", "仓库操作已派发或当前不可派发")),
        };
        Ok(Self {
            operation,
            intent,
            provenance,
            actor,
            guards: vec![
                version(&rows[0]),
                version(&rows[1]),
                version(&rows[2]),
                version(&context_rows[0]),
            ],
            mode,
        })
    }

    /// attempt_id is allocated by the scheduler before submission and retained
    /// for an uncertain commit result. It is never supplied as a human Actor.
    pub fn prepare(self, attempt_id: &str) -> Result<PreparedRepositoryDispatch> {
        require(
            !attempt_id.is_empty() && attempt_id.len() <= 1024,
            "仓库派发需要稳定尝试身份",
        )?;
        let claim_id = format!(
            "repository-dispatch:{}",
            configuration_hash(&(&self.operation.id, self.operation.revision, attempt_id))
        );
        let mut operation = self.operation.clone();
        operation.status = RepositoryOperationStatus::Dispatched;
        operation.revision = operation
            .revision
            .checked_add(1)
            .filter(|revision| *revision <= i64::MAX as u64)
            .ok_or_else(|| err("resource_exhausted", "仓库版本越界"))?;
        let mut claim = crate::governance::record(
            "repository_dispatch",
            operation.id.clone(),
            Scope::project(&operation.project_id),
            "仓库派发意图".into(),
            1,
            None,
            json!({"operation_id":operation.id,"intent_id":self.intent.id,"command_context_id":self.provenance.id,"actor":self.actor,"action_digest":self.intent.data["action_digest"],"expected_operation_revision":self.operation.revision,"dispatched_operation_revision":operation.revision,"attempt_id":attempt_id,"mode":self.mode}),
            true,
        );
        claim.id = claim_id.clone();
        let mut guards = self.guards;
        guards.push(ResourceVersion {
            resource: ResourceKey::new("governance", &claim_id),
            revision: 0,
        });
        let prepared = PreparedChangeSet {
            command_id: claim_id.clone(),
            fingerprint: configuration_hash(&(
                "repository-dispatch/v1",
                &self.operation,
                &self.intent.data,
                &self.provenance.data,
                attempt_id,
            )),
            read_set: ReadSet::new(guards)?,
            changes: vec![
                ResourceChange {
                    resource: ResourceKey::new("repository_operation", &operation.id),
                    value: json!(operation),
                },
                ResourceChange {
                    resource: ResourceKey::new("governance", &claim_id),
                    value: claim.to_storage(),
                },
            ],
            result: json!({"operation_id":operation.id,"claim_id":claim_id,"operation_revision":operation.revision}),
            time: now() as i64,
        };
        prepared.validate()?;
        Ok(PreparedRepositoryDispatch {
            prepared,
            dispatch: RepositoryDispatch {
                operation: self.operation,
                claim,
                mode: self.mode,
            },
        })
    }
}

pub struct PreparedRepositoryDispatch {
    prepared: PreparedChangeSet,
    dispatch: RepositoryDispatch,
}
pub enum DispatchAdmission {
    /// The newly committed claim grants one external attempt for this caller.
    Admitted(Box<RepositoryDispatch>),
    /// The claim already exists; observe/reconcile, never send the effect again.
    Replayed(CommitReceipt),
}
impl PreparedRepositoryDispatch {
    pub fn submit(self, store: &dyn EngineeringStore) -> Result<DispatchAdmission> {
        if namespace(store)? != self.dispatch.operation.project_id {
            return Err(err("invalid_scope", "仓库派发提交到其他项目"));
        }
        let outcome = store.submit_changes(&self.prepared)?;
        Ok(if outcome.applied {
            DispatchAdmission::Admitted(Box::new(self.dispatch))
        } else {
            DispatchAdmission::Replayed(outcome.receipt)
        })
    }
}

/// Created only by a newly committed claim. Original Actor and action are fixed
/// by the durable intent; callers cannot construct this from an RPC callback.
pub struct RepositoryDispatch {
    operation: RepositoryOperation,
    claim: ConfigurationRecord,
    mode: DispatchMode,
}
#[derive(Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RepositoryObservation {
    Received { receipt: Box<RepositoryReceipt> },
    Failed { code: String, message: String },
}
impl RepositoryDispatch {
    pub fn operation(&self) -> &RepositoryOperation {
        &self.operation
    }
    pub fn mode(&self) -> DispatchMode {
        self.mode
    }
    pub fn claim_id(&self) -> &str {
        &self.claim.id
    }
    /// Persist the observation even when an authoritative operation has advanced.
    /// Projection is a separate original-version commit: a late reply must never
    /// erase external evidence or silently overwrite a newer business decision.
    pub fn record_observation(
        &self,
        store: &dyn EngineeringStore,
        observation: &RepositoryObservation,
    ) -> Result<CommitReceipt> {
        RepositoryObservationContext {
            project_id: self.operation.project_id.clone(),
            operation_id: self.operation.id.clone(),
            claim: self.claim.clone(),
            claim_revision: 1,
        }
        .record(store, observation)
    }
}

/// Restart recovery grants only an observation journal, never a fresh execution
/// capability. Authoritative projection still needs its original-version guard.
pub struct RepositoryObservationContext {
    project_id: Id,
    operation_id: Id,
    claim: ConfigurationRecord,
    claim_revision: u64,
}
impl RepositoryObservationContext {
    pub fn read(store: &dyn EngineeringStore, claim_id: &str) -> Result<Self> {
        let project_id = namespace(store)?;
        let claim_key = ResourceKey::new("governance", claim_id);
        let rows = store.read_resources(std::slice::from_ref(&claim_key))?;
        require(
            rows.len() == 1 && rows[0].resource == claim_key,
            "仓库派发来源读取不完整",
        )?;
        let claim = immutable_record(&rows[0], "repository_dispatch", &project_id)?;
        let operation_id = claim.definition_id.clone();
        let intent_key =
            ResourceKey::new("governance", format!("repository-intent:{operation_id}"));
        let context_key = ResourceKey::new(
            "governance",
            claim.data["command_context_id"]
                .as_str()
                .ok_or_else(|| err("provenance_required", "仓库派发缺少原始调用来源"))?,
        );
        let inputs = store.read_resources(&[intent_key.clone(), context_key.clone()])?;
        require(
            inputs.len() == 2
                && inputs[0].resource == intent_key
                && inputs[1].resource == context_key,
            "仓库观察恢复来源不完整",
        )?;
        let intent = immutable_record(&inputs[0], "repository_intent", &project_id)?;
        let context = immutable_record(&inputs[1], "command_context", &project_id)?;
        let expected = claim.data["expected_operation_revision"]
            .as_u64()
            .filter(|revision| *revision > 0 && *revision < i64::MAX as u64)
            .ok_or_else(|| err("storage", "仓库派发缺少原始操作版本"))?;
        let dispatched = claim.data["dispatched_operation_revision"]
            .as_u64()
            .filter(|revision| *revision <= i64::MAX as u64)
            .ok_or_else(|| err("storage", "仓库派发缺少有效提交版本"))?;
        require(
            claim.data["operation_id"] == operation_id
                && intent.definition_id == operation_id
                && intent.data["operation_id"] == operation_id
                && claim.data["intent_id"] == intent.id
                && intent.data["command_context_id"] == context.id
                && intent.data["command_id"] == context.definition_id
                && context.name == intent.name
                && context.data["target"] == project_id
                && context.data["context"]["project_id"] == project_id
                && claim.data["action_digest"] == intent.data["action_digest"]
                && claim.data["actor"] == context.data["context"]["actor"]
                && expected.checked_add(1) == Some(dispatched),
            "仓库观察恢复与原始派发来源不符",
        )?;
        let _: WorkflowActor = serde_json::from_value(context.data["context"]["actor"].clone())
            .map_err(|e| err("provenance_required", e.to_string()))?;
        Ok(Self {
            project_id,
            operation_id,
            claim,
            claim_revision: rows[0].revision,
        })
    }

    pub fn record(
        &self,
        store: &dyn EngineeringStore,
        observation: &RepositoryObservation,
    ) -> Result<CommitReceipt> {
        if namespace(store)? != self.project_id {
            return Err(err("invalid_scope", "仓库回执提交到其他项目"));
        }
        let id = format!("repository-observation:{}", self.claim.id);
        let mut row = crate::governance::record(
            "repository_observation",
            self.operation_id.clone(),
            self.claim.scope.clone(),
            "仓库外部观察".into(),
            1,
            None,
            json!({"claim_id":self.claim.id,"intent_id":self.claim.data["intent_id"],"command_context_id":self.claim.data["command_context_id"],"operation_id":self.operation_id,"action_digest":self.claim.data["action_digest"],"dispatched_operation_revision":self.claim.data["dispatched_operation_revision"],"observation":observation}),
            true,
        );
        row.id = id.clone();
        store.commit_changes(&PreparedChangeSet {
            command_id: id.clone(),
            fingerprint: configuration_hash(&(
                "repository-observation/v1",
                &self.claim.id,
                &self.claim.data,
                observation,
            )),
            read_set: ReadSet::new(vec![
                ResourceVersion {
                    resource: ResourceKey::new("governance", &self.claim.id),
                    revision: self.claim_revision,
                },
                ResourceVersion {
                    resource: ResourceKey::new("governance", &id),
                    revision: 0,
                },
            ])?,
            changes: vec![ResourceChange {
                resource: ResourceKey::new("governance", &id),
                value: row.to_storage(),
            }],
            result: json!({"operation_id":self.operation_id,"observation_id":id}),
            time: now() as i64,
        })
    }
}
