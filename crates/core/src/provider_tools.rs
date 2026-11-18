//! Native worker tool requests cross a durable observation/admission boundary.
//! Context/platform calls retain a guarded bridge until their writer family moves.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    *,
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderToolBasis {
    pub session_id: Id,
    pub session_revision: u64,
    pub turn_id: Id,
    pub turn_revision: u64,
    pub worker_operation_id: Id,
    pub binding_digest: String,
}
pub(crate) struct ProviderPoll {
    namespace: StoreNamespace,
    root: String,
    pub session: Session,
    pub turn: ProviderTurn,
    pub worker: Id,
    pub authority: ConversationAuthority,
    pub journal: Option<ResourceVersion>,
    pub pending: Option<ResourceVersion>,
    pub batch: u64,
}
/// The original runtime facts used by this observer, rather than a fabricated
/// conversation with an empty input or omitted source dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderTurn {
    pub id: Id,
    pub session_id: Id,
    pub project_id: Id,
    pub revision: u64,
    pub status: TurnStatus,
    pub external_message: Option<String>,
    pub external_turn: Option<String>,
    pub consumed: bool,
    pub cancel_requested: bool,
    pub input_digest: String,
}
pub struct ConversationRuntimeFacts {
    pub session: Session,
    pub turn: ProviderTurn,
    pub workers: Vec<RuntimeExecution>,
    pub authority: Option<ConversationAuthority>,
    pub worker_active: bool,
}
impl From<&ConversationTurn> for ProviderTurn {
    fn from(turn: &ConversationTurn) -> Self {
        Self {
            id: turn.id.clone(),
            session_id: turn.session_id.clone(),
            project_id: turn.project_id.clone(),
            revision: turn.revision,
            status: turn.status,
            external_message: turn.external_message.clone(),
            external_turn: turn.external_turn.clone(),
            consumed: turn.consumed,
            cancel_requested: turn.cancel_requested,
            input_digest: turn.input_digest.clone(),
        }
    }
}
impl ProviderPoll {
    pub fn require_store(&self, store: &dyn ports::EngineeringStore) -> Result<()> {
        if self.namespace != store.namespace() || self.root != store.root() {
            return Err(err("invalid_scope", "原生观察不能跨原数据集"));
        }
        Ok(())
    }
    pub fn guards(&self) -> Result<ReadSet> {
        let mut versions = vec![
            ResourceVersion {
                resource: ResourceKey::new("tool_session", &self.session.id),
                revision: self.session.revision,
            },
            ResourceVersion {
                resource: ResourceKey::new("tool_turn", &self.turn.id),
                revision: self.turn.revision,
            },
            ResourceVersion {
                resource: ResourceKey::new("tool_worker", &self.worker),
                revision: 1,
            },
        ];
        if let Some(journal) = &self.journal {
            versions.push(journal.clone());
        }
        ReadSet::new(versions)
    }
    pub fn basis(&self) -> ProviderToolBasis {
        ProviderToolBasis {
            session_id: self.session.id.clone(),
            session_revision: self.session.revision,
            turn_id: self.turn.id.clone(),
            turn_revision: self.turn.revision,
            worker_operation_id: self.worker.clone(),
            binding_digest: configuration_hash(&self.session.runtime_binding),
        }
    }
}
impl Engine {
    /// Recovery replays saved observations and their own durable checkpoints.
    /// It never polls the provider again or substitutes a newer Turn baseline.
    pub(crate) fn resume_provider_polls(&self) -> Result<()> {
        let after = self
            .provider_poll_cursor
            .lock()
            .map_err(|e| err("storage", e.to_string()))?
            .clone();
        let mut page = self.store.provider_poll_page(after.as_deref())?;
        if page.ids.is_empty() && after.is_some() {
            page = self.store.provider_poll_page(None)?;
        }
        let started = std::time::Instant::now();
        let mut last = None;
        let mut visited = 0;
        for id in &page.ids {
            if visited > 0 && started.elapsed() >= std::time::Duration::from_millis(25) {
                break;
            }
            visited += 1;
            last = Some(id.clone());
            let key = ResourceKey::new("governance", id);
            let pending = self
                .store
                .read_resources(std::slice::from_ref(&key))?
                .into_iter()
                .next()
                .ok_or_else(|| err("storage", "原生交接读取不完整"))?;
            let Some(value) = pending.value.as_ref() else {
                continue;
            };
            let row = ConfigurationRecord::from_storage(value.clone())?;
            if row.r#type != "provider_poll_pending" || row.data["status"] != "prepared" {
                continue;
            }
            let outcome = (|| -> Result<()> {
                let key = ResourceKey::new("governance", text(&row.data, "observation_id")?);
                let observation = self
                    .store
                    .read_resources(std::slice::from_ref(&key))?
                    .into_iter()
                    .next()
                    .ok_or_else(|| err("storage", "原观察读取不完整"))?;
                let poll = self.provider_poll_from_observation(
                    &observation,
                    ResourceVersion {
                        resource: pending.resource.clone(),
                        revision: pending.revision,
                    },
                )?;
                let raw = observation.value.as_ref().unwrap();
                if raw["data"]["value"]["error"].is_object() {
                    return Err(err(
                        "provider_observation_unresolved",
                        "原供应商错误已保留，不能通过重新 poll 替换原结果",
                    ));
                }
                let observations: Vec<ports::ConversationObservation> =
                    serde_json::from_value(raw["data"]["value"]["observations"].clone())
                        .map_err(|e| err("storage", e.to_string()))?;
                let mut poll = poll;
                self.apply_provider_observations(&mut poll, &observations)?;
                self.settle_provider_poll(&poll, "completed", None)
            })();
            if let Err(error) = outcome {
                if matches!(
                    error.code.as_str(),
                    "storage_busy" | "resource_exhausted" | "worker_log_unavailable"
                ) {
                    continue;
                }
                let status = if error.code == "revision_conflict" {
                    "conflicted"
                } else {
                    "needs_reconciliation"
                };
                let command = format!("provider-poll-reject:{id}:{status}");
                let fingerprint = configuration_hash(&(
                    "provider-poll-reject/v1",
                    &pending.resource,
                    pending.revision,
                    &error,
                ));
                let mut next = row.clone();
                let mut data = next.data.clone();
                data["status"] = json!(status);
                data["error"] = json!(error);
                update_record(&mut next, data);
                // This closes only the local projection handoff. It grants no
                // external action and retains the original immutable report.
                let result = self.store.commit_changes(&PreparedChangeSet {
                    command_id: command,
                    fingerprint,
                    read_set: ReadSet::new(vec![ResourceVersion {
                        resource: pending.resource.clone(),
                        revision: pending.revision,
                    }])?,
                    changes: vec![ResourceChange {
                        resource: pending.resource,
                        value: next.to_storage(),
                    }],
                    result: json!({"id":id,"status":status}),
                    time: now() as i64,
                });
                if let Err(error) = result
                    && !matches!(error.code.as_str(), "revision_conflict" | "storage_busy")
                {
                    return Err(error);
                }
            }
        }
        *self
            .provider_poll_cursor
            .lock()
            .map_err(|e| err("storage", e.to_string()))? = if visited < page.ids.len() {
            last
        } else {
            page.next_after
        };
        Ok(())
    }
    pub(crate) fn prepare_provider_poll(
        &self,
        facts: ConversationRuntimeFacts,
    ) -> Result<ProviderPoll> {
        let session = &facts.session;
        let turn = &facts.turn;
        let workers = facts
            .workers
            .iter()
            .filter(|e| {
                e.session_id == session.id
                    && e.kind == RuntimeExecutionKind::Harness
                    && !e.writer_revoked
            })
            .collect::<Vec<_>>();
        require(workers.len() == 1, "原生观察需要唯一的原 Harness worker")?;
        require(
            facts.worker_active,
            "原 worker 已有停止意图，不再接纳新的原生动作",
        )?;
        require(
            turn.session_id == session.id && turn.status.active() && !turn.cancel_requested,
            "原生观察轮次已结束或取消",
        )?;
        Ok(ProviderPoll {
            namespace: self.store.namespace(),
            root: self.store.root(),
            session: session.clone(),
            turn: turn.clone(),
            worker: workers[0].operation_id.clone(),
            authority: facts.authority.ok_or_else(|| {
                err(
                    "conversation_origin_missing",
                    "原 worker 缺少可核对的执行授权",
                )
            })?,
            journal: None,
            pending: None,
            batch: 0,
        })
    }
    pub(crate) fn journal_provider_poll(
        &self,
        poll: &mut ProviderPoll,
        observations: &Result<Vec<ports::ConversationObservation>>,
    ) -> Result<()> {
        poll.require_store(self.store.as_ref())?;
        if observations.as_ref().is_ok_and(Vec::is_empty) {
            return Ok(());
        }
        let value = match observations {
            Ok(value) => json!({"observations":value}),
            Err(error) => json!({"error":error}),
        };
        let digest = configuration_hash(&(
            "provider-poll/v1",
            self.store.namespace(),
            poll.basis(),
            &value,
        ));
        let id = format!("provider-poll:{digest}");
        let command = format!("provider-poll-observe:{digest}");
        let receipt = if let Some(receipt) = self.store.replay_commit(&command, &digest)? {
            receipt
        } else {
            let scope = if poll.session.scope == Some(SessionScope::Global) {
                Scope::global()
            } else {
                Scope::project(
                    poll.session
                        .project_id
                        .as_ref()
                        .ok_or_else(|| err("invalid_scope", "原生观察缺少项目"))?,
                )
            };
            let mut row = record(
                "provider_poll_observation",
                poll.turn.id.clone(),
                scope,
                "原生供应商观察".into(),
                1,
                None,
                json!({"basis":poll.basis(),"turn":poll.turn,"session":poll.session,"authority":poll.authority,"value":value}),
                true,
            );
            row.id = id.clone();
            let pending_id = format!("provider-poll-pending:{digest}");
            let mut pending = record(
                "provider_poll_pending",
                id.clone(),
                row.scope.clone(),
                "原生观察投影交接".into(),
                1,
                None,
                json!({"status":"prepared","observation_id":id}),
                false,
            );
            pending.id = pending_id.clone();
            let prepared = PreparedChangeSet {
                command_id: command,
                fingerprint: digest,
                read_set: ReadSet::new(vec![
                    ResourceVersion {
                        resource: ResourceKey::new("governance", &id),
                        revision: 0,
                    },
                    ResourceVersion {
                        resource: ResourceKey::new("governance", &pending_id),
                        revision: 0,
                    },
                ])?,
                changes: vec![
                    ResourceChange {
                        resource: ResourceKey::new("governance", &id),
                        value: self.store.encode_runtime_record(&row)?,
                    },
                    ResourceChange {
                        resource: ResourceKey::new("governance", &pending_id),
                        value: pending.to_storage(),
                    },
                ],
                result: json!({"id":id}),
                time: now() as i64,
            };
            // Retry only persistence of this exact already-observed batch. The
            // provider is never polled or sent its input again by this retry.
            let mut attempts = 0;
            loop {
                match self.store.commit_changes(&prepared) {
                    Err(error) if error.code == "storage_busy" && attempts < 7 => {
                        attempts += 1;
                        std::thread::yield_now();
                    }
                    result => break result?,
                }
            }
        };
        poll.pending = receipt
            .versions
            .iter()
            .find(|v| v.resource.id.starts_with("provider-poll-pending:"))
            .cloned();
        poll.journal = Some(
            receipt
                .versions
                .into_iter()
                .find(|v| v.resource == ResourceKey::new("governance", &id))
                .ok_or_else(|| err("storage", "原生观察缺少提交版本"))?,
        );
        Ok(())
    }
    pub(crate) fn provider_poll_from_observation(
        &self,
        observation: &ResourceSnapshot,
        pending: ResourceVersion,
    ) -> Result<ProviderPoll> {
        let raw = observation
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "原生观察缺失"))?;
        let restored = self.store.restore_runtime_record(raw)?;
        let row: ConfigurationRecord =
            serde_json::from_value(restored).map_err(|e| err("storage", e.to_string()))?;
        require(
            row.r#type == "provider_poll_observation" && row.immutable,
            "不是不可变原生观察",
        )?;
        let session: Session = serde_json::from_value(row.data["session"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let turn: ProviderTurn = serde_json::from_value(row.data["turn"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let basis: ProviderToolBasis = serde_json::from_value(row.data["basis"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let scope = if session.scope == Some(SessionScope::Global) {
            Scope::global()
        } else {
            Scope::project(&turn.project_id)
        };
        require(
            row.scope == scope
                && session.project_id.as_ref() == Some(&turn.project_id)
                && session.id == turn.session_id
                && session.id == basis.session_id
                && turn.id == basis.turn_id
                && turn.revision == basis.turn_revision
                && session.revision == basis.session_revision
                && configuration_hash(&session.runtime_binding) == basis.binding_digest
                && self
                    .store
                    .project_id()
                    .is_none_or(|id| id == turn.project_id),
            "原生观察的冻结依据或范围不符",
        )?;
        Ok(ProviderPoll {
            namespace: self.store.namespace(),
            root: self.store.root(),
            session,
            turn,
            worker: basis.worker_operation_id,
            authority: serde_json::from_value(row.data["authority"].clone())
                .map_err(|e| err("storage", e.to_string()))?,
            journal: Some(ResourceVersion {
                resource: observation.resource.clone(),
                revision: observation.revision,
            }),
            pending: Some(pending),
            batch: 0,
        })
    }
    pub(crate) fn settle_provider_poll(
        &self,
        poll: &ProviderPoll,
        status: &str,
        error: Option<&Error>,
    ) -> Result<()> {
        poll.require_store(self.store.as_ref())?;
        let (Some(journal), Some(pending)) = (&poll.journal, &poll.pending) else {
            return Ok(());
        };
        let command = format!("provider-poll-settle:{}:{status}", journal.resource.id);
        let fingerprint =
            configuration_hash(&("provider-poll-settle/v1", &journal.resource, status, error));
        if self.store.replay_commit(&command, &fingerprint)?.is_some() {
            return Ok(());
        }
        let rows = self
            .store
            .read_resources(std::slice::from_ref(&pending.resource))?;
        require(
            rows.len() == 1 && rows[0].resource == pending.resource,
            "原生观察交接缺失",
        )?;
        let mut row = ConfigurationRecord::from_storage(
            rows[0]
                .value
                .clone()
                .ok_or_else(|| err("not_found", "原交接缺失"))?,
        )?;
        require(
            row.r#type == "provider_poll_pending" && row.definition_id == journal.resource.id,
            "原交接身份不符",
        )?;
        let mut data = row.data.clone();
        data["status"] = json!(status);
        if let Some(error) = error {
            data["error"] = json!(error);
        }
        update_record(&mut row, data);
        self.store.commit_changes(&PreparedChangeSet {
            command_id: command,
            fingerprint,
            read_set: ReadSet::new(vec![journal.clone(), pending.clone()])?,
            changes: vec![ResourceChange {
                resource: pending.resource.clone(),
                value: row.to_storage(),
            }],
            result: json!({"id":pending.resource.id,"status":status}),
            time: now() as i64,
        })?;
        Ok(())
    }
    pub(crate) fn provider_tool_intent_record(
        &self,
        poll: &ProviderPoll,
        invocation: &ToolInvocation,
        observation: &ports::ConversationObservation,
        turn_revision: u64,
    ) -> Result<ConfigurationRecord> {
        poll.require_store(self.store.as_ref())?;
        let session = &poll.session;
        let journal = poll
            .journal
            .as_ref()
            .ok_or_else(|| err("storage", "工具创建前必须保留供应商观察"))?;
        let scope = if session.scope == Some(SessionScope::Global) {
            Scope::global()
        } else {
            Scope::project(
                session
                    .project_id
                    .as_ref()
                    .ok_or_else(|| err("invalid_scope", "工具缺少项目"))?,
            )
        };
        let mut row = record(
            "provider_tool_intent",
            invocation.id.clone(),
            scope,
            "原生工具创建意图".into(),
            1,
            None,
            json!({"status":"prepared","invocation":invocation,"value":observation,"basis":poll.basis(),"conversation_authority":poll.authority,"poll_id":journal.resource.id,"origin":{"kind":"native_worker","session_id":session.id,"turn_id":poll.turn.id,"worker_operation_id":poll.worker,"agent_instance_id":invocation.agent_instance_id}}),
            false,
        );
        row.id = format!("provider-tool-intent:{}", invocation.id);
        row.data["basis"]["turn_revision"] = json!(turn_revision);
        row.content_hash = content_digest(&row.refs, &row.data);
        Ok(row)
    }
    pub fn admit_provider_tool(&self, id: &str) -> Result<Value> {
        require(
            matches!(self.store.namespace(), StoreNamespace::Engineering { .. }),
            "原生工具需要会话库",
        )?;
        let key = ResourceKey::new("governance", id);
        let rows = self.store.read_resources(std::slice::from_ref(&key))?;
        require(
            rows.len() == 1 && rows[0].resource == key,
            "原生工具意图读取不完整",
        )?;
        let raw = ConfigurationRecord::from_storage(
            rows[0]
                .value
                .clone()
                .ok_or_else(|| err("not_found", "原生工具意图缺失"))?,
        )?;
        require(
            raw.r#type == "provider_tool_intent"
                && self
                    .store
                    .project_id()
                    .is_none_or(|p| raw.scope.project_id() == Some(p.as_str())),
            "原生工具意图范围不符",
        )?;
        let fingerprint = configuration_hash(&(
            "provider-tool-admit/v1",
            &raw.id,
            &raw.data["invocation"],
            &raw.data["basis"],
            &raw.data["poll_id"],
            &raw.data["origin"],
            &raw.data["conversation_authority"],
        ));
        let command = format!("provider-tool-admit:{id}");
        if let Some(receipt) = self.store.replay_commit(&command, &fingerprint)? {
            return Ok(receipt.result);
        }
        require(raw.data["status"] == "prepared", "原生工具意图已结束")?;
        let restored = self.store.restore_runtime_record(&raw.to_storage())?;
        let intent: ConfigurationRecord =
            serde_json::from_value(restored).map_err(|e| err("storage", e.to_string()))?;
        let invocation: ToolInvocation = serde_json::from_value(intent.data["invocation"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        let basis: ProviderToolBasis = serde_json::from_value(intent.data["basis"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        require(
            invocation.id == intent.definition_id
                && invocation.session_id.as_ref() == Some(&basis.session_id)
                && invocation.turn_id.as_ref() == Some(&basis.turn_id)
                && invocation.operation_id.as_ref() == Some(&basis.worker_operation_id)
                && invocation.request_id.is_some()
                && intent.data["origin"]["kind"] == "native_worker",
            "原生工具创建缺少原请求来源",
        )?;
        let derived = configuration_hash(&json!([
            basis.session_id,
            basis.turn_id,
            basis.worker_operation_id,
            invocation.call_id
        ]));
        require(
            invocation.id == format!("tool-call-{}", &derived[..32]),
            "原生工具调用身份与原 worker 不符",
        )?;
        let catalog = self.store.configuration_snapshot()?;
        crate::tool_creation::frozen_sources(&catalog, &invocation)?;
        let call_key = ResourceKey::new("governance", &invocation.id);
        let origin_key = ResourceKey::new("governance", format!("tool-origin:{}", invocation.id));
        let keys = vec![
            call_key.clone(),
            origin_key.clone(),
            ResourceKey::new("session", &basis.session_id),
            ResourceKey::new("tool_session", &basis.session_id),
            ResourceKey::new("tool_turn", &basis.turn_id),
            ResourceKey::new("tool_worker", &basis.worker_operation_id),
            ResourceKey::new("catalog_clock", "configuration"),
            ResourceKey::new("governance", text(&intent.data, "poll_id")?),
            ResourceKey::new("source", &invocation.tool.revision_id),
            ResourceKey::new("source", &invocation.tool.definition.integration_ref),
            ResourceKey::new("source", &invocation.tool.binding_ref),
        ];
        let facts = self.store.read_resources(&keys)?;
        require(
            facts.len() == keys.len() && facts.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "原生工具接纳事实不完整",
        )?;
        if facts[0].value.is_some() || facts[1].value.is_some() {
            if let Some(receipt) = self.store.replay_commit(&command, &fingerprint)? {
                return Ok(receipt.result);
            }
            return Err(err("revision_conflict", "原生调用或来源已存在"));
        }
        let observed = &intent.data["value"];
        let poll = facts[7]
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "原供应商观察缺失"))?;
        require(
            poll["type"] == "provider_poll_observation"
                && poll["scope"] == json!(intent.scope)
                && poll["data"]["basis"]["session_id"] == basis.session_id
                && poll["data"]["basis"]["turn_id"] == basis.turn_id
                && poll["data"]["basis"]["worker_operation_id"] == basis.worker_operation_id
                && poll["data"]["authority"] == intent.data["conversation_authority"]
                && poll["data"]["value"]["observations"]
                    .as_array()
                    .is_some_and(|rows| rows.contains(observed))
                && observed["turn"] == basis.turn_id
                && observed["call"] == invocation.call_id
                && observed["request"] == json!(invocation.request_id)
                && observed["tool"] == invocation.tool.definition.qualified_name
                && observed["arguments"] == invocation.arguments
                && observed["worktree"] == invocation.worktree,
            "原生工具与原供应商事实不符",
        )?;
        let mut execution_guards = Vec::new();
        let authorize = (|| -> Result<()> {
            if facts[3].revision != basis.session_revision
                || facts[4].revision != basis.turn_revision
                || facts[5].revision != 1
                || facts[6].revision != catalog.revision
            {
                return Err(err(
                    "revision_conflict",
                    "原生工具原会话、轮次、worker 或配置判断已变化",
                ));
            }
            let session: Session = serde_json::from_value(
                self.store.restore_runtime_value(
                    facts[2]
                        .value
                        .as_ref()
                        .ok_or_else(|| err("not_found", "原会话缺失"))?,
                )?,
            )
            .map_err(|e| err("storage", e.to_string()))?;
            require(
                session.project_id == invocation.project_id
                    && configuration_hash(&session.runtime_binding) == basis.binding_digest
                    && session.scope.is_some(),
                "原生工具改变了原会话冻结运行锁",
            )?;
            let binding = session
                .runtime_binding
                .as_ref()
                .ok_or_else(|| err("storage", "原会话缺少运行锁"))?;
            let config = binding
                .agent_configuration
                .as_ref()
                .ok_or_else(|| err("storage", "原会话缺少冻结 Agent"))?;
            require(
                binding
                    .permissions
                    .as_ref()
                    .is_some_and(|p| json!(p) == json!(invocation.permissions))
                    && config.agent_instance_id
                        == invocation.agent_instance_id.as_deref().unwrap_or("")
                    && config.integration_lock.as_ref().is_some_and(|l| {
                        l.tools.iter().any(|t| json!(t) == json!(invocation.tool))
                    }),
                "原生调用不属于原冻结 Agent 授权",
            )?;
            let turn = facts[4]
                .value
                .as_ref()
                .ok_or_else(|| err("not_found", "原轮次缺失"))?;
            let worker = facts[5]
                .value
                .as_ref()
                .ok_or_else(|| err("not_found", "原 worker 缺失"))?;
            if turn["cancel_requested"] == true
                || !matches!(
                    turn["status"].as_str(),
                    Some("dispatching" | "running" | "waiting_interaction")
                )
                || turn["session_id"] != session.id
                || worker["session_id"] != session.id
                || worker["active"] != true
                || worker["kind"] != "harness"
                || worker["image_digest"] != invocation.image_id
            {
                return Err(err("revision_conflict", "原生工具原执行资格已撤销"));
            }
            let observed = &intent.data["value"];
            let external = if binding.installation.key.provider == HarnessProvider::Dsh {
                &turn["external_message"]
            } else {
                &turn["external_turn"]
            };
            require(
                observed["thread"] == json!(session.external_session)
                    && &observed["external_turn"] == external,
                "工具观察改变了原供应商线程或输入身份",
            )?;
            let authority: ConversationAuthority =
                serde_json::from_value(intent.data["conversation_authority"].clone())
                    .map_err(|e| err("storage", e.to_string()))?;
            execution_guards = authority
                .live(self.store.as_ref(), &session)?
                .ok_or_else(|| {
                    err(
                        "workflow_tool_suspended",
                        "原步骤或祖先暂停，保留原工具意图",
                    )
                })?;
            Ok(())
        })();
        if authorize
            .as_ref()
            .is_err_and(|e| e.code == "workflow_tool_suspended")
        {
            return authorize.map(|_| Value::Null);
        }
        let validation = authorize.and_then(|_| self.tool_call_record(&catalog.state, &invocation));
        let (mut call, rejection) = match validation {
            Ok(call) => (call, None),
            Err(error) => {
                let mut call = record(
                    "tool_call",
                    invocation.id.clone(),
                    intent.scope.clone(),
                    invocation.tool.definition.qualified_name.clone(),
                    1,
                    None,
                    json!({"status":"failed","remote_effect":"not_applied","invocation":invocation,"created_at":now(),"finished_at":now(),"cancel_requested":false,"reply_sent":false,"local_rejection":error,"error":error}),
                    false,
                );
                call.id = invocation.id.clone();
                (call, Some(error))
            }
        };
        call.scope = intent.scope.clone();
        call.data["origin_context"] = json!(origin_key.id);
        call.data["conversation_authority"] = intent.data["conversation_authority"].clone();
        call.data["origin_proof"] =
            json!({"provider_intent_id":intent.id,"poll_observation_id":intent.data["poll_id"]});
        call.content_hash = content_digest(&call.refs, &call.data);
        let encoded = self.store.encode_runtime_record(&call)?;
        require(
            self.store.restore_runtime_record(&encoded)? == json!(call),
            "原生工具源重建不一致",
        )?;
        let mut origin = record(
            "tool_call_origin",
            call.id.clone(),
            intent.scope.clone(),
            "原生 worker 工具来源".into(),
            1,
            None,
            json!({"context":intent.data["origin"],"provider_intent_id":intent.id,"poll_observation_id":intent.data["poll_id"]}),
            true,
        );
        origin.id = origin_key.id.clone();
        let mut next = raw.clone();
        let mut data = next.data.clone();
        data["status"] = json!(if rejection.is_some() {
            "rejected"
        } else {
            "completed"
        });
        data["result"] = json!({"id":call.id});
        if let Some(error) = &rejection {
            data["error"] = json!(error)
        }
        update_record(&mut next, data);
        let mut guards = vec![
            ResourceVersion {
                resource: key.clone(),
                revision: rows[0].revision,
            },
            ResourceVersion {
                resource: call_key.clone(),
                revision: 0,
            },
            ResourceVersion {
                resource: origin_key.clone(),
                revision: 0,
            },
            ResourceVersion {
                resource: facts[7].resource.clone(),
                revision: facts[7].revision,
            },
        ];
        require(
            facts[7]
                .value
                .as_ref()
                .is_some_and(|r| r["type"] == "provider_poll_observation"),
            "原供应商观察缺失",
        )?;
        require(
            facts[8..].iter().all(|r| r.value.is_some()),
            "原生工具的精确源缺失",
        )?;
        guards.extend(facts[8..].iter().map(|r| ResourceVersion {
            resource: r.resource.clone(),
            revision: r.revision,
        }));
        if rejection.is_none() {
            guards.extend(execution_guards);
            guards.extend([
                ResourceVersion {
                    resource: facts[3].resource.clone(),
                    revision: basis.session_revision,
                },
                ResourceVersion {
                    resource: facts[4].resource.clone(),
                    revision: basis.turn_revision,
                },
                ResourceVersion {
                    resource: facts[5].resource.clone(),
                    revision: 1,
                },
                ResourceVersion {
                    resource: facts[6].resource.clone(),
                    revision: catalog.revision,
                },
            ]);
        }
        self.store
            .commit_changes(&PreparedChangeSet {
                command_id: command,
                fingerprint,
                read_set: ReadSet::new(guards)?,
                changes: vec![
                    ResourceChange {
                        resource: call_key,
                        value: encoded,
                    },
                    ResourceChange {
                        resource: origin_key,
                        value: origin.to_storage(),
                    },
                    ResourceChange {
                        resource: key,
                        value: next.to_storage(),
                    },
                ],
                result: json!({"id":call.id}),
                time: now() as i64,
            })
            .map(|r| r.result)
    }
    pub fn drive_provider_tool_intents(&self) -> Result<()> {
        let after = self
            .provider_tool_cursor
            .lock()
            .map_err(|e| err("storage", e.to_string()))?
            .clone();
        let mut page = self.store.provider_tool_intent_page(after.as_deref())?;
        if page.ids.is_empty() && after.is_some() {
            page = self.store.provider_tool_intent_page(None)?;
        }
        let started = std::time::Instant::now();
        let mut last = None;
        let mut visited = 0;
        let mut failure = None;
        for id in &page.ids {
            if visited > 0 && started.elapsed() >= std::time::Duration::from_millis(25) {
                break;
            }
            last = Some(id.clone());
            visited += 1;
            if let Err(error) = self.admit_provider_tool(id)
                && error.code != "workflow_tool_suspended"
            {
                failure.get_or_insert(error);
            }
        }
        *self
            .provider_tool_cursor
            .lock()
            .map_err(|e| err("storage", e.to_string()))? = if visited < page.ids.len() {
            last
        } else {
            page.next_after
        };
        failure.map_or(Ok(()), Err)
    }
}
