//! Test disposal has its own once-only grant. Missing acknowledgements use
//! read-only inspection and never trigger another cleanup operation.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    platform_facts::{one, restored},
    workflow_resources::version,
    *,
};
impl Engine {
    pub(crate) fn prepare_platform_test_cleanup(
        &self,
        effect: &ConfigurationRecord,
        test: &ports::TestRequest,
        proof: &ResourceVersion,
    ) -> Result<()> {
        let selected = one(self.store.as_ref(), "governance", &proof.resource.id)?;
        require(
            selected.revision == proof.revision
                && selected.value.as_ref().is_some_and(|r| {
                    r["type"] == "platform_effect_observation" && r["definition_id"] == effect.id
                }),
            "测试清理需要原操作的原始观察",
        )?;
        let raw = selected.value.as_ref().unwrap();
        let result: Result<Option<Value>> = serde_json::from_value(raw["data"]["result"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let value =
            result?.ok_or_else(|| err("precondition_required", "测试清理缺少完成或停止证明"))?;
        let completed = serde_json::from_value::<TestResult>(value.clone()).is_ok_and(|r| {
            r.operation_id == test.operation_id
                && r.candidate == test.candidate
                && r.environment == test.environment
                && r.origin == Origin::External
        });
        require(
            completed
                || (matches!(raw["data"]["phase"].as_str(), Some("stop" | "stop-proof"))
                    && value["stopped"] == true),
            "测试清理需要原执行的实际完成或停止证明",
        )?;
        let key = ResourceKey::new("governance", format!("platform-test-cleanup:{}", effect.id));
        let previous = one(self.store.as_ref(), "governance", &key.id)?;
        if previous.value.is_some() {
            return Ok(());
        }
        let mut row = record(
            "platform_test_cleanup",
            effect.id.clone(),
            effect.scope.clone(),
            "清理已观察的测试执行".into(),
            1,
            None,
            json!({"status":"prepared","request":test,"proof":proof,"issuer":"kernel_platform"}),
            false,
        );
        row.id = key.id.clone();
        self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("platform-cleanup-prepare:{}", effect.id),
            fingerprint: configuration_hash(&row.data),
            read_set: ReadSet::new(vec![version(&previous), proof.clone()])?,
            changes: vec![ResourceChange {
                resource: key,
                value: row.to_storage(),
            }],
            result: json!({"id":row.id}),
            time: now() as i64,
        })?;
        Ok(())
    }
    pub(crate) fn drive_platform_test_cleanup(&self) -> Result<()> {
        for status in ["prepared", "running"] {
            for original in self.pending_runtime_records("platform_test_cleanup", status)? {
                let mut row = restored(self, &original)?;
                let request: ports::TestRequest =
                    serde_json::from_value(row.data["request"].clone())
                        .map_err(|e| err("storage", e.to_string()))?;
                let intent_key = ResourceKey::new(
                    "governance",
                    format!("platform-test-cleanup-grant:{}", row.id),
                );
                let mut grant = record(
                    "platform_test_cleanup_grant",
                    row.id.clone(),
                    row.scope.clone(),
                    "测试清理执行资格".into(),
                    1,
                    None,
                    json!({"status":"granted","request":request,"proof":row.data["proof"]}),
                    true,
                );
                grant.id = intent_key.id.clone();
                let mut guard = version(&original);
                let fresh = if status == "prepared" {
                    let mut data = row.data.clone();
                    data["status"] = json!("running");
                    update_record(&mut row, data);
                    let outcome = self.store.submit_changes(&PreparedChangeSet {
                        command_id: intent_key.id.clone(),
                        fingerprint: configuration_hash(&grant.data),
                        read_set: ReadSet::new(vec![
                            guard.clone(),
                            ResourceVersion {
                                resource: intent_key.clone(),
                                revision: 0,
                            },
                        ])?,
                        changes: vec![
                            ResourceChange {
                                resource: guard.resource.clone(),
                                value: row.to_storage(),
                            },
                            ResourceChange {
                                resource: intent_key,
                                value: grant.to_storage(),
                            },
                        ],
                        result: json!({"id":row.id}),
                        time: now() as i64,
                    })?;
                    if !outcome.applied {
                        continue;
                    }
                    guard.revision += 1;
                    true
                } else {
                    grant = restored(
                        self,
                        &one(self.store.as_ref(), "governance", &intent_key.id)?,
                    )?;
                    require(
                        grant.data["request"] == json!(request),
                        "原测试清理不能替换请求",
                    )?;
                    false
                };
                let result = if fresh {
                    self.tests
                        .cleanup_test(&request)
                        .map(|_| Some(json!({"cleaned":true})))
                } else {
                    match self.platform_observed(&grant, "cleanup")? {
                        Some((_, Ok(Some(value)))) if value["cleaned"] == true => Ok(Some(value)),
                        _ => self.tests.inspect_test_cleanup(&request).and_then(|done| {
                            if done {
                                Ok(Some(json!({"cleaned":true,"inspected":true})))
                            } else {
                                Err(err(
                                    "needs_reconciliation",
                                    "原清理结果未知，不重新移除容器",
                                ))
                            }
                        }),
                    }
                };
                let observed = self.platform_observe(
                    &grant,
                    if fresh { "cleanup" } else { "cleanup-proof" },
                    &result,
                )?;
                let mut data = row.data.clone();
                data["status"] = json!(if result.is_ok() {
                    "completed"
                } else {
                    "needs_reconciliation"
                });
                data["observation_id"] = json!(observed.resource.id);
                data["error"] = json!(result.as_ref().err());
                update_record(&mut row, data);
                self.store.commit_changes(&PreparedChangeSet {
                    command_id: format!("platform-cleanup-finish:{}:{}", row.id, guard.revision),
                    fingerprint: configuration_hash(&(&guard, &observed)),
                    read_set: ReadSet::new(vec![guard.clone(), observed])?,
                    changes: vec![ResourceChange {
                        resource: guard.resource,
                        value: row.to_storage(),
                    }],
                    result: json!({"id":row.id}),
                    time: now() as i64,
                })?;
            }
        }
        Ok(())
    }
}
