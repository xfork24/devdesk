//! Only a newly applied intent authorizes its discovery/import attempt;
//! observations are immutable and completion never refreshes its original guards.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    *,
};

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OperationValue {
    Literal {
        value: Value,
    },
    Source {
        reference: Id,
        supersedes: Option<Id>,
        revision: u64,
        created_at: u64,
        digest: String,
    },
}
impl OperationValue {
    fn capture(
        store: &dyn EngineeringStore,
        value: &Value,
    ) -> Result<(Self, Vec<ResourceVersion>)> {
        if value["type"] == "skill"
            && let Ok(record) = serde_json::from_value::<ConfigurationRecord>(value.clone())
            && record.immutable
        {
            let key = ResourceKey::new("source", &record.id);
            let rows = store.read_resources(std::slice::from_ref(&key))?;
            require(
                rows.len() == 1 && rows[0].resource == key,
                "接入结果的源读取不完整",
            )?;
            if let Some(source) = rows[0]
                .value
                .as_ref()
                .map(|value| {
                    serde_json::from_value::<SourceDefinition>(value.clone())
                        .map_err(|e| err("storage", e.to_string()))
                })
                .transpose()?
                && source == record.source()
            {
                return Ok((
                    Self::Source {
                        reference: record.id,
                        supersedes: record.supersedes,
                        revision: record.revision,
                        created_at: record.created_at,
                        digest: configuration_hash(value),
                    },
                    vec![ResourceVersion {
                        resource: key,
                        revision: rows[0].revision,
                    }],
                ));
            }
        }
        Ok((
            Self::Literal {
                value: value.clone(),
            },
            vec![],
        ))
    }
    fn restore(self, store: &dyn EngineeringStore) -> Result<Value> {
        match self {
            Self::Literal { value } => Ok(value),
            Self::Source {
                reference,
                supersedes,
                revision,
                created_at,
                digest,
            } => {
                let key = ResourceKey::new("source", reference);
                let rows = store.read_resources(std::slice::from_ref(&key))?;
                require(
                    rows.len() == 1 && rows[0].resource == key,
                    "接入结果的源读取不完整",
                )?;
                let source: SourceDefinition = serde_json::from_value(
                    rows[0]
                        .value
                        .clone()
                        .ok_or_else(|| err("not_found", "接入结果的精确源缺失"))?,
                )
                .map_err(|e| err("storage", e.to_string()))?;
                let mut record = source.into_record()?;
                record.supersedes = supersedes;
                record.revision = revision;
                record.created_at = created_at;
                let value = json!(record);
                require(
                    configuration_hash(&value) == digest,
                    "接入结果的精确源或原回执内容不符",
                )?;
                Ok(value)
            }
        }
    }
}

pub enum IntegrationOperationAdmission {
    Admitted(Box<IntegrationOperation>),
    Replay(Value),
}
pub struct IntegrationOperation {
    namespace: StoreNamespace,
    row: ConfigurationRecord,
    revision: u64,
    context: ResourceVersion,
}
impl IntegrationOperation {
    pub fn id(&self) -> &str {
        &self.row.id
    }
}
fn fingerprint(method: &str, c: &Command, context: &InvocationContext) -> String {
    configuration_hash(&("integration-operation/v1", method, c, context))
}
fn scope(store: &dyn EngineeringStore, context: &InvocationContext) -> Result<Scope> {
    context.require_scope(store.project_id().as_deref())?;
    if !matches!(context.actor(), WorkflowActor::Human) {
        return Err(err("permission_denied", "接入准备需要可信人类入口"));
    }
    Ok(store
        .project_id()
        .as_ref()
        .map_or_else(Scope::global, Scope::project))
}
fn operation(store: &dyn EngineeringStore, id: &str) -> Result<ResourceSnapshot> {
    let key = ResourceKey::new("governance", id);
    let mut rows = store.read_resources(std::slice::from_ref(&key))?;
    require(
        rows.len() == 1 && rows[0].resource == key,
        "接入意图读取不完整",
    )?;
    Ok(rows.remove(0))
}
fn replay(
    store: &dyn EngineeringStore,
    c: &Command,
    fp: &str,
    expected_scope: &Scope,
) -> Result<Option<Value>> {
    let Some(receipt) = store.replay_commit(&c.command_id, fp)? else {
        return Ok(None);
    };
    let id = receipt.result["id"]
        .as_str()
        .ok_or_else(|| err("storage", "接入回执缺少意图身份"))?;
    let saved = operation(store, id)?;
    let row = ConfigurationRecord::from_storage(
        saved
            .value
            .ok_or_else(|| err("storage", "已提交接入意图缺失"))?,
    )?;
    require(
        row.r#type == "integration_operation"
            && row.scope == *expected_scope
            && row.definition_id == c.command_id,
        "接入回执来源不符",
    )?;
    if row.data["status"] == "succeeded" {
        return Ok(Some(
            serde_json::from_value::<OperationValue>(row.data["value"].clone())
                .map_err(|e| err("storage", e.to_string()))?
                .restore(store)?,
        ));
    }
    Err(err(
        "needs_reconciliation",
        "先前接入意图已记录；不会重复外部 I/O，请核对原操作",
    ))
}
impl Engine {
    pub fn replay_integration_operation(
        &self,
        method: &str,
        c: &Command,
        context: &InvocationContext,
    ) -> Result<Option<Value>> {
        let expected_scope = scope(self.store.as_ref(), context)?;
        replay(
            self.store.as_ref(),
            c,
            &fingerprint(method, c, context),
            &expected_scope,
        )
    }
    pub fn prepare_integration_operation(
        &self,
        method: &str,
        c: &Command,
        input: Value,
        context: &InvocationContext,
    ) -> Result<IntegrationOperationAdmission> {
        let expected_scope = scope(self.store.as_ref(), context)?;
        require(
            ["skill.import_package", "integration.discover"].contains(&method),
            "不支持的接入操作",
        )?;
        require(
            !c.command_id.is_empty() && c.command_id.len() <= 512,
            "接入操作需要稳定命令身份",
        )?;
        let fp = fingerprint(method, c, context);
        if let Some(value) = replay(self.store.as_ref(), c, &fp, &expected_scope)? {
            return Ok(IntegrationOperationAdmission::Replay(value));
        }
        let mut guards = vec![];
        if method == "integration.discover" {
            let binding = text(&input, "binding_ref")?;
            let source = text(&input, "source_ref")?;
            let keys = [
                ResourceKey::new("source", &binding),
                ResourceKey::new("source", &source),
            ];
            let rows = self.store.read_resources(&keys)?;
            require(rows.len() == keys.len(), "接入冻结源读取不完整")?;
            for (index, row) in rows.iter().enumerate() {
                require(row.resource == keys[index], "接入冻结源身份不符")?;
                let source: SourceDefinition = serde_json::from_value(
                    row.value
                        .clone()
                        .ok_or_else(|| err("not_found", "接入冻结源缺失"))?,
                )
                .map_err(|e| err("invalid_configuration", e.to_string()))?;
                let source = source.into_record()?;
                require(
                    source.r#type
                        == if index == 0 {
                            "integration_binding"
                        } else {
                            "integration"
                        },
                    "接入冻结源类型不符",
                )?;
                require(
                    source.content_hash
                        == input[if index == 0 {
                            "binding_hash"
                        } else {
                            "source_hash"
                        }],
                    "接入冻结源内容不符",
                )?;
                if index == 0 {
                    require(
                        c.target.as_ref().is_none_or(|target| target == &keys[0].id),
                        "发现动作与原绑定目标不符",
                    )?;
                    if c.expected_revision
                        .is_some_and(|expected| expected != source.revision)
                    {
                        return Err(err("revision_conflict", "接入绑定的原始修订不符"));
                    }
                    let binding: IntegrationBinding = source.decode()?;
                    require(
                        binding.integration_ref == keys[1].id,
                        "接入绑定改变了原服务来源",
                    )?;
                }
                guards.push(ResourceVersion {
                    resource: row.resource.clone(),
                    revision: row.revision,
                });
            }
        } else {
            require(
                input["package_hash"] == configuration_hash(&c.payload),
                "导入意图改变了原始包输入",
            )?;
        }
        let mut row = record(
            "integration_operation",
            c.command_id.clone(),
            expected_scope,
            method.into(),
            1,
            None,
            json!({"status":"prepared","inputs":input,"created_at":now()}),
            false,
        );
        row.id = format!(
            "integration-op-{}",
            &configuration_hash(&(self.store.namespace(), &c.command_id))[..32]
        );
        let context_change = context.prepare_record(method, c);
        let context_key = context_change.resource.clone();
        let changes = vec![
            ResourceChange {
                resource: ResourceKey::new("governance", &row.id),
                value: row.to_storage(),
            },
            context_change,
        ];
        guards.extend(changes.iter().map(|change| ResourceVersion {
            resource: change.resource.clone(),
            revision: 0,
        }));
        let outcome = self.store.submit_changes(&PreparedChangeSet {
            command_id: c.command_id.clone(),
            fingerprint: fp,
            read_set: ReadSet::new(guards)?,
            changes,
            result: json!({"id":row.id}),
            time: now() as i64,
        })?;
        if !outcome.applied {
            return self
                .replay_integration_operation(method, c, context)?
                .map(IntegrationOperationAdmission::Replay)
                .ok_or_else(|| err("storage", "接入重放缺少回执"));
        }
        let revision = outcome
            .receipt
            .versions
            .iter()
            .find(|v| v.resource == ResourceKey::new("governance", &row.id))
            .ok_or_else(|| err("storage", "接入提交缺少资源版本"))?
            .revision;
        let context = outcome
            .receipt
            .versions
            .iter()
            .find(|v| v.resource == context_key)
            .ok_or_else(|| err("storage", "接入提交缺少调用来源版本"))?
            .clone();
        Ok(IntegrationOperationAdmission::Admitted(Box::new(
            IntegrationOperation {
                namespace: self.store.namespace(),
                row,
                revision,
                context,
            },
        )))
    }
    pub fn finish_integration_operation(
        &self,
        operation: &IntegrationOperation,
        result: &Result<Value>,
    ) -> Result<()> {
        if operation.namespace != self.store.namespace() {
            return Err(err("invalid_scope", "接入观察不能提交到另一个命名空间"));
        }
        let (captured, source_guards) = match result {
            Ok(value) => {
                let (captured, guards) = OperationValue::capture(self.store.as_ref(), value)?;
                (Some(captured), guards)
            }
            Err(_) => (None, vec![]),
        };
        let captured = captured
            .map(|value| serde_json::to_value(value).map_err(|e| err("storage", e.to_string())))
            .transpose()?;
        let observed = match result {
            Ok(_) => json!({"result":captured}),
            Err(error) => json!({"error":error}),
        };
        let digest = configuration_hash(&(
            "integration-observation/v1",
            &operation.row.id,
            operation.row.revision,
            operation.revision,
            &operation.context,
            &observed,
        ));
        let observation_id = format!("integration-observation:{digest}");
        let command_id = format!("integration-observe:{digest}");
        let observation_key = ResourceKey::new("governance", &observation_id);
        if self.store.replay_commit(&command_id, &digest)?.is_none() {
            let mut observation = record(
                "integration_observation",
                operation.row.id.clone(),
                operation.row.scope.clone(),
                "接入外部观察".into(),
                1,
                None,
                json!({"intent_id":operation.row.id,"intent_revision":operation.row.revision,"resource_revision":operation.revision,"command_context_id":operation.context.resource.id,"intent_digest":operation.row.content_hash,"value":observed}),
                true,
            );
            observation.id = observation_id.clone();
            self.store.commit_changes(&PreparedChangeSet {
                command_id,
                fingerprint: digest.clone(),
                read_set: ReadSet::new(vec![ResourceVersion {
                    resource: observation_key.clone(),
                    revision: 0,
                }])?,
                changes: vec![ResourceChange {
                    resource: observation_key.clone(),
                    value: observation.to_storage(),
                }],
                result: json!({"id":observation_id}),
                time: now() as i64,
            })?;
        }
        let accept_id = format!("integration-accept:{digest}");
        if self.store.replay_commit(&accept_id, &digest)?.is_some() {
            return Ok(());
        }
        // This resource family has no other writer. Use the exact admitted row,
        // including its frozen input, rather than reading/rebasing the callback.
        let mut row = operation.row.clone();
        let mut data = row.data.clone();
        match result {
            Ok(_) => {
                data["status"] = json!("succeeded");
                data["value"] = captured.unwrap();
            }
            Err(error) => {
                data["status"] = json!("failed");
                data["error"] = json!(error);
            }
        }
        data["observation_id"] = json!(observation_id);
        data["finished_at"] = json!(now());
        update_record(&mut row, data);
        let mut guards = source_guards;
        guards.extend([
            ResourceVersion {
                resource: ResourceKey::new("governance", &row.id),
                revision: operation.revision,
            },
            operation.context.clone(),
            ResourceVersion {
                resource: observation_key,
                revision: 1,
            },
        ]);
        self.store.commit_changes(&PreparedChangeSet {
            command_id: accept_id,
            fingerprint: digest,
            read_set: ReadSet::new(guards)?,
            changes: vec![ResourceChange {
                resource: ResourceKey::new("governance", &row.id),
                value: row.to_storage(),
            }],
            result: json!({"recorded":true}),
            time: now() as i64,
        })?;
        Ok(())
    }
}
