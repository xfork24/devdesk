use crate::*;
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
        store.transact(None, &mut |s| {
            if let Some(previous) = s.governance.iter().find(|r| r.id == key) {
                require(
                    previous.data == *data,
                    "此 Harness 已物化为另一个安装，禁止替换",
                )?;
            } else {
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
                s.governance.push(receipt);
            }
            Ok(Value::Null)
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
        let h: HarnessDefinition = record.decode()?;
        let provider: HarnessProvider = serde_json::from_value(json!(h.provider))
            .map_err(|e| err("unsupported", e.to_string()))?;
        let state = self.store.snapshot()?;
        let selected = harness_runtime_resource(&state, record)?.clone();
        let runtime: RuntimeResourceDefinition = selected.decode()?;
        let digest = runtime.recipe_digest();
        let store = if record.scope.is_global() {
            resources.shared_store()
        } else {
            self.store.as_ref()
        };
        let key = format!(
            "harness-materialization:{}:linux:{}",
            record.id,
            resources.architecture()
        );
        if let Some(saved) = store.snapshot()?.governance.iter().find(|r| r.id == key) {
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
        let data = json!({"harness_revision_id":record.id,"runtime_resource_ref":selected.id,"recipe_digest":digest,"installation_id":installation.id,"key":installation.key,"artifact":installation.artifact});
        self.record_harness_materialization(store, &key, record, &data)?;
        Ok(installation)
    }
}
