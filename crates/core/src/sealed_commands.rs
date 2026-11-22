//! Local material writes retain their exact finite admission plan across file
//! I/O. Recovery authenticates the original file identity before admitting it.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    *,
};

fn key(store: &dyn EngineeringStore, command: &str) -> ResourceKey {
    ResourceKey::new(
        "governance",
        format!(
            "sealed-command:{}",
            configuration_hash(&(
                "sealed-command-id/v1",
                store.namespace(),
                store.root(),
                command
            ))
        ),
    )
}
fn version(row: &ResourceSnapshot) -> ResourceVersion {
    ResourceVersion {
        resource: row.resource.clone(),
        revision: row.revision,
    }
}

impl Engine {
    pub(crate) fn replay_sealed_command(
        &self,
        command: &str,
        fingerprint: &str,
    ) -> Result<Option<Value>> {
        if let Some(receipt) = self.store.replay_commit(command, fingerprint)? {
            return Ok(Some(receipt.result));
        }
        let key = key(self.store.as_ref(), command);
        let mut rows = self.store.read_resources(std::slice::from_ref(&key))?;
        require(
            rows.len() == 1 && rows[0].resource == key,
            "材料命令意图读取不完整",
        )?;
        let original = rows.remove(0);
        let Some(raw) = &original.value else {
            return Ok(None);
        };
        require(
            raw["data"]["fingerprint"] == fingerprint,
            "command_id 被另一份材料请求复用",
        )?;
        let _attempt = self.effect_in_flight.enter(&key.id)?.ok_or_else(|| {
            err(
                "resource_pending",
                "原材料命令仍在封存或接纳，请保留原命令身份",
            )
        })?;
        self.admit_sealed_command(&original).map(Some)
    }

    pub(crate) fn seal_command_changes(
        &self,
        planned: PreparedChangeSet,
        seal: ArtifactSeal,
        body: &str,
        scope: Scope,
    ) -> Result<Value> {
        self.seal_command_batch_changes(planned, &[(seal, body)], scope)
    }

    pub(crate) fn seal_command_batch_changes(
        &self,
        planned: PreparedChangeSet,
        files: &[(ArtifactSeal, &str)],
        scope: Scope,
    ) -> Result<Value> {
        if let Some(result) =
            self.replay_sealed_command(&planned.command_id, &planned.fingerprint)?
        {
            return Ok(result);
        }
        planned.validate()?;
        require(
            !files.is_empty() && files.len() <= 64,
            "一次材料命令需要 1—64 个确切封存身份",
        )?;
        require(
            files
                .iter()
                .map(|(seal, _)| &seal.artifact.id)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == files.len(),
            "一次材料命令不能复用文件身份",
        )?;
        for (seal, body) in files {
            seal.validate_body(body)?;
        }
        require(
            serde_json::to_vec(&planned.changes)
                .map_err(|e| err("invalid_params", e.to_string()))?
                .len()
                <= 32 * 1024 * 1024,
            "一次材料命令的有限变化超出封存预算",
        )?;
        let seals = files.iter().map(|(seal, _)| seal).collect::<Vec<_>>();
        let key = key(self.store.as_ref(), &planned.command_id);
        // Register before publishing the prepared intent. A recovery scan must
        // not inspect a live attempt's files before their observations exist.
        let _attempt = self.effect_in_flight.enter(&key.id)?.ok_or_else(|| {
            err(
                "resource_pending",
                "原材料命令仍在封存或接纳，请保留原命令身份",
            )
        })?;
        let mut intent = record(
            "sealed_command_intent",
            planned.command_id.clone(),
            scope,
            "封存原材料命令".into(),
            1,
            None,
            json!({"status":"prepared","namespace":self.store.namespace(),"root":self.store.root(),"command_id":planned.command_id,"fingerprint":planned.fingerprint,
                "seals":seals,"read_set":planned.read_set,"changes":planned.changes,"result":planned.result}),
            false,
        );
        intent.id = key.id.clone();
        let outcome = self.store.submit_changes(&PreparedChangeSet {
            command_id: format!("{}:prepare", key.id),
            fingerprint: configuration_hash(&(
                "sealed-command-prepare/v2",
                &intent.data["fingerprint"],
                &seals,
            )),
            read_set: planned.read_set.merge([ResourceVersion {
                resource: key.clone(),
                revision: 0,
            }])?,
            changes: vec![ResourceChange {
                resource: key.clone(),
                value: self.store.encode_runtime_record(&intent)?,
            }],
            result: json!({"id":key.id}),
            time: now() as i64,
        })?;
        if outcome.applied {
            for (index, (seal, body)) in files.iter().enumerate() {
                let result = self.artifacts.seal_prepared(seal, body);
                let observed =
                    ResourceKey::new("governance", format!("{}:observation:{index}", key.id));
                self.journal_sealed_command(&intent, &observed, seal, &result, false)?;
            }
        }
        let original = self
            .store
            .read_resources(std::slice::from_ref(&key))?
            .into_iter()
            .next()
            .ok_or_else(|| err("storage", "原材料命令意图缺失"))?;
        self.admit_sealed_command(&original)
    }

    fn journal_sealed_command(
        &self,
        intent: &ConfigurationRecord,
        key: &ResourceKey,
        seal: &ArtifactSeal,
        result: &Result<Artifact>,
        reconciled: bool,
    ) -> Result<()> {
        let mut proof = record(
            "sealed_command_observation",
            intent.id.clone(),
            intent.scope.clone(),
            "原身份文件封存观察".into(),
            1,
            None,
            json!({"seal":seal,"reconciled":reconciled,"value":{"result":result}}),
            true,
        );
        proof.id = key.id.clone();
        self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("{}:journal", key.id),
            fingerprint: configuration_hash(&proof.data),
            read_set: ReadSet::new(vec![ResourceVersion {
                resource: key.clone(),
                revision: 0,
            }])?,
            changes: vec![ResourceChange {
                resource: key.clone(),
                value: proof.to_storage(),
            }],
            result: json!({"id":proof.id}),
            time: now() as i64,
        })?;
        Ok(())
    }

    fn admit_sealed_command(&self, original: &ResourceSnapshot) -> Result<Value> {
        let mut intent = ConfigurationRecord::from_storage(
            self.store.restore_runtime_record(
                original
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "材料命令意图缺失"))?,
            )?,
        )?;
        require(
            intent.r#type == "sealed_command_intent"
                && intent.data["namespace"] == json!(self.store.namespace())
                && intent.data["root"] == self.store.root(),
            "材料命令不能跨数据集恢复",
        )?;
        let command = text(&intent.data, "command_id")?;
        let fingerprint = text(&intent.data, "fingerprint")?;
        if let Some(receipt) = self.store.replay_commit(&command, &fingerprint)? {
            return Ok(receipt.result);
        }
        if intent.data["status"] == "conflicted" {
            return Err(serde_json::from_value(intent.data["error"].clone())
                .map_err(|e| err("storage", e.to_string()))?);
        }
        require(
            matches!(
                intent.data["status"].as_str(),
                Some("prepared" | "needs_reconciliation")
            ),
            "原材料命令已结束",
        )?;
        let seals: Vec<ArtifactSeal> = serde_json::from_value(intent.data["seals"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        require(
            !seals.is_empty() && seals.len() <= 64,
            "原材料命令的封存身份数量无效",
        )?;
        let mut witnesses = vec![version(original)];
        let mut artifacts = Vec::new();
        let mut observation_ids = Vec::new();
        for (index, seal) in seals.iter().enumerate() {
            let observed_key =
                ResourceKey::new("governance", format!("{}:observation:{index}", intent.id));
            let mut observations = self
                .store
                .read_resources(std::slice::from_ref(&observed_key))?;
            require(observations.len() == 1, "原文件观察读取不完整")?;
            let result = observations[0]
                .value
                .as_ref()
                .map(|v| {
                    require(
                        v["type"] == "sealed_command_observation"
                            && v["definition_id"] == intent.id
                            && v["data"]["seal"] == json!(seal),
                        "封存观察不能更改原文件身份或意图",
                    )?;
                    serde_json::from_value::<Result<Artifact>>(v["data"]["value"]["result"].clone())
                        .map_err(|e| err("storage", e.to_string()))
                })
                .transpose()?;
            let artifact = if let Some(Ok(artifact)) = result {
                artifact
            } else {
                let artifact = self.artifacts.inspect_prepared(seal)?.ok_or_else(|| {
                    err(
                        "material_seal_unresolved",
                        "原身份封存结果尚未核实；原草稿、版本及已知观察保留",
                    )
                })?;
                let key =
                    ResourceKey::new("governance", format!("{}:reconciled:{index}", intent.id));
                self.journal_sealed_command(&intent, &key, seal, &Ok(artifact.clone()), true)?;
                observations = self.store.read_resources(std::slice::from_ref(&key))?;
                require(observations.len() == 1, "原文件核对观察读取不完整")?;
                artifact
            };
            require(
                json!(artifact) == json!(seal.artifact),
                "文件回执改变了预分配身份、内容或归属",
            )?;
            witnesses.push(version(&observations[0]));
            observation_ids.push(observations[0].resource.id.clone());
            artifacts.push(artifact);
        }
        let read_set: ReadSet = serde_json::from_value(intent.data["read_set"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let mut changes: Vec<ResourceChange> =
            serde_json::from_value(intent.data["changes"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        for artifact in artifacts {
            let key = ResourceKey::new("artifact", &artifact.id);
            require(
                !changes.iter().any(|c| c.resource == key),
                "材料计划不能重复提交封存回执",
            )?;
            witnesses.push(ResourceVersion {
                resource: key.clone(),
                revision: 0,
            });
            changes.push(ResourceChange {
                resource: key,
                value: json!(artifact),
            });
        }
        let mut data = intent.data.clone();
        let result = data
            .as_object_mut()
            .unwrap()
            .remove("result")
            .unwrap_or(Value::Null);
        data.as_object_mut().unwrap().remove("changes");
        data.as_object_mut().unwrap().remove("read_set");
        data["status"] = json!("completed");
        data["observation_ids"] = json!(observation_ids);
        update_record(&mut intent, data);
        changes.push(ResourceChange {
            resource: original.resource.clone(),
            value: self.store.encode_runtime_record(&intent)?,
        });
        let plan = PreparedChangeSet {
            command_id: command.clone(),
            fingerprint: fingerprint.clone(),
            read_set: read_set.merge(witnesses)?,
            changes,
            result,
            time: now() as i64,
        };
        match self.store.commit_changes(&plan) {
            Ok(receipt) => Ok(receipt.result),
            Err(error) if error.code == "revision_conflict" => {
                if let Some(receipt) = self.store.replay_commit(&command, &fingerprint)? {
                    return Ok(receipt.result);
                }
                let mut conflict = ConfigurationRecord::from_storage(
                    self.store
                        .restore_runtime_record(original.value.as_ref().unwrap())?,
                )?;
                let mut data = conflict.data.clone();
                data["status"] = json!("conflicted");
                data["error"] = json!(error);
                update_record(&mut conflict, data);
                self.store.commit_changes(&PreparedChangeSet {
                    command_id: format!("{}:conflicted", intent.id),
                    fingerprint: configuration_hash(&(
                        "sealed-command-conflict/v1",
                        &intent.id,
                        &error,
                    )),
                    read_set: ReadSet::new(vec![version(original)])?,
                    changes: vec![ResourceChange {
                        resource: original.resource.clone(),
                        value: self.store.encode_runtime_record(&conflict)?,
                    }],
                    result: json!({"id":intent.id,"status":"conflicted"}),
                    time: now() as i64,
                })?;
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn resume_sealed_commands(&self) -> Result<()> {
        for original in self.pending_runtime_records("sealed_command_intent", "prepared")? {
            let Some(_attempt) = self.effect_in_flight.enter(&original.resource.id)? else {
                continue;
            };
            match self.admit_sealed_command(&original) {
                Err(error)
                    if matches!(error.code.as_str(), "revision_conflict" | "storage_busy") =>
                {
                    continue;
                }
                Err(error) => {
                    let mut intent = ConfigurationRecord::from_storage(
                        self.store.restore_runtime_record(
                            original
                                .value
                                .as_ref()
                                .ok_or_else(|| err("storage", "材料恢复意图缺失"))?,
                        )?,
                    )?;
                    let mut data = intent.data.clone();
                    data["status"] = json!("needs_reconciliation");
                    data["error"] = json!(error);
                    update_record(&mut intent, data);
                    let outcome = self.store.commit_changes(&PreparedChangeSet {
                        command_id: format!(
                            "{}:recovery-deferred:{}",
                            intent.id, original.revision
                        ),
                        fingerprint: configuration_hash(&(
                            "sealed-command-defer/v1",
                            version(&original),
                            &error,
                        )),
                        read_set: ReadSet::new(vec![version(&original)])?,
                        changes: vec![ResourceChange {
                            resource: original.resource.clone(),
                            value: self.store.encode_runtime_record(&intent)?,
                        }],
                        result: json!({"id":intent.id,"status":"needs_reconciliation"}),
                        time: now() as i64,
                    });
                    if let Err(error) = outcome
                        && !matches!(error.code.as_str(), "revision_conflict" | "storage_busy")
                    {
                        return Err(error);
                    }
                }
                Ok(_) => {}
            }
        }
        Ok(())
    }
}
