//! Git target advancement has a durable intent before the supplier is called.
use super::*;
impl Engine {
    pub(super) fn drain_advances(&self) -> Result<()> {
        let snapshot = self.store.snapshot()?;
        for intent in snapshot.advances.iter().filter(|a| {
            a.status == AdvanceStatus::Prepared
                || a.status == AdvanceStatus::Observed
                || (a.status == AdvanceStatus::NeedsReconciliation
                    && a.reason == "reconcile_requested")
        }) {
            if intent.provider != self.git.identity() {
                continue;
            }
            let Some(context) = intent.context.as_ref() else {
                continue;
            };
            let mut receipt = intent.receipt.clone();
            if receipt.is_none() {
                if intent.status == AdvanceStatus::Prepared {
                    let claimed = self.store.transact(None, &mut |s| {
                        let current = s.advances.iter().find(|a| a.id == intent.id).unwrap();
                        if current.status != AdvanceStatus::Prepared {
                            return Ok(json!(false));
                        }
                        let m = s.merge(&intent.id)?;
                        let delivery = s.delivery(&intent.delivery)?;
                        let valid = m.status == Status::Accepted
                            && m.input_version == s.feature(&m.feature_id)?.input_version
                            && self.check_merge_target(s, delivery).is_ok()
                            && s.validate_evidence(delivery).is_ok()
                            && s.accepted(delivery);
                        let current = s.advances.iter_mut().find(|a| a.id == intent.id).unwrap();
                        if !valid {
                            current.status = AdvanceStatus::NotApplied;
                            current.reason = "派发前输入/目标/验收失效，未调用供应商".into();
                            current.revision += 1;
                            event(
                                s,
                                &intent.id,
                                "merge.advance_not_dispatched",
                                "未调用供应商",
                            );
                            return Ok(json!(false));
                        }
                        current.status = AdvanceStatus::Dispatched;
                        current.revision += 1;
                        event(
                            s,
                            &intent.id,
                            "merge.advance_dispatched",
                            "目标推进已登记，崩溃后必须核对",
                        );
                        Ok(json!(true))
                    })?;
                    if claimed != true {
                        continue;
                    }
                    receipt = self
                        .git
                        .advance(
                            context,
                            &intent.expected_target,
                            &intent.expected_target,
                            &intent.candidate,
                            &intent.id,
                        )
                        .ok();
                } else {
                    match self
                        .git
                        .reconcile_advance(context, &intent.id)
                        .unwrap_or(AdvanceObservation::Unknown)
                    {
                        AdvanceObservation::Unknown => {}
                        AdvanceObservation::Applied { revision } => {
                            receipt = Some(revision);
                        }
                        AdvanceObservation::NotApplied { actual_target } => {
                            self.store.transact(None, &mut |s| {
                                let current =
                                    s.advances.iter_mut().find(|a| a.id == intent.id).unwrap();
                                if current.status == AdvanceStatus::Applied {
                                    return Ok(json!({}));
                                }
                                current.status = AdvanceStatus::NotApplied;
                                current.receipt = Some(actual_target.clone());
                                current.revision += 1;
                                current.reason =
                                    "供应商确认未执行推进，按观察目标重新构造候选".into();
                                if let Some(head) = s.heads.iter_mut().find(|head| {
                                    head.id == intent.target
                                        && head.revision == intent.expected_target
                                }) {
                                    head.revision = actual_target.clone();
                                }
                                let obsolete: Vec<_> = s
                                    .merges
                                    .iter_mut()
                                    .filter(|m| {
                                        m.target == intent.target
                                            && m.expected_target != actual_target
                                            && m.status != Status::Completed
                                    })
                                    .map(|m| {
                                        m.status = Status::Interrupted;
                                        m.revision += 1;
                                        m.id.clone()
                                    })
                                    .collect();
                                for d in s.deliveries.iter_mut().filter(|d| {
                                    d.merge_id.as_ref().is_some_and(|id| obsolete.contains(id))
                                }) {
                                    d.stale = true;
                                }
                                for r in s.runs.iter_mut().filter(|r| {
                                    r.merge_id.as_ref().is_some_and(|id| obsolete.contains(id))
                                }) {
                                    if r.status == Status::Running {
                                        r.cancel_requested = true;
                                    } else if r.status == Status::Queued {
                                        r.status = Status::Cancelled;
                                        r.finished = Some(now());
                                    }
                                }
                                event(s, &intent.id, "merge.advance_not_applied", &actual_target);
                                Ok(json!({}))
                            })?;
                            continue;
                        }
                    }
                }
                self.store.transact(None, &mut |s| {
                    let current = s.advances.iter_mut().find(|a| a.id == intent.id).unwrap();
                    if current.status == AdvanceStatus::Applied {
                        return Ok(json!({}));
                    }
                    current.receipt = receipt.clone();
                    current.status = if receipt.is_some() {
                        AdvanceStatus::Observed
                    } else {
                        AdvanceStatus::NeedsReconciliation
                    };
                    current.revision += 1;
                    current.reason = if receipt.is_some() {
                        "供应商推进回执已保存"
                    } else {
                        "供应商结果未确认，禁止重复推进"
                    }
                    .into();
                    event(
                        s,
                        &intent.id,
                        "merge.advance_observed",
                        if receipt.is_some() {
                            "已确认回执"
                        } else {
                            "需要核对"
                        },
                    );
                    Ok(json!({}))
                })?;
            }
            let Some(committed) = receipt else {
                continue;
            };
            self.store.transact(None, &mut |s| {
                let current = s.advances.iter().find(|a| a.id == intent.id).unwrap();
                if current.status == AdvanceStatus::Applied {
                    return Ok(json!({}));
                }
                let m = s.merge(&intent.id)?.clone();
                let d = s.delivery(&intent.delivery)?.clone();
                let valid = committed == m.candidate
                    && m.status != Status::Completed
                    && m.input_version == s.feature(&m.feature_id)?.input_version
                    && self.check_merge_target(s, &d).is_ok()
                    && s.validate_evidence(&d).is_ok()
                    && s.accepted(&d);
                if !valid {
                    let current = s.advances.iter_mut().find(|a| a.id == intent.id).unwrap();
                    current.status = AdvanceStatus::NeedsReconciliation;
                    current.reason =
                        "推进回执与当前输入/目标/验收不一致，需人工核对外部目标".into();
                    current.revision += 1;
                    event(
                        s,
                        &intent.id,
                        "merge.advance_conflict",
                        "保留外部回执，不伪造合入完成",
                    );
                    return Ok(json!({}));
                }
                s.heads
                    .iter_mut()
                    .find(|h| h.id == m.target)
                    .unwrap()
                    .revision = committed.clone();
                let mm = s.merges.iter_mut().find(|x| x.id == m.id).unwrap();
                mm.status = Status::Completed;
                mm.committed_revision = Some(committed.clone());
                mm.revision += 1;
                for other in s.merges.iter_mut().filter(|x| {
                    x.target == m.target && x.id != m.id && x.status != Status::Completed
                }) {
                    for d in s
                        .deliveries
                        .iter_mut()
                        .filter(|d| d.merge_id.as_deref() == Some(&other.id))
                    {
                        d.stale = true;
                    }
                }
                if let Some(tid) = m.task_id.clone() {
                    let t = s.tasks.iter_mut().find(|t| t.id == tid).unwrap();
                    t.status = Status::Completed;
                    t.revision += 1;
                } else {
                    let f = s
                        .features
                        .iter_mut()
                        .find(|f| f.id == m.feature_id)
                        .unwrap();
                    f.status = Status::Completed;
                    f.revision += 1;
                }
                let current = s.advances.iter_mut().find(|a| a.id == intent.id).unwrap();
                current.status = AdvanceStatus::Applied;
                current.revision += 1;
                current.reason = "回执、目标与验收一致，已提交工程结果".into();
                event(
                    s,
                    &intent.id,
                    "merge.completed",
                    "已核对推进回执；simulation",
                );
                Ok(json!({}))
            })?;
        }
        Ok(())
    }
}
