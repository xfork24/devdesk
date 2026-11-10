//! Discussion permission changes create successors; Tasks require an explicit new Session.
use crate::*;

pub fn permission_switch_id(command: &str) -> String {
    format!("permission-switch:{}", configuration_hash(&json!(command)))
}

impl Engine {
    /// Trusted Host entry, deliberately not exposed as a client-supplied binding command.
    pub fn send_with_permission_binding(
        &self,
        c: Command,
        new_binding: RuntimeBinding,
    ) -> Result<Value> {
        if c.command_id.is_empty() {
            return Err(err("invalid_params", "command_id 不能为空"));
        }
        let fp = command_fingerprint("conversation.send", &c);
        if let Some(previous) = self.store.replay(&c.command_id, &fp)? {
            return Ok(previous);
        }
        let sid = c
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "需要 Session"))?;
        let intent_id = permission_switch_id(&c.command_id);
        self.store.transact(None, &mut |s| {
            let session = s
                .sessions
                .iter()
                .find(|r| r.id == sid)
                .ok_or_else(|| err("not_found", "Session 不存在"))?;
            require(
                session.scope != Some(SessionScope::Task),
                "Task 会话的运行基线已冻结；更换权限或集成时请显式新建 Session",
            )?;
            if let Some(intent) = s.governance.iter().find(|r| r.id == intent_id) {
                if intent.data["fingerprint"] != fp {
                    return Err(err("revision_conflict", "切换命令被不同输入复用"));
                }
                return Ok(Value::Null);
            }
            if c.expected_revision != Some(session.revision) {
                return Err(err("revision_conflict", "Session 已改变"));
            }
            if !session.runtime_binding.as_ref().is_some_and(|b| {
                let mut expected = b.clone();
                expected.permissions = new_binding.permissions.clone();
                expected.configuration_digest = new_binding.configuration_digest.clone();
                [WorkType::Discussion, WorkType::Design].contains(&b.work_type)
                    && expected == new_binding
            }) {
                return Err(err(
                    "permission_conflict",
                    "仅允许相同 Agent 与运行时的讨论切换",
                ));
            }
            if s.turns
                .iter()
                .any(|t| t.session_id == sid && t.status.active())
                || s.runs
                    .iter()
                    .any(|r| r.session_id == sid && r.status == Status::Running)
                || matches!(
                    session.lifecycle,
                    Some(
                        SessionLifecycle::Closed
                            | SessionLifecycle::NeedsReconciliation
                            | SessionLifecycle::RequiresSuccessor
                    )
                )
            {
                return Err(err(
                    "permission_switch_pending",
                    "会话有活动或待核对工作，结束后再切换",
                ));
            }
            if s.governance.iter().any(|r| {
                r.kind == "permission_switch"
                    && r.definition_id == sid
                    && r.data["status"] != "adopted"
            }) {
                return Err(err("permission_switch_pending", "此会话有待恢复的权限切换"));
            }
            let mut r = crate::governance::record(
                "permission_switch",
                sid.into(),
                Scope::Project {
                    project_id: session.project_id.clone().unwrap(),
                },
                "权限会话切换".into(),
                1,
                None,
                json!({"status":"prepared","command":c,"fingerprint":fp,"binding":new_binding}),
                false,
            );
            r.id = intent_id.clone();
            s.governance.push(r);
            Ok(Value::Null)
        })?;
        let state = self.store.snapshot()?;
        let session = state
            .sessions
            .iter()
            .find(|r| r.id == sid)
            .ok_or_else(|| err("not_found", "原 Session 不存在"))?
            .clone();
        // Stop calls are idempotent, and receipts precede the next writer. A crash here
        // leaves a durable intent; retry reconciles the same owned operation.
        for execution in state
            .executions
            .iter()
            .filter(|e| e.session_id == sid && !e.writer_revoked)
        {
            let proof = match self.harness.stop_session(&session, execution) {
                Ok(proof)
                    if proof.writer_revoked && proof.operation_id == execution.operation_id =>
                {
                    proof
                }
                other => {
                    let error = other
                        .err()
                        .unwrap_or_else(|| err("needs_reconciliation", "原 worker 停止回执不匹配"));
                    self.store.transact(None, &mut |s| {
                        let r = s
                            .governance
                            .iter_mut()
                            .find(|r| r.id == intent_id)
                            .ok_or_else(|| err("needs_reconciliation", "切换意图丢失"))?;
                        let mut data = r.data.clone();
                        data["status"] = json!("needs_reconciliation");
                        data["reason"] = json!(error.message);
                        crate::governance::update_record(r, data);
                        Ok(Value::Null)
                    })?;
                    return Err(error);
                }
            };
            let protocol = if execution.protocol_artifact.is_none() {
                if let Some(log) = self.harness.session_log(execution)? {
                    let owner = match session.scope {
                        Some(SessionScope::Global) => MaterialOwner::Global,
                        Some(SessionScope::Project) => MaterialOwner::Project {
                            id: session.project_id.clone().unwrap(),
                        },
                        Some(SessionScope::Feature) => MaterialOwner::Feature {
                            id: session.feature_id.clone().unwrap(),
                        },
                        Some(SessionScope::Task) => MaterialOwner::Task {
                            feature: session.feature_id.clone().unwrap(),
                            id: session.task_id.clone().unwrap(),
                        },
                        None => return Err(err("invalid_scope", "讨论缺少范围")),
                    };
                    let mut artifact = self.artifacts.seal_scoped(&owner, "protocol", &log)?;
                    artifact.origin = Origin::External;
                    Some(artifact)
                } else {
                    None
                }
            } else {
                None
            };
            self.store.transact(None, &mut |s| {
                let e = s
                    .executions
                    .iter_mut()
                    .find(|e| e.id == execution.id)
                    .ok_or_else(|| err("needs_reconciliation", "原执行不存在"))?;
                e.writer_revoked = true;
                e.finished = Some(now());
                e.status = Status::Interrupted;
                if let Some(artifact) = &protocol {
                    e.protocol_artifact = Some(artifact.id.clone());
                    s.artifacts.push(artifact.clone());
                }
                let r = s
                    .governance
                    .iter_mut()
                    .find(|r| r.id == intent_id)
                    .ok_or_else(|| err("needs_reconciliation", "切换意图丢失"))?;
                let mut data = r.data.clone();
                data["stop_receipts"][&execution.operation_id] = json!(proof);
                crate::governance::update_record(r, data);
                Ok(Value::Null)
            })?;
        }
        self.store.transact(Some((&c.command_id,&fp)),&mut |s| {
            let intent=s.configuration(&intent_id)?.clone();
            let binding:RuntimeBinding=serde_json::from_value(intent.data["binding"].clone()).map_err(|e|err("invalid_configuration",e.to_string()))?;
            let original=s.sessions.iter().find(|v|v.id==sid).cloned().ok_or_else(||err("not_found","原会话不存在"))?;
            if c.expected_revision!=Some(original.revision) {return Err(err("revision_conflict","切换时原会话已改变"));}
            if s.turns.iter().any(|t|t.session_id==sid && t.status.active()) || s.executions.iter().any(|e|e.session_id==sid && !e.writer_revoked) {
                return Err(err("needs_reconciliation","原会话尚未静止"));
            }
            let snapshot=self.capture_conversation(s,&original,"permission_handoff",None,None,None)?;
            let instance=s.governance.iter().find(|r|r.kind=="agent_session" && r.definition_id==sid).and_then(|r|r.data["agent_instance_id"].as_str()).map(str::to_owned);
            let mut payload=json!({"agent":original.agent,"binding":binding,"force_new":true,"predecessor":sid,"snapshot_id":snapshot});
            if let Some(instance)=instance {payload["agent_instance_id"]=json!(instance);}
            let open=Command {command_id:format!("{}:successor",c.command_id),target:Some(original.owner_id.clone()),expected_revision:revision(s,&original.owner_id),payload};
            let opened=self.apply_command("session.open",&open,s)?;
            self.governance_after_command("session.open",&open,s,&opened)?;
            let next=opened["id"].as_str().ok_or_else(||err("invalid_configuration","缺少后继 Session"))?.to_owned();
            self.inherit_task_binding(s,sid,&next)?;
            let old=s.sessions.iter_mut().find(|v|v.id==sid).unwrap();
            old.lifecycle=Some(SessionLifecycle::RequiresSuccessor);old.revision+=1;
            let send=Command {command_id:c.command_id.clone(),target:Some(next.clone()),expected_revision:Some(1),payload:c.payload.clone()};
            let mut result=self.apply_command("conversation.send",&send,s)?;
            self.governance_after_command("conversation.send",&send,s,&result)?;
            result["effective_session_id"]=json!(next);
            result["predecessor"]=json!(sid);
            let r=s.governance.iter_mut().find(|r|r.id==intent_id).unwrap();
            let mut data=r.data.clone();data["status"]=json!("adopted");data["effective_session_id"]=result["effective_session_id"].clone();
            crate::governance::update_record(r,data);
            Ok(result)
        })
    }
}
