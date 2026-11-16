//! Resource-owned tool lifecycle. Only a fresh durable claim grants external I/O.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    *,
};

pub struct ToolCallPage {
    pub ids: Vec<Id>,
    pub next_after: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowToolAuthority {
    pub instances: Vec<Id>,
    pub executions: Vec<Id>,
    pub execution_id: Id,
    pub plan_revision_id: Id,
    pub input_digest: String,
    pub contract_digest: String,
    pub node_id: Id,
}
pub(crate) fn workflow_authority(
    state: &EngineeringState,
    row: &ConfigurationRecord,
    execution: &str,
) -> Result<WorkflowToolAuthority> {
    let step = row.data["steps"]
        .as_array()
        .and_then(|steps| steps.iter().find(|s| s["id"] == execution))
        .ok_or_else(|| err("not_found", "工具原步骤缺失"))?;
    let mut instances = vec![row.id.clone()];
    let mut executions = vec![execution.to_owned()];
    let mut parent = row.data["parent_execution_id"].as_str();
    for _ in 0..8 {
        let Some(parent_id) = parent else { break };
        let ancestor = state
            .governance
            .iter()
            .find(|r| {
                r.r#type == "workflow_instance"
                    && r.data["steps"]
                        .as_array()
                        .is_some_and(|steps| steps.iter().any(|step| step["id"] == parent_id))
            })
            .ok_or_else(|| err("storage", "工具步骤祖先缺失"))?;
        instances.push(ancestor.id.clone());
        executions.push(parent_id.to_owned());
        parent = ancestor.data["parent_execution_id"].as_str();
    }
    require(parent.is_none(), "工具步骤祖先超限")?;
    Ok(WorkflowToolAuthority {
        instances,
        executions,
        execution_id: execution.into(),
        plan_revision_id: text(step, "plan_revision_id")?,
        input_digest: text(step, "input_digest")?,
        contract_digest: text(step, "contract_digest")?,
        node_id: text(step, "node_id")?,
    })
}
/// A native row plus its exact frozen invocation. The adapter restores source
/// references only; this is not a partial EngineeringState for business rules.
pub struct ToolCallFacts {
    pub row: ResourceSnapshot,
    pub invocation: ToolInvocation,
    pub session: Option<Session>,
    pub worker_guards: Vec<ResourceVersion>,
    pub worker_active: bool,
    pub dispatch_allowed: bool,
    pub dispatch_suspended: bool,
}
pub struct ToolCall {
    namespace: StoreNamespace,
    row: ConfigurationRecord,
    version: ResourceVersion,
    invocation: ToolInvocation,
    session: Option<Session>,
    worker_guards: Vec<ResourceVersion>,
    worker_active: bool,
    dispatch_allowed: bool,
    dispatch_suspended: bool,
}
/// Non-cloneable and non-deserializable, and constructed only on applied=true.
pub struct ToolDispatch {
    call: ToolCall,
}
pub struct ToolCancellation {
    call: ToolCall,
}
impl ToolCancellation {
    pub fn invocation(&self) -> &ToolInvocation {
        &self.call.invocation
    }
    pub fn observe(&self, store: &dyn EngineeringStore, result: &Result<()>) -> Result<()> {
        let observed = self.call.journal(
            store,
            "cancel",
            json!({"value":result.as_ref().ok(),"error":result.as_ref().err()}),
        )?;
        let mut data = self.call.row.data.clone();
        data["cancel_observation_id"] = json!(observed.resource.id);
        self.call
            .commit(
                store,
                "cancel_receipt",
                &self.call.next(data),
                vec![],
                vec![observed],
            )
            .map(|_| ())
    }
}
impl ToolDispatch {
    pub fn invocation(&self) -> &ToolInvocation {
        &self.call.invocation
    }
    pub fn observe(&self, store: &dyn EngineeringStore, receipt: &ToolReceipt) -> Result<()> {
        self.call.observe(store, receipt)
    }
    pub fn unknown(&self, store: &dyn EngineeringStore, error: &Error) -> Result<()> {
        self.call.mark_unknown(store, "start", error)
    }
}
fn key(id: &str) -> ResourceKey {
    ResourceKey::new("governance", id)
}
fn write(row: &ConfigurationRecord) -> ResourceChange {
    ResourceChange {
        resource: key(&row.id),
        value: row.to_storage(),
    }
}
fn snapshot(store: &dyn EngineeringStore, id: &str) -> Result<ResourceSnapshot> {
    let expected = key(id);
    let mut rows = store.read_resources(std::slice::from_ref(&expected))?;
    require(
        rows.len() == 1 && rows[0].resource == expected,
        "工具资源读取不完整",
    )?;
    Ok(rows.remove(0))
}
fn terminal(row: &ConfigurationRecord) -> bool {
    ["succeeded", "failed", "cancelled"].contains(&row.data["status"].as_str().unwrap_or(""))
}
impl ToolCall {
    pub fn read(store: &dyn EngineeringStore, id: &str) -> Result<Self> {
        let facts = store.tool_call_facts(id)?;
        require(facts.row.resource == key(id), "工具调用资源不符")?;
        let row = ConfigurationRecord::from_storage(
            facts
                .row
                .value
                .ok_or_else(|| err("not_found", "工具调用缺失"))?,
        )?;
        let invocation = facts.invocation;
        require(
            (invocation.session_id.is_none()
                && invocation.turn_id.is_none()
                && invocation.request_id.is_none())
                || (invocation.session_id.is_some()
                    && invocation.turn_id.is_some()
                    && invocation.operation_id.is_some()
                    && invocation.request_id.is_some()),
            "原生工具调用缺少完整会话、轮次、worker 或请求身份",
        )?;
        require(
            row.r#type == "tool_call"
                && row.id == invocation.id
                && row.scope
                    == invocation
                        .project_id
                        .as_ref()
                        .map_or_else(Scope::global, Scope::project)
                && store
                    .project_id()
                    .is_none_or(|project| invocation.project_id.as_ref() == Some(&project))
                && invocation.input_hash == configuration_hash(&invocation.arguments),
            "工具调用身份、范围或参数摘要不符",
        )?;
        Ok(Self {
            namespace: store.namespace(),
            version: ResourceVersion {
                resource: facts.row.resource,
                revision: facts.row.revision,
            },
            row,
            invocation,
            session: facts.session,
            worker_guards: facts.worker_guards,
            worker_active: facts.worker_active,
            dispatch_allowed: facts.dispatch_allowed,
            dispatch_suspended: facts.dispatch_suspended,
        })
    }
    pub fn id(&self) -> &str {
        &self.row.id
    }
    pub fn invocation(&self) -> &ToolInvocation {
        &self.invocation
    }
    pub fn data(&self) -> &Value {
        &self.row.data
    }
    pub fn ended(&self) -> bool {
        terminal(&self.row)
    }
    fn bound(&self, store: &dyn EngineeringStore) -> Result<()> {
        require(
            store.namespace() == self.namespace,
            "工具调用不能跨命名空间接纳",
        )
    }
    fn next(&self, data: Value) -> ConfigurationRecord {
        let mut row = self.row.clone();
        update_record(&mut row, data);
        row
    }
    fn commit(
        &self,
        store: &dyn EngineeringStore,
        action: &str,
        next: &ConfigurationRecord,
        extra: Vec<ResourceChange>,
        mut guards: Vec<ResourceVersion>,
    ) -> Result<CommitOutcome> {
        self.bound(store)?;
        let semantic = |row: &Value| {
            let mut value = row.clone();
            if let Some(object) = value.as_object_mut() {
                object.remove("created_at");
                object.remove("content_hash");
                if let Some(data) = object.get_mut("data").and_then(Value::as_object_mut) {
                    data.remove("dispatched_at");
                    data.remove("finished_at");
                }
            }
            value
        };
        // Observation identity and the original resource version define replay.
        // Local capture times cannot turn an identical callback into a new action.
        let fingerprint = configuration_hash(&(
            "tool-lifecycle/v1",
            &self.namespace,
            &self.version,
            action,
            semantic(&next.to_storage()),
            &extra
                .iter()
                .map(|c| (&c.resource, semantic(&c.value)))
                .collect::<Vec<_>>(),
        ));
        guards.push(self.version.clone());
        let mut changes = if next == &self.row {
            vec![]
        } else {
            vec![write(next)]
        };
        changes.extend(extra);
        if changes.is_empty() {
            return Ok(CommitOutcome {
                applied: false,
                receipt: CommitReceipt {
                    result: json!({"id":self.id()}),
                    versions: vec![self.version.clone()],
                    cursor: store.event_cursor()?,
                },
            });
        }
        store.submit_changes(&PreparedChangeSet {
            command_id: format!("tool-lifecycle:{fingerprint}"),
            fingerprint,
            read_set: ReadSet::new(guards)?,
            changes,
            result: json!({"id":self.id()}),
            time: now() as i64,
        })
    }
    pub fn claim(self, store: &dyn EngineeringStore) -> Result<Option<ToolDispatch>> {
        self.bound(store)?;
        require(
            self.row.data["status"] == "prepared" && self.row.data["cancel_requested"] != true,
            "工具调用不允许派发",
        )?;
        if self.dispatch_suspended {
            return Ok(None);
        }
        let dispatch_id = format!(
            "tool-dispatch:{}",
            configuration_hash(&(&self.namespace, self.id()))
        );
        let grant = snapshot(store, &dispatch_id)?;
        if let Some(value) = grant.value {
            let known = ConfigurationRecord::from_storage(value)?;
            require(
                known.r#type == "tool_dispatch"
                    && known.scope == self.row.scope
                    && known.definition_id == self.id()
                    && known.data["namespace"] == json!(self.namespace)
                    && known.data["invocation_hash"]
                        == configuration_hash(&self.row.data["invocation"]),
                "工具派发意图身份冲突",
            )?;
            return Ok(None);
        }
        let mut intent = record(
            "tool_dispatch",
            self.id().into(),
            self.row.scope.clone(),
            "工具派发意图".into(),
            1,
            None,
            json!({"namespace":self.namespace,"call_id":self.id(),"invocation_hash":configuration_hash(&self.row.data["invocation"]),"call_revision":self.row.revision,"inputs":{"session_id":self.invocation.session_id,"turn_id":self.invocation.turn_id,"operation_id":self.invocation.operation_id,"agent_instance_id":self.invocation.agent_instance_id}}),
            true,
        );
        intent.id = dispatch_id.clone();
        let mut data = self.row.data.clone();
        let mut guards = self.worker_guards.clone();
        guards.push(ResourceVersion {
            resource: grant.resource,
            revision: grant.revision,
        });
        if !self.dispatch_allowed {
            data["status"] = json!("cancelled");
            data["remote_effect"] = json!("not_applied");
            data["reply_sent"] = json!(true);
            data["reply_status"] = json!("worker_unavailable");
            data["finished_at"] = json!(now());
            data["local_pre_dispatch_stop"] =
                json!({"operation_id":self.invocation.operation_id,"authority":self.worker_guards});
            return match self.commit(
                store,
                "inactive_before_dispatch",
                &self.next(data),
                vec![],
                guards,
            ) {
                Ok(_) => Ok(None),
                Err(error) if error.code == "revision_conflict" => Ok(None),
                Err(error) => Err(error),
            };
        }
        data["status"] = json!("dispatched");
        data["dispatch_id"] = json!(dispatch_id);
        data["dispatched_at"] = json!(now());
        let next = self.next(data);
        let outcome = self.commit(store, "dispatch", &next, vec![write(&intent)], guards);
        match outcome {
            Ok(outcome) if outcome.applied => {
                let mut admitted = self;
                admitted.row = next;
                admitted.version.revision += 1;
                Ok(Some(ToolDispatch { call: admitted }))
            }
            Ok(_) => Ok(None),
            Err(error) if error.code == "revision_conflict" => Ok(None),
            Err(error) => Err(error),
        }
    }
    fn dispatch_guard(&self, store: &dyn EngineeringStore) -> Result<ResourceVersion> {
        let dispatch = text(&self.row.data, "dispatch_id")?;
        let saved = snapshot(store, &dispatch)?;
        let row = ConfigurationRecord::from_storage(
            saved
                .value
                .ok_or_else(|| err("needs_reconciliation", "原工具派发意图缺失"))?,
        )?;
        require(
            row.r#type == "tool_dispatch"
                && row.definition_id == self.id()
                && row.scope == self.row.scope
                && row.data["namespace"] == json!(self.namespace)
                && row.data["invocation_hash"] == configuration_hash(&self.row.data["invocation"]),
            "原工具派发意图不符",
        )?;
        Ok(ResourceVersion {
            resource: saved.resource,
            revision: saved.revision,
        })
    }
    fn journal(
        &self,
        store: &dyn EngineeringStore,
        kind: &str,
        value: Value,
    ) -> Result<ResourceVersion> {
        self.bound(store)?;
        let fingerprint = configuration_hash(&(
            "tool-observation/v1",
            &self.namespace,
            self.id(),
            &self.row.data["dispatch_id"],
            kind,
            &value,
        ));
        let id = format!("tool-observation:{fingerprint}");
        let mut row = record(
            "tool_observation",
            self.id().into(),
            self.row.scope.clone(),
            kind.into(),
            1,
            None,
            json!({"namespace":self.namespace,"dispatch_id":self.row.data["dispatch_id"],"invocation_hash":configuration_hash(&self.row.data["invocation"]),"value":value}),
            true,
        );
        row.id = id;
        let condition = ResourceVersion {
            resource: key(&row.id),
            revision: 0,
        };
        let receipt = store.commit_changes(&PreparedChangeSet {
            command_id: format!("tool-observation:{fingerprint}"),
            fingerprint,
            read_set: ReadSet::new(vec![condition])?,
            changes: vec![write(&row)],
            result: json!({"id":row.id}),
            time: now() as i64,
        })?;
        receipt
            .versions
            .into_iter()
            .find(|v| v.resource == key(&row.id))
            .ok_or_else(|| err("storage", "工具观察回执缺少版本"))
    }
    pub fn observe(&self, store: &dyn EngineeringStore, receipt: &ToolReceipt) -> Result<()> {
        require(
            receipt.invocation_id == self.id()
                && receipt.origin == Origin::External
                && ["unknown", "applied", "not_applied"].contains(&receipt.remote_effect.as_str()),
            "工具观察身份、来源或效果无效",
        )?;
        let guard = self.dispatch_guard(store)?;
        let observed = self.journal(store, "receipt", json!(receipt))?;
        if self.row.data["receipt"] == json!(receipt) {
            return Ok(());
        }
        if terminal(&self.row) {
            return Err(err(
                "revision_conflict",
                "工具原始观察已保留，当前终态或人工核对不能被覆盖",
            ));
        }
        require(
            ["dispatched", "needs_reconciliation"]
                .contains(&self.row.data["status"].as_str().unwrap_or("")),
            "工具观察不属于已派发调用",
        )?;
        let mut data = self.row.data.clone();
        data["status"] = json!(if receipt.success {
            "succeeded"
        } else if receipt.remote_effect == "unknown" {
            "needs_reconciliation"
        } else {
            "failed"
        });
        data["remote_effect"] = json!(receipt.remote_effect);
        data["receipt"] = json!(receipt);
        data["finished_at"] = json!(now());
        data["observation_id"] = json!(observed.resource.id);
        self.commit(
            store,
            "receipt",
            &self.next(data),
            vec![],
            vec![guard, observed],
        )
        .map(|_| ())
    }
    pub fn mark_unknown(
        &self,
        store: &dyn EngineeringStore,
        phase: &str,
        error: &Error,
    ) -> Result<()> {
        let dispatch = self.dispatch_guard(store)?;
        let observed = self.journal(store, phase, json!({"error":error}))?;
        require(!terminal(&self.row), "工具调用已结束或人工核对")?;
        let mut data = self.row.data.clone();
        data["status"] = json!("needs_reconciliation");
        data["remote_effect"] = json!("unknown");
        data["observation_id"] = json!(observed.resource.id);
        self.commit(
            store,
            phase,
            &self.next(data),
            vec![],
            vec![dispatch, observed],
        )
        .map(|_| ())
    }
    /// Durable cancellation intent is granted once, independently from receipt
    /// polling. Cancellation never turns an unknown remote effect into absence.
    pub fn claim_cancel(&self, store: &dyn EngineeringStore) -> Result<Option<ToolCancellation>> {
        require(
            self.row.data["cancel_requested"] == true && !terminal(&self.row),
            "工具调用未请求取消",
        )?;
        let dispatch = self.dispatch_guard(store)?;
        let id = format!(
            "tool-cancel:{}",
            configuration_hash(&(&self.namespace, self.id(), &dispatch.resource.id))
        );
        let known = snapshot(store, &id)?;
        if known.value.is_some() {
            return Ok(None);
        }
        let mut intent = record(
            "tool_cancel_intent",
            self.id().into(),
            self.row.scope.clone(),
            "工具停止意图".into(),
            1,
            None,
            json!({"dispatch_id":dispatch.resource.id,"inputs":{"control_context":self.row.data["control_context"],"operation_id":self.invocation.operation_id}}),
            true,
        );
        intent.id = id;
        let mut data = self.row.data.clone();
        data["status"] = json!("needs_reconciliation");
        data["remote_effect"] = json!("unknown");
        let next = self.next(data);
        match self.commit(
            store,
            "cancel_dispatch",
            &next,
            vec![write(&intent)],
            vec![
                dispatch,
                ResourceVersion {
                    resource: known.resource,
                    revision: known.revision,
                },
            ],
        ) {
            Ok(result) if result.applied => {
                let mut call = self.clone_for_observation(next);
                if let Some(version) = result
                    .receipt
                    .versions
                    .into_iter()
                    .find(|v| v.resource == self.version.resource)
                {
                    call.version = version;
                }
                Ok(Some(ToolCancellation { call }))
            }
            Ok(_) => Ok(None),
            Err(error) if error.code == "revision_conflict" => Ok(None),
            Err(error) => Err(error),
        }
    }
    pub fn deliver(
        &self,
        store: &dyn EngineeringStore,
        harness: &dyn ports::Harness,
    ) -> Result<()> {
        let (Some(session), Some(request)) = (&self.session, &self.invocation.request_id) else {
            return Ok(());
        };
        if self.row.data["reply_sent"] == true {
            return Ok(());
        }
        let receipt: ToolReceipt = serde_json::from_value(self.row.data["receipt"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let id = format!(
            "tool-reply:{}",
            configuration_hash(&(&self.namespace, self.id(), "native-reply/v1"))
        );
        let known = snapshot(store, &id)?;
        if known.value.is_some() {
            return Ok(());
        }
        let mut intent = record(
            "tool_reply_intent",
            self.id().into(),
            self.row.scope.clone(),
            "原生工具回复意图".into(),
            1,
            None,
            json!({"inputs":{"session_id":session.id,"request_id":request,"operation_id":self.invocation.operation_id},"value":receipt,"worker_active":self.worker_active}),
            true,
        );
        intent.id = id;
        let mut data = self.row.data.clone();
        data["reply_status"] = json!(if self.worker_active {
            "dispatching"
        } else {
            "worker_unavailable"
        });
        data["reply_sent"] = json!(!self.worker_active);
        let next = self.next(data);
        let mut guards = self.worker_guards.clone();
        guards.push(ResourceVersion {
            resource: known.resource,
            revision: known.revision,
        });
        let outcome = self.commit(store, "reply_claim", &next, vec![write(&intent)], guards)?;
        if !outcome.applied || !self.worker_active {
            return Ok(());
        }
        let result = harness.answer_tool_call(session, request, &receipt);
        let mut admitted = self.clone_for_observation(next);
        admitted.version.revision += 1;
        let observation = admitted.journal(
            store,
            "reply",
            json!({"value":result.as_ref().ok(),"error":result.as_ref().err()}),
        )?;
        let mut data = admitted.row.data.clone();
        data["reply_sent"] = json!(result.is_ok());
        data["reply_status"] = json!(if result.is_ok() {
            "delivered"
        } else {
            "needs_reconciliation"
        });
        admitted
            .commit(
                store,
                "reply_receipt",
                &admitted.next(data),
                vec![],
                vec![observation],
            )
            .map(|_| ())
    }
    fn clone_for_observation(&self, row: ConfigurationRecord) -> Self {
        Self {
            namespace: self.namespace.clone(),
            row,
            version: self.version.clone(),
            invocation: self.invocation.clone(),
            session: self.session.clone(),
            worker_guards: self.worker_guards.clone(),
            worker_active: self.worker_active,
            dispatch_allowed: self.dispatch_allowed,
            dispatch_suspended: self.dispatch_suspended,
        }
    }
}

pub fn control(
    store: &dyn EngineeringStore,
    method: &str,
    command: &Command,
    context: &InvocationContext,
) -> Result<Option<Value>> {
    if ![
        "tool.call.cancel",
        "tool.call.reconcile",
        "tool.call.approve",
    ]
    .contains(&method)
    {
        return Ok(None);
    }
    context.require_scope(store.project_id().as_deref())?;
    require(
        matches!(context.actor(), WorkflowActor::Human),
        "工具控制需要实际人类入口",
    )?;
    let fingerprint = configuration_hash(&("tool-control/v1", method, command, context));
    let target = command
        .target
        .as_deref()
        .ok_or_else(|| err("invalid_params", "需要工具调用标识"))?;
    let saved = snapshot(store, target)?;
    let Some(value) = saved.value else {
        return Err(err("not_found", "工具调用缺失"));
    };
    let row = ConfigurationRecord::from_storage(value)?;
    if row.r#type != "tool_call" {
        return Ok(None);
    }
    if let Some(receipt) = store.replay_commit(&command.command_id, &fingerprint)? {
        return Ok(Some(receipt.result));
    }
    require(
        store
            .project_id()
            .is_none_or(|project| row.scope.project_id() == Some(project.as_str())),
        "工具控制作用域不符",
    )?;
    let expected = command
        .expected_revision
        .ok_or_else(|| err("precondition_required", "工具控制缺少原始版本"))?;
    if expected != row.revision {
        return Err(err("revision_conflict", "工具调用版本已变化"));
    }
    let provenance = context.prepare_record(method, command);
    let audit = snapshot(store, &provenance.resource.id)?;
    require(audit.value.is_none(), "工具控制调用来源已存在")?;
    let mut data = row.data.clone();
    if method == "tool.call.approve" {
        require(
            data["status"] == "awaiting_approval",
            "工具调用不在等待批准",
        )?;
        let approve = command.payload["approve"]
            .as_bool()
            .ok_or_else(|| err("invalid_params", "approve 需要布尔值"))?;
        data["approved_input_hash"] = data["invocation"]["input_hash"].clone();
        data["status"] = json!(if approve { "prepared" } else { "cancelled" });
        if !approve {
            data["receipt"] = json!(human_receipt(target, "declined", "not_applied"));
            data["finished_at"] = json!(now());
        }
    } else if method == "tool.call.cancel" {
        require(!terminal(&row), "工具调用已经结束")?;
        data["cancel_requested"] = json!(true);
        if ["prepared", "awaiting_approval"].contains(&data["status"].as_str().unwrap_or("")) {
            data["status"] = json!("cancelled");
            data["remote_effect"] = json!("not_applied");
            data["receipt"] = json!(human_receipt(target, "cancelled", "not_applied"));
            data["finished_at"] = json!(now());
        }
    } else {
        require(
            data["status"] == "needs_reconciliation",
            "仅允许核对未确认工具调用",
        )?;
        let effect = text(&command.payload, "remote_effect")?;
        require(
            ["unknown", "applied", "not_applied"].contains(&effect.as_str()),
            "无效远端事实",
        )?;
        let evidence = text(&command.payload, "evidence")?;
        require(!evidence.trim().is_empty(), "人工核对需要证据")?;
        data["reconciliation"] = json!({"effect":effect,"inputs":{"evidence":evidence},"time":now(),"context":provenance.resource.id});
        data["remote_effect"] = json!(effect);
        if effect != "unknown" {
            if data["receipt"].is_object() {
                data["original_receipt"] = data["receipt"].clone();
            }
            data["status"] = json!("failed");
            data["receipt"] = json!(human_receipt(target, "human_reconciled", &effect));
        }
    }
    data["control_context"] = json!(provenance.resource.id);
    let mut next = row;
    update_record(&mut next, data);
    let result = json!(next);
    store
        .commit_changes(&PreparedChangeSet {
            command_id: command.command_id.clone(),
            fingerprint,
            read_set: ReadSet::new(vec![
                ResourceVersion {
                    resource: saved.resource,
                    revision: saved.revision,
                },
                ResourceVersion {
                    resource: audit.resource,
                    revision: audit.revision,
                },
            ])?,
            changes: vec![write(&next), provenance],
            result,
            time: now() as i64,
        })
        .map(|r| Some(r.result))
}
fn human_receipt(id: &str, code: &str, effect: &str) -> ToolReceipt {
    ToolReceipt {
        invocation_id: id.into(),
        success: false,
        content: vec![json!({"type":"text","text":code})],
        structured_content: None,
        remote_effect: effect.into(),
        error: Some(json!({"code":code})),
        origin: Origin::Human,
    }
}
