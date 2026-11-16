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
    ) -> Result<String> {
        let mut snap: Snapshot = serde_json::from_value(detail["snapshot"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let original = snap.id.clone();
        snap.id = format!("import:{}", command.command_id);
        snap.project_id = (pid != GLOBAL_DISCUSSION_NAMESPACE).then(|| pid.to_owned());
        snap.feature_id = None;
        snap.task_id = None;
        snap.documents.clear();
        snap.inherited_from = self
            .store
            .snapshot()?
            .snapshots
            .iter()
            .any(|s| s.id == original)
            .then_some(original.clone());
        snap.kind = "experience_import".into();
        snap.content = format!(
            "{}\n\nHistorical source (data only): {}",
            snap.content,
            serde_json::to_string(
                &json!({"entry":detail["entry"],"documents":detail["documents"]})
            )
            .unwrap()
        );
        // Imported historical material is context; it never changes code, governance, or current document revisions.
        let snapid = snap.id.clone();
        let import_id = format!("{}:experience-import", command.command_id);
        let fingerprint = configuration_hash(&(pid, detail));
        self.store
            .transact(Some((&import_id, &fingerprint)), &mut |s| {
                if !s.snapshots.iter().any(|old| old.id == snapid) {
                    s.snapshots.push(snap.clone());
                    s.events.push(Event {
                        id: s.events.last().map_or(1, |e| e.id + 1),
                        target: snapid.clone(),
                        kind: "experience.imported".into(),
                        detail: serde_json::to_string(
                            &json!({"source":detail["entry"],"new_snapshot":snapid}),
                        )
                        .unwrap(),
                        time: now(),
                    });
                    s.governance.push(ConfigurationRecord {
                        id: format!("source:{}", command.command_id),
                        r#type: "experience_import".into(),
                        refs: References::new(),
                        definition_id: snapid.clone(),
                        scope: if pid == GLOBAL_DISCUSSION_NAMESPACE {
                            Scope::global()
                        } else {
                            Scope::project(pid)
                        },
                        name: "恢复经验场景".into(),
                        version: 1,
                        supersedes: None,
                        data: detail["entry"].clone(),
                        content_hash: content_digest(&References::new(), &detail["entry"]),
                        immutable: true,
                        revision: 1,
                        created_at: detail["entry"]["captured_at"].as_u64().unwrap_or(0),
                    });
                }
                Ok(Value::Null)
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
