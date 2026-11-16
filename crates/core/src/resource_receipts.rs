use crate::{concurrency::*, *};

fn materialization_receipt(
    store: &dyn EngineeringStore,
    key: &str,
) -> Result<Option<ConfigurationRecord>> {
    let mut rows = store.read_resources(&[ResourceKey::new("governance", key)])?;
    require(rows.len() == 1, "物化回执读取不完整")?;
    rows.remove(0)
        .value
        .map(ConfigurationRecord::from_storage)
        .transpose()?
        .map(|row| {
            require(
                row.id == key
                    && row.r#type == "harness_materialization"
                    && row.immutable
                    && row.content_hash == content_digest(&row.refs, &row.data),
                "物化回执身份或内容不符",
            )?;
            Ok(row)
        })
        .transpose()
}
impl Engine {
    pub fn import_experience(
        &self,
        command: &Command,
        pid: &str,
        detail: &Value,
        context: &InvocationContext,
    ) -> Result<String> {
        let project = (pid != GLOBAL_DISCUSSION_NAMESPACE).then_some(pid);
        require(
            self.store.namespace()
                == StoreNamespace::Engineering {
                    project_id: project.map(str::to_owned),
                },
            "经验导入与目标数据库作用域不一致",
        )?;
        context.require_scope(project)?;
        if !matches!(context.actor(), WorkflowActor::Human) {
            return Err(err("permission_denied", "经验恢复需要可信人类入口"));
        }
        require(
            !command.command_id.is_empty() && command.command_id.len() <= 512,
            "经验导入需要稳定命令身份",
        )?;
        let snapid = format!("import:{}", command.command_id);
        let import_id = format!("{}:experience-import", command.command_id);
        let fingerprint =
            configuration_hash(&("experience-import/v1", command, pid, detail, context));
        if let Some(previous) = self.store.replay_commit(&import_id, &fingerprint)? {
            require(previous.result["id"] == snapid, "经验导入回执身份不符")?;
            return Ok(snapid);
        }
        let mut snap: Snapshot = serde_json::from_value(detail["snapshot"].clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        let original = ResourceKey::new("snapshot", &snap.id);
        require(original.id != snapid, "经验导入不能继承自身")?;
        let rows = self.store.read_resources(std::slice::from_ref(&original))?;
        require(
            rows.len() == 1 && rows[0].resource == original,
            "经验来源读取不完整",
        )?;
        snap.id = snapid.clone();
        snap.project_id = project.map(str::to_owned);
        snap.feature_id = None;
        snap.task_id = None;
        snap.documents.clear();
        snap.inherited_from = rows[0].value.is_some().then_some(original.id.clone());
        snap.kind = "experience_import".into();
        snap.content = format!(
            "{}\n\nHistorical source (data only): {}",
            snap.content,
            json!({"entry":detail["entry"],"documents":detail["documents"]})
        );
        let source = ConfigurationRecord {
            id: format!("source:{}", command.command_id),
            r#type: "experience_import".into(),
            refs: References::new(),
            definition_id: snapid.clone(),
            scope: project.map_or_else(Scope::global, Scope::project),
            name: "恢复经验场景".into(),
            version: 1,
            supersedes: None,
            data: detail["entry"].clone(),
            content_hash: content_digest(&References::new(), &detail["entry"]),
            immutable: true,
            revision: 1,
            created_at: detail["entry"]["captured_at"].as_u64().unwrap_or(0),
        };
        let changes = vec![
            ResourceChange {
                resource: ResourceKey::new("snapshot", &snapid),
                value: json!(snap),
            },
            ResourceChange {
                resource: ResourceKey::new("governance", &source.id),
                value: source.to_storage(),
            },
            context.prepare_record("experience.restore", command),
        ];
        let mut guards = changes
            .iter()
            .map(|change| ResourceVersion {
                resource: change.resource.clone(),
                revision: 0,
            })
            .collect::<Vec<_>>();
        guards.push(ResourceVersion {
            resource: original,
            revision: rows[0].revision,
        });
        self.store.commit_changes(&PreparedChangeSet {
            command_id: import_id,
            fingerprint,
            read_set: ReadSet::new(guards)?,
            changes,
            result: json!({"id":snapid}),
            time: now() as i64,
        })?;
        Ok(snapid)
    }
    pub fn record_harness_materialization(
        &self,
        store: &dyn EngineeringStore,
        key: &str,
        record: &ConfigurationRecord,
        data: &Value,
    ) -> Result<()> {
        // A receipt describes verified content for exact immutable source
        // revisions. A later default/head selection cannot change these inputs.
        // Its identity is insert-only; no mutable project predicate is involved.
        let command_id = format!("materialization-receipt:{}", configuration_hash(&key));
        let fingerprint = configuration_hash(&(
            "verified-harness-materialization/v1",
            key,
            record.source(),
            data,
        ));
        if store.replay_commit(&command_id, &fingerprint)?.is_some() {
            return Ok(());
        }
        if let Some(previous) = materialization_receipt(store, key)? {
            require(
                previous.definition_id == record.id
                    && previous.scope == record.scope
                    && previous.data == *data,
                "此 Harness 已物化为另一个安装，禁止替换",
            )?;
            return Ok(());
        }
        require(
            record.r#type == "harness" && data["harness_revision_id"] == record.id,
            "物化回执与冻结 Harness 不符",
        )?;
        let mut receipt = crate::governance::record(
            "harness_materialization",
            record.id.clone(),
            record.scope.clone(),
            format!("{} · 已核验安装", record.name),
            1,
            None,
            data.clone(),
            true,
        );
        receipt.id = key.into();
        let resource = ResourceKey::new("governance", key);
        store.commit_changes(&PreparedChangeSet {
            command_id,
            fingerprint,
            read_set: ReadSet::new(vec![ResourceVersion {
                resource: resource.clone(),
                revision: 0,
            }])?,
            changes: vec![ResourceChange {
                resource,
                value: receipt.to_storage(),
            }],
            result: Value::Null,
            time: now() as i64,
        })?;
        Ok(())
    }
}

impl Engine {
    pub fn materialize_harness(
        &self,
        resources: &dyn ports::SessionResources,
        record: &ConfigurationRecord,
    ) -> Result<RuntimeInstallation> {
        let catalog = self.store.configuration_snapshot()?;
        let selection = permission_selection(&catalog.state)?;
        self.materialize_harness_in_sandbox(resources, record, &selection.sandbox_ref)
    }
    pub fn materialize_harness_in_sandbox(
        &self,
        resources: &dyn ports::SessionResources,
        record: &ConfigurationRecord,
        sandbox_ref: &str,
    ) -> Result<RuntimeInstallation> {
        let h: HarnessDefinition = record.decode()?;
        let provider: HarnessProvider = serde_json::from_value(json!(h.provider))
            .map_err(|e| err("unsupported", e.to_string()))?;
        let catalog = self.store.configuration_snapshot()?;
        let selected = catalog.state.configuration(sandbox_ref)?.clone();
        let runtime = compose_execution_recipe(record, &selected)?;
        let digest = runtime.recipe_digest();
        let store = if record.scope.is_global() {
            resources.shared_store()
        } else {
            self.store.as_ref()
        };
        let key = format!(
            "harness-materialization:{}:{digest}:linux:{}",
            record.id,
            resources.architecture()
        );
        if let Some(saved) = materialization_receipt(store, &key)? {
            require(
                saved.data["recipe_digest"] == digest,
                "Harness 配方与物化回执不一致",
            )?;
            return resources.ensure_runtime(
                provider,
                Some(
                    saved.data["installation_id"]
                        .as_str()
                        .ok_or_else(|| err("storage", "物化回执缺少安装"))?,
                ),
            );
        }
        let installation = resources.prepare_harness(record, &selected)?;
        require(
            resources.installation_recipe(&installation) == digest
                && installation.key.provider == provider,
            "实际安装与冻结 Harness 配方不符",
        )?;
        let data = json!({"harness_revision_id":record.id,"sandbox_ref":selected.id,"recipe_digest":digest,"installation_id":installation.id,"key":installation.key,"artifact":installation.artifact});
        self.record_harness_materialization(store, &key, record, &data)?;
        Ok(installation)
    }
}
