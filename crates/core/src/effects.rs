//! Durable adapter calls. No supplier call below holds EngineeringStore's writer lock.
use super::*;
impl Engine {
    fn begin_effect(&self, run: &Run, kind: EffectKind, request: Value) -> Result<EffectRecord> {
        let adapter = match kind {
            EffectKind::Harness | EffectKind::Cancel => self.harness.identity(),
            EffectKind::Test => self.tests.identity(),
            EffectKind::Cleanup => self.sandbox.identity(),
        };
        let mut request = request;
        request["adapter"] = json!(adapter);
        let key = format!("{}:{kind:?}", run.id);
        let result = self.store.transact(None, &mut |s| {
            let current = s
                .runs
                .iter()
                .find(|current| current.id == run.id)
                .ok_or_else(|| err("not_found", "Run 不存在"))?;
            require(
                current.status == Status::Running && json!(current.binding) == json!(run.binding),
                "外部调用前执行状态或绑定已变化",
            )?;
            require(
                !current.cancel_requested || kind == EffectKind::Cancel,
                "执行已请求取消，禁止发起新工作",
            )?;
            if let Some(e) = s.effects.iter().find(|e| e.id == key) {
                require(e.request == request, "同一适配器操作不得改变冻结请求")?;
                return Ok(json!(e));
            }
            let e = EffectRecord {
                id: key.clone(),
                run_id: run.id.clone(),
                kind,
                status: EffectStatus::Pending,
                request: request.clone(),
                receipt: None,
                revision: 1,
                created: now(),
                updated: now(),
                reason: "调用意图已持久化".into(),
            };
            s.effects.push(e.clone());
            event(s, &key, "effect.prepared", "供应商调用前登记意图");
            Ok(json!(e))
        })?;
        serde_json::from_value(result).map_err(|e| err("storage", e.to_string()))
    }
    fn observe_effect(&self, id: &str, receipt: Value, confirmed: bool) -> Result<()> {
        self.store.transact(None, &mut |s| {
            let e = s
                .effects
                .iter_mut()
                .find(|e| e.id == id)
                .ok_or_else(|| err("not_found", "外部操作不存在"))?;
            if [EffectStatus::Applied, EffectStatus::Discarded].contains(&e.status) {
                return Ok(json!({}));
            }
            if e.status == EffectStatus::Observed {
                require(e.receipt.as_ref() == Some(&receipt), "重复外部终态回执冲突")?;
                return Ok(json!({}));
            }
            if confirmed {
                e.receipt = Some(receipt.clone());
                e.status = EffectStatus::Observed;
            }
            e.updated = now();
            e.revision += 1;
            event(
                s,
                id,
                if confirmed {
                    "effect.observed"
                } else {
                    "effect.unconfirmed"
                },
                &receipt.to_string(),
            );
            Ok(json!({}))
        })?;
        Ok(())
    }
    pub(super) fn poll_harness(&self, run: &Run, input: &Value) -> Result<Option<Value>> {
        let kind = if run.cancel_requested {
            EffectKind::Cancel
        } else {
            EffectKind::Harness
        };
        // Freeze each Run's input once. Sibling DAG nodes may finish while this
        // supplier is still polling; their outputs must not mutate this request.
        let snapshot = self.store.snapshot()?;
        let frozen = snapshot
            .effects
            .iter()
            .find(|e| e.run_id == run.id && e.kind == EffectKind::Harness)
            .map(|e| e.request["input"].clone())
            .unwrap_or_else(|| input.clone());
        let e = self.begin_effect(
            run,
            kind,
            json!({"input":frozen,"binding":run.binding,"session":run.session_id,"node":run.node}),
        )?;
        match e.status {
            EffectStatus::Observed => return Ok(e.receipt),
            EffectStatus::Applied | EffectStatus::Discarded => return Ok(None),
            EffectStatus::NeedsReconciliation => return Err(err("blocked", "外部操作需要核对")),
            _ => {}
        }
        let output = self.harness.poll(run, &frozen, now())?;
        if let Some(value) = &output {
            self.observe_effect(
                &e.id,
                value.clone(),
                !run.cancel_requested || value["cancellation_confirmed"] == true,
            )?;
        }
        Ok(output)
    }
    pub(super) fn test_effect(&self, run: &Run, request: &TestRequest) -> Result<Value> {
        let e = self.begin_effect(
            run,
            EffectKind::Test,
            json!({"test":request,"scenario":run.outcome}),
        )?;
        if let Some(value) = e.receipt {
            return Ok(value);
        }
        require(e.status == EffectStatus::Pending, "测试操作需要核对")?;
        let output = self.tests.validate(request, &run.outcome);
        self.observe_effect(&e.id, output.clone(), true)?;
        Ok(output)
    }
    pub(super) fn finish_effects(
        &self,
        s: &mut EngineeringState,
        run: &str,
        status: EffectStatus,
        reason: &str,
    ) {
        for e in s.effects.iter_mut().filter(|e| {
            e.run_id == run
                && e.kind != EffectKind::Cleanup
                && [EffectStatus::Pending, EffectStatus::Observed].contains(&e.status)
        }) {
            e.status = status;
            e.reason = reason.into();
            e.updated = now();
            e.revision += 1;
        }
    }
    pub(super) fn enqueue_cleanup(&self, s: &mut EngineeringState, run: &str, fail: bool) {
        let resources: Vec<_> = s
            .resources
            .iter()
            .filter(|r| r.run_id == run && r.status == Status::Active)
            .cloned()
            .collect();
        for r in resources {
            let key = format!("{}:Cleanup:{}", run, r.id);
            if s.effects.iter().any(|e| e.id == key) {
                continue;
            }
            s.effects.push(EffectRecord {
                id: key,
                run_id: run.into(),
                kind: EffectKind::Cleanup,
                status: EffectStatus::Pending,
                request: json!({"resource":r,"fault_injection":fail,"adapter":self.sandbox.identity()}),
                receipt: None,
                revision: 1,
                created: now(),
                updated: now(),
                reason: "产物已封存，等待清理确认".into(),
            });
        }
    }
    pub(super) fn drain_cleanup(&self) -> Result<()> {
        let snapshot = self.store.snapshot()?;
        for e in snapshot.effects.iter().filter(|e| {
            e.kind == EffectKind::Cleanup
                && [EffectStatus::Pending, EffectStatus::Observed].contains(&e.status)
        }) {
            if e.request["adapter"] != self.sandbox.identity() {
                continue;
            }
            let mut resource: Resource = serde_json::from_value(e.request["resource"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            // Stable Resource ID lets a provider reconcile an already completed delete.
            if let Some(receipt) = &e.receipt {
                resource = serde_json::from_value(receipt.clone())
                    .map_err(|e| err("storage", e.to_string()))?;
            } else {
                self.sandbox
                    .cleanup(&mut resource, e.request["fault_injection"] == true);
                self.observe_effect(&e.id, json!(resource), true)?;
            }
            require(
                [Status::Released, Status::CleanupFailed].contains(&resource.status),
                "清理回执必须明确 released/cleanup_failed",
            )?;
            self.store.transact(None, &mut |s| {
                let current = s
                    .resources
                    .iter_mut()
                    .find(|r| r.id == resource.id)
                    .ok_or_else(|| err("not_found", "Resource 不存在"))?;
                require(current.run_id == resource.run_id, "清理回执归属错误")?;
                current.status = resource.status;
                let effect = s
                    .effects
                    .iter_mut()
                    .find(|effect| effect.id == e.id)
                    .unwrap();
                effect.status = if resource.status == Status::Released {
                    EffectStatus::Applied
                } else {
                    EffectStatus::Failed
                };
                effect.revision += 1;
                effect.updated = now();
                event(
                    s,
                    &resource.id,
                    "resource.cleanup",
                    &format!("{:?}", resource.status),
                );
                Ok(json!({}))
            })?;
        }
        Ok(())
    }
    fn confirms_termination(effect: &EffectRecord, run: &Run, value: &Value) -> bool {
        if !value["status"]
            .as_str()
            .is_some_and(|status| ["succeeded", "failed", "cancelled"].contains(&status))
        {
            return false;
        }
        match effect.kind {
            EffectKind::Harness | EffectKind::Cancel => {
                value["run_id"] == run.id && value["session_id"] == run.session_id
            }
            EffectKind::Test => {
                let Ok(result) = serde_json::from_value::<TestResult>(value.clone()) else {
                    return false;
                };
                let Ok(request) =
                    serde_json::from_value::<TestRequest>(effect.request["test"].clone())
                else {
                    return false;
                };
                result.operation_id == request.operation_id
                    && result.candidate == request.candidate
                    && result.environment == request.environment
                    && result.test_phase == request.phase
                    && result.test_definitions == request.test_definitions
                    && result.origin == request.origin
            }
            EffectKind::Cleanup => false,
        }
    }
    /// Reconciliation queries supplier facts; it never turns an interrupted Run into success.
    pub(super) fn reconcile_effects(&self) -> Result<()> {
        let snapshot = self.store.snapshot()?;
        for effect in snapshot.effects.iter().filter(|e| {
            e.status == EffectStatus::NeedsReconciliation && e.reason == "reconcile_requested"
        }) {
            let adapter = match effect.kind {
                EffectKind::Harness | EffectKind::Cancel => self.harness.identity(),
                EffectKind::Test => self.tests.identity(),
                EffectKind::Cleanup => self.sandbox.identity(),
            };
            if effect.request["adapter"] != adapter {
                continue;
            }
            let run = snapshot
                .runs
                .iter()
                .find(|r| r.id == effect.run_id)
                .ok_or_else(|| err("not_found", "Run 不存在"))?;
            let observed = if effect
                .receipt
                .as_ref()
                .is_some_and(|value| Self::confirms_termination(effect, run, value))
            {
                effect.receipt.clone()
            } else {
                match effect.kind {
                    EffectKind::Harness | EffectKind::Cancel => self.harness.reconcile(run)?,
                    EffectKind::Test => {
                        let request: TestRequest =
                            serde_json::from_value(effect.request["test"].clone())
                                .map_err(|e| err("storage", e.to_string()))?;
                        self.tests.reconcile(&request)?
                    }
                    EffectKind::Cleanup => None,
                }
            };
            self.store.transact(None, &mut |s| {
                let e = s.effects.iter_mut().find(|e| e.id == effect.id).unwrap();
                if e.status != EffectStatus::NeedsReconciliation {
                    return Ok(json!({}));
                }
                let terminal = observed
                    .as_ref()
                    .is_some_and(|value| Self::confirms_termination(effect, run, value));
                if terminal {
                    e.receipt = observed.clone();
                    e.status = EffectStatus::Discarded;
                    e.reason = "供应商终态已核对；保留中断事实，需新执行".into();
                } else {
                    e.reason = "供应商结果仍未知，保留资源与写许可".into();
                }
                e.revision += 1;
                e.updated = now();
                event(
                    s,
                    &effect.id,
                    "effect.reconciled",
                    if terminal {
                        "终态已确认，不自动接纳"
                    } else {
                        "未确认"
                    },
                );
                let unresolved = s.effects.iter().any(|e| {
                    e.run_id == run.id
                        && [
                            EffectStatus::NeedsReconciliation,
                            EffectStatus::Pending,
                            EffectStatus::Observed,
                        ]
                        .contains(&e.status)
                        && e.kind != EffectKind::Cleanup
                });
                if terminal && !unresolved {
                    self.enqueue_cleanup(s, &run.id, false);
                }
                Ok(json!({}))
            })?;
        }
        self.drain_cleanup()
    }
}
