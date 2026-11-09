//! Discussion is a persistent domain workflow, independent of engineering Attempts and Runs.
use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionScope {
    Project,
    Feature,
    Task,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionLifecycle {
    Prepared,
    Active,
    Idle,
    NeedsReconciliation,
    RequiresSuccessor,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeBinding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_configuration: Option<AgentExecutionConfiguration>,
    #[serde(default)]
    pub permissions: Option<ExecutionPermissions>,
    pub installation: RuntimeInstallation,
    pub adapter_version: String,
    pub model: String,
    pub tool_policy: ToolPolicy,
    #[serde(default)]
    pub writable_paths: Vec<String>,
    #[serde(default)]
    pub read_only_paths: Vec<String>,
    pub work_type: WorkType,
    pub configuration_digest: String,
    #[serde(default)]
    pub credential_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    Queued,
    Dispatching,
    Running,
    WaitingInteraction,
    Completed,
    Failed,
    Cancelled,
    NeedsReconciliation,
}
impl TurnStatus {
    pub fn active(self) -> bool {
        !matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenCode {
    pub context: GitContext,
    pub revision: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurn {
    pub id: Id,
    pub project_id: Id,
    pub session_id: Id,
    pub client_message_id: String,
    pub input: String,
    pub input_digest: String,
    #[serde(default)]
    pub input_snapshot: Option<Id>,
    #[serde(default)]
    pub code_view: Option<FrozenCode>,
    #[serde(default)]
    pub source_revision: Option<u64>,
    pub status: TurnStatus,
    pub external_message: Option<String>,
    pub external_turn: Option<String>,
    pub consumed: bool,
    #[serde(default)]
    pub cancel_requested: bool,
    pub started: Option<u64>,
    pub finished: Option<u64>,
    pub run_id: Option<Id>,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationEventKind {
    User,
    Assistant,
    Tool,
    Approval,
    Question,
    Status,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageMode {
    #[default]
    Full,
    Delta,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationEvent {
    pub id: Id,
    pub session_id: Id,
    pub turn_id: Id,
    pub sequence: u64,
    pub deduplication_key: String,
    pub kind: ConversationEventKind,
    pub content: String,
    pub artifact_id: Option<Id>,
    pub time: u64,
    #[serde(default)]
    pub mode: MessageMode,
    #[serde(default)]
    pub provider_item: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionKind {
    ToolApproval,
    AgentQuestion,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionStatus {
    Pending,
    Answered,
    Cancelled,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionQuestion {
    pub id: String,
    pub question: String,
    pub options: Vec<String>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub multi_select: bool,
    #[serde(default)]
    pub intent: Option<Value>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    AllowedOnce,
    Rejected,
    Cancelled,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionAnswer {
    pub id: String,
    pub selected: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InteractionAnswer {
    ToolApproval {
        outcome: ApprovalDecision,
    },
    UserQuestions {
        answers: Vec<QuestionAnswer>,
        cancelled: bool,
    },
    /// Readable migration history; never sent back to a live provider.
    Legacy {
        text: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingInteraction {
    pub id: Id,
    pub project_id: Id,
    pub session_id: Id,
    pub turn_id: Id,
    pub external_request: String,
    pub kind: InteractionKind,
    pub status: InteractionStatus,
    pub questions: Vec<InteractionQuestion>,
    pub tool: Option<String>,
    pub reason: String,
    pub answer: Option<InteractionAnswer>,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Draft,
    Accepted,
    Rejected,
    Stale,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposedTask {
    pub key: String,
    pub title: String,
    pub acceptance: Vec<String>,
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub test_recipe: Option<TestRecipe>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProposalContent {
    Requirement {
        title: String,
        body: String,
        acceptance: Vec<AcceptanceCriterion>,
    },
    Feature {
        title: String,
        requirement: Id,
    },
    Design {
        title: String,
        body: String,
        requirement: Id,
    },
    TaskPlan {
        requirement: Id,
        design: Id,
        tasks: Vec<ProposedTask>,
    },
    TestPlan {
        title: String,
        body: String,
        requirement: Id,
        design: Id,
        verifies: Vec<String>,
        #[serde(default)]
        test_recipe: Option<TestRecipe>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: Id,
    pub project_id: Id,
    pub session_id: Id,
    pub turn_id: Id,
    pub schema_version: u32,
    pub content: ProposalContent,
    pub target: Option<Id>,
    pub expected_revision: Option<u64>,
    pub dependencies: Vec<Id>,
    pub status: ProposalStatus,
    pub artifact_id: Option<Id>,
    pub adopted_objects: Vec<Id>,
    #[serde(default)]
    pub task_instances: std::collections::HashMap<String, Id>,
    pub supersedes: Option<Id>,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeExecutionKind {
    #[default]
    Harness,
    Tests,
}
/// Append one row per worker lifetime; a new worker cannot overwrite the previous receipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeExecution {
    #[serde(default)]
    pub permissions: Option<ExecutionPermissions>,
    #[serde(default)]
    pub kind: RuntimeExecutionKind,
    pub id: Id,
    pub session_id: Id,
    pub run_id: Option<Id>,
    pub operation_id: Id,
    pub container_id: Option<String>,
    pub image_digest: String,
    pub status: Status,
    pub writer_revoked: bool,
    pub started: Option<u64>,
    pub finished: Option<u64>,
    #[serde(default)]
    pub protocol_artifact: Option<Id>,
}

impl EngineeringState {
    pub fn validate_conversations(&self, before: &Self) -> Result<()> {
        self.validate_material_owners()?;
        self.validate_repositories(before)?;
        use std::collections::HashSet;
        for definition in &self.definitions {
            if let Some(profile) = &definition.agent_profile {
                require(definition.family == "agent", "角色配置只适用于 Agent 定义")?;
                profile.validate()?;
                require(
                    profile.skill_revisions.iter().all(|id| {
                        self.definitions
                            .iter()
                            .any(|d| &d.id == id && d.family == "skill")
                    }),
                    "角色 Skill 必须引用确定修订",
                )?;
                if let Some(old) = before.definitions.iter().find(|d| d.id == definition.id) {
                    require(
                        serde_json::to_value(old).unwrap()
                            == serde_json::to_value(definition).unwrap(),
                        "Agent 修订不可原地修改",
                    )?;
                }
            }
        }
        let mut external = HashSet::new();
        for session in &self.sessions {
            if let Some(old) = before.sessions.iter().find(|s| s.id == session.id) {
                require(
                    serde_json::to_value((
                        &old.owner_id,
                        &old.agent,
                        &old.harness,
                        &old.sandbox,
                        &old.project_id,
                        &old.feature_id,
                        &old.task_id,
                        old.scope,
                        &old.runtime_binding,
                        &old.predecessor,
                    ))
                    .unwrap()
                        == serde_json::to_value((
                            &session.owner_id,
                            &session.agent,
                            &session.harness,
                            &session.sandbox,
                            &session.project_id,
                            &session.feature_id,
                            &session.task_id,
                            session.scope,
                            &session.runtime_binding,
                            &session.predecessor,
                        ))
                        .unwrap(),
                    "Session 范围与执行绑定不可原地变更",
                )?;
                require(
                    old.external_session
                        .as_ref()
                        .is_none_or(|id| Some(id) == session.external_session.as_ref()),
                    "外部 Session ID 不可替换",
                )?;
            }
            let Some(scope) = session.scope else {
                require(
                    session.project_id.is_none()
                        && session.feature_id.is_none()
                        && session.task_id.is_none()
                        && session.runtime_binding.is_none()
                        && session.external_session.is_none(),
                    "历史 Session 不能倒填真实运行身份",
                )?;
                continue;
            };
            let project = session
                .project_id
                .as_deref()
                .ok_or_else(|| err("blocked", "讨论 Session 缺少项目"))?;
            require(session.lifecycle.is_some(), "讨论 Session 缺少生命周期状态")?;
            require(
                self.projects.iter().any(|p| p.id == project),
                "Session 项目不存在",
            )?;
            match scope {
                SessionScope::Project => require(
                    session.owner_id == project
                        && session.feature_id.is_none()
                        && session.task_id.is_none(),
                    "Project Session 归属不一致",
                )?,
                SessionScope::Feature | SessionScope::Task => {
                    let feature = self.feature(
                        session
                            .feature_id
                            .as_deref()
                            .ok_or_else(|| err("blocked", "Session 缺少 Feature"))?,
                    )?;
                    require(feature.project_id == project, "Session 跨项目引用 Feature")?;
                    if scope == SessionScope::Task {
                        let task = self.task(
                            session
                                .task_id
                                .as_deref()
                                .ok_or_else(|| err("blocked", "Session 缺少 Task"))?,
                        )?;
                        require(
                            task.feature_id == feature.id && session.owner_id == task.id,
                            "Task Session 归属不一致",
                        )?;
                    } else {
                        require(
                            session.owner_id == feature.id && session.task_id.is_none(),
                            "Feature Session 归属不一致",
                        )?;
                    }
                }
            }
            let binding = session
                .runtime_binding
                .as_ref()
                .ok_or_else(|| err("blocked", "Session 缺少冻结运行时绑定"))?;
            binding.installation.key.validate()?;
            if let Some(p) = &binding.permissions {
                require(
                    p.scope.work_type == binding.work_type
                        && p.scope.writable_paths == binding.writable_paths
                        && p.scope.read_only_paths == binding.read_only_paths,
                    "Permission 执行范围与运行锁不一致",
                )?;
                p.harness.policy.validate()?;
                p.sandbox.policy.validate()?;
                for r in [&p.harness.reference, &p.sandbox.reference] {
                    require(
                        self.permissions.iter().any(|d| d.id == r.revision_id),
                        "冻结 Permission 修订缺失",
                    )?;
                }
            }
            match binding.tool_policy {
                ToolPolicy::ReadOnly => {
                    require(binding.writable_paths.is_empty(), "只读角色不能有写路径")?
                }
                ToolPolicy::TaskWrite => require(
                    scope == SessionScope::Task && binding.writable_paths == vec!["."],
                    "开发写权限只能属于当前 Task 根目录",
                )?,
                ToolPolicy::TestWrite => {
                    require(
                        [SessionScope::Task, SessionScope::Feature].contains(&scope)
                            && !binding.writable_paths.is_empty(),
                        "测试作者只能写当前 Task 的明确测试目录",
                    )?;
                    for path in &binding.writable_paths {
                        require(
                            path.starts_with("tests/")
                                && path.split('/').count() >= 2
                                && !path.contains(['\\', '\n', '\r'])
                                && path
                                    .split('/')
                                    .all(|v| !v.is_empty() && v != "." && v != ".."),
                            "测试写目录越界",
                        )?;
                    }
                }
            }

            require(
                binding.installation.key.os == "linux"
                    && !matches!(
                        binding.installation.artifact,
                        RuntimeArtifact::Executable { .. }
                    ),
                "真实工具会话必须使用 Linux 容器运行时",
            )?;
            require(
                !binding.adapter_version.is_empty()
                    && !binding.model.is_empty()
                    && binding.configuration_digest.len() == 64,
                "运行时绑定缺少适配器、模型或配置摘要",
            )?;
            let agent = self
                .definitions
                .iter()
                .find(|d| d.id == session.agent)
                .and_then(|d| d.agent_profile.as_ref())
                .ok_or_else(|| err("blocked", "讨论 Session 必须绑定明确角色修订"))?;
            let effective_policy = if [WorkType::Discussion, WorkType::Design, WorkType::Review]
                .contains(&binding.work_type)
            {
                ToolPolicy::ReadOnly
            } else {
                agent.tool_policy
            };
            require(
                agent.allowed_work.contains(&binding.work_type)
                    && effective_policy == binding.tool_policy,
                "角色工作类型或工具权限与冻结绑定不符",
            )?;
            require(
                session.origin == Origin::External,
                "真实讨论 Session 来源必须明确为 external",
            )?;
            if let Some(id) = &session.external_session {
                require(
                    external.insert((&binding.installation.id, id)),
                    "不同 Session 不得共享外部会话",
                )?;
            }
            if let Some(id) = &session.predecessor {
                require(
                    self.sessions
                        .iter()
                        .any(|s| &s.id == id && s.project_id == session.project_id),
                    "后继 Session 不能跨项目继承",
                )?;
            }
        }
        let mut active = HashSet::new();
        let mut inputs = HashSet::new();
        for turn in &self.turns {
            if let Some(view) = &turn.code_view {
                require(
                    view.context.origin == Origin::External
                        && view.context.project == turn.project_id
                        && self.sessions.iter().any(|s| {
                            s.id == turn.session_id && s.owner_id == view.context.source.owner
                        }),
                    "代码输入视图跨项目或归属不符",
                )?;
            }
            if let Some(id) = &turn.input_snapshot {
                require(
                    self.snapshots.iter().any(|snap| {
                        &snap.id == id
                            && self
                                .material_project(
                                    &material_owner(
                                        snap.project_id.as_deref(),
                                        snap.feature_id.as_deref(),
                                        snap.task_id.as_deref(),
                                    )
                                    .unwrap(),
                                )
                                .is_ok_and(|p| p == turn.project_id)
                    }),
                    "Turn 冻结快照不存在或跨项目",
                )?;
            }
            let session = self
                .sessions
                .iter()
                .find(|s| s.id == turn.session_id)
                .ok_or_else(|| err("blocked", "轮次 Session 不存在"))?;
            require(
                session.project_id.as_ref() == Some(&turn.project_id),
                "轮次跨项目引用 Session",
            )?;
            require(
                !turn.client_message_id.is_empty()
                    && !turn.input.trim().is_empty()
                    && turn.input.len()
                        <= if turn.run_id.is_some() {
                            512 * 1024
                        } else {
                            65536
                        }
                    && turn.input_digest.len() == 64,
                "讨论输入或摘要无效",
            )?;
            require(
                inputs.insert((&turn.session_id, &turn.client_message_id)),
                "同一客户端输入不能重复登记",
            )?;
            if turn.status.active() {
                require(
                    active.insert(&turn.session_id),
                    "同一 Session 同时只能有一个活动轮次",
                )?;
            }
            if turn.status == TurnStatus::Completed {
                require(
                    turn.consumed && turn.finished.is_some() && session.external_session.is_some(),
                    "轮次完成需要真实消费、外部会话和终态边界",
                )?;
            }
            if let Some(id) = &turn.run_id {
                require(
                    self.runs
                        .iter()
                        .any(|r| &r.id == id && r.session_id == turn.session_id),
                    "讨论轮次不能引用其他 Session 的 Run",
                )?;
            }
            if let Some(old) = before.turns.iter().find(|t| t.id == turn.id) {
                require(
                    !old.cancel_requested || turn.cancel_requested,
                    "取消请求不可撤销",
                )?;
                require(!old.consumed || turn.consumed, "已确认的消费事实不可撤销")?;
                require(
                    old.external_message
                        .as_ref()
                        .is_none_or(|id| Some(id) == turn.external_message.as_ref())
                        && old
                            .external_turn
                            .as_ref()
                            .is_none_or(|id| Some(id) == turn.external_turn.as_ref()),
                    "原生输入与 Turn 身份不可替换",
                )?;
                require(
                    old.project_id == turn.project_id
                        && old.session_id == turn.session_id
                        && old.input == turn.input
                        && old.client_message_id == turn.client_message_id
                        && old.input_digest == turn.input_digest
                        && old.input_snapshot == turn.input_snapshot
                        && serde_json::to_value(&old.code_view).unwrap()
                            == serde_json::to_value(&turn.code_view).unwrap()
                        && old.source_revision == turn.source_revision,
                    "已派发输入不可修改",
                )?;
                require(
                    old.status.active()
                        || (old.status == turn.status
                            && old.external_message == turn.external_message
                            && old.external_turn == turn.external_turn
                            && old.finished == turn.finished),
                    "已结束轮次不可重新派发",
                )?;
            }
        }
        let old_messages: std::collections::HashMap<_, _> = before
            .conversation_events
            .iter()
            .map(|e| (e.id.as_str(), e))
            .collect();
        let turns: std::collections::HashMap<_, _> = self
            .turns
            .iter()
            .map(|t| (t.id.as_str(), t.session_id.as_str()))
            .collect();
        let mut message_ids = HashSet::new();
        let mut sequences = HashSet::new();
        let mut dedup = HashSet::new();
        for message in &self.conversation_events {
            require(
                turns
                    .get(message.turn_id.as_str())
                    .is_some_and(|session| *session == message.session_id)
                    && message_ids.insert(message.id.as_str()),
                "消息不属于当前轮次",
            )?;
            require(
                sequences.insert((&message.session_id, message.sequence))
                    && dedup.insert((&message.session_id, &message.deduplication_key)),
                "会话事件序号或去重键冲突",
            )?;
            if let Some(old) = old_messages.get(message.id.as_str()) {
                require(*old == message, "会话事件只允许追加")?;
            }
        }
        require(
            old_messages.keys().all(|id| message_ids.contains(id)),
            "会话事件不能删除",
        )?;
        for interaction in &self.interactions {
            if let Some(old) = before.interactions.iter().find(|i| i.id == interaction.id) {
                require(
                    serde_json::to_value((
                        &old.project_id,
                        &old.session_id,
                        &old.turn_id,
                        &old.external_request,
                        old.kind,
                        &old.questions,
                        &old.tool,
                        &old.reason,
                    ))
                    .unwrap()
                        == serde_json::to_value((
                            &interaction.project_id,
                            &interaction.session_id,
                            &interaction.turn_id,
                            &interaction.external_request,
                            interaction.kind,
                            &interaction.questions,
                            &interaction.tool,
                            &interaction.reason,
                        ))
                        .unwrap(),
                    "交互请求身份不可变更",
                )?;
                require(
                    old.status == InteractionStatus::Pending
                        || (old.status == interaction.status
                            && serde_json::to_value(&old.answer).unwrap()
                                == serde_json::to_value(&interaction.answer).unwrap()),
                    "交互回答不可覆盖",
                )?;
            }
            let turn = self
                .turns
                .iter()
                .find(|t| {
                    t.id == interaction.turn_id
                        && t.session_id == interaction.session_id
                        && t.project_id == interaction.project_id
                })
                .ok_or_else(|| err("blocked", "交互请求跨项目或轮次"))?;
            require(
                interaction.status != InteractionStatus::Pending || turn.status.active(),
                "终态轮次不能保留待回答交互",
            )?;
            require(
                interaction.status != InteractionStatus::Answered || interaction.answer.is_some(),
                "已回答交互缺少回答",
            )?;
        }
        for proposal in &self.proposals {
            if let Some(old) = before.proposals.iter().find(|p| p.id == proposal.id) {
                require(
                    serde_json::to_value((
                        &old.project_id,
                        &old.session_id,
                        &old.turn_id,
                        old.schema_version,
                        &old.content,
                        &old.target,
                        old.expected_revision,
                        &old.dependencies,
                        &old.artifact_id,
                        &old.supersedes,
                    ))
                    .unwrap()
                        == serde_json::to_value((
                            &proposal.project_id,
                            &proposal.session_id,
                            &proposal.turn_id,
                            proposal.schema_version,
                            &proposal.content,
                            &proposal.target,
                            proposal.expected_revision,
                            &proposal.dependencies,
                            &proposal.artifact_id,
                            &proposal.supersedes,
                        ))
                        .unwrap(),
                    "提案内容只允许创建新修订",
                )?;
                require(
                    old.status == ProposalStatus::Draft || old.status == proposal.status,
                    "已处理提案不能重新采纳",
                )?;
                require(
                    old.adopted_objects.is_empty()
                        || old.adopted_objects == proposal.adopted_objects,
                    "采纳对象不可替换",
                )?;
                require(
                    old.task_instances
                        .iter()
                        .all(|(key, id)| proposal.task_instances.get(key) == Some(id)),
                    "TaskPlan 实例映射不可覆盖",
                )?;
            }
            if !proposal.task_instances.is_empty() {
                let ProposalContent::TaskPlan { tasks, .. } = &proposal.content else {
                    return Err(err("blocked", "非 TaskPlan 不能关联 Task 实例"));
                };
                require(
                    proposal.status == ProposalStatus::Accepted
                        && proposal.task_instances.len() == tasks.len()
                        && tasks.iter().all(|entry| {
                            proposal.task_instances.get(&entry.key).is_some_and(|id| {
                                self.tasks.iter().any(|t| {
                                    &t.id == id && Some(&t.feature_id) == proposal.target.as_ref()
                                })
                            })
                        }),
                    "TaskPlan 实例不完整或跨 Feature",
                )?;
            }
            require(
                self.turns.iter().any(|t| {
                    t.id == proposal.turn_id
                        && t.session_id == proposal.session_id
                        && t.project_id == proposal.project_id
                        && t.status == TurnStatus::Completed
                }),
                "提案必须来自同项目的已完成讨论轮次",
            )?;
            require(proposal.schema_version == 1, "不支持的提案 schema 版本")?;
            require(
                (proposal.status == ProposalStatus::Accepted)
                    == !proposal.adopted_objects.is_empty(),
                "提案采纳状态与正式对象不一致",
            )?;
            if let Some(old) = &proposal.supersedes {
                require(
                    self.proposals
                        .iter()
                        .any(|p| &p.id == old && p.project_id == proposal.project_id),
                    "提案修订不能跨项目继承",
                )?;
            }
        }
        for attempt in &self.attempts {
            if let Some(old) = before.attempts.iter().find(|a| a.id == attempt.id) {
                require(
                    old.source_base == attempt.source_base
                        && old.live_sessions == attempt.live_sessions,
                    "Attempt 原始代码与角色 Session 映射不可重写",
                )?;
                if attempt.source_base.is_some() {
                    require(
                        old.baseline
                            .as_ref()
                            .is_none_or(|v| Some(v) == attempt.baseline.as_ref()),
                        "真实 Red baseline 不可重写",
                    )?;
                }
            }
            for sid in attempt.live_sessions.values() {
                require(
                    self.sessions.iter().any(|v| {
                        &v.id == sid
                            && v.owner_id
                                == attempt.task_id.as_deref().unwrap_or(&attempt.feature_id)
                            && v.project_id.as_ref()
                                == Some(&self.feature(&attempt.feature_id).unwrap().project_id)
                            && v.runtime_binding.is_some()
                    }),
                    "Attempt 角色 Session 绑定跨范围或缺失",
                )?;
            }
        }
        for execution in &self.executions {
            require(
                self.sessions.iter().any(|s| s.id == execution.session_id),
                "运行实例缺少 Session",
            )?;
            if let Some(id) = &execution.run_id {
                require(
                    self.runs
                        .iter()
                        .any(|r| &r.id == id && r.session_id == execution.session_id),
                    "运行实例引用错误工程 Run",
                )?;
            }
            require(
                execution.status != Status::Released
                    || (execution.writer_revoked && execution.finished.is_some()),
                "释放实例前必须证明写权限已撤销",
            )?;
            if let Some(old) = before.executions.iter().find(|e| e.id == execution.id) {
                require(
                    old.permissions == execution.permissions
                        && old.kind == execution.kind
                        && old.session_id == execution.session_id
                        && old.operation_id == execution.operation_id
                        && old.image_digest == execution.image_digest
                        && old.run_id == execution.run_id,
                    "运行实例归属不可覆盖",
                )?;
            }
        }
        Ok(())
    }
}

impl EngineeringState {
    pub fn material_project(&self, owner: &MaterialOwner) -> Result<&str> {
        match owner {
            MaterialOwner::Project { id } => self
                .projects
                .iter()
                .find(|p| &p.id == id)
                .map(|p| p.id.as_str())
                .ok_or_else(|| err("not_found", "材料 Project 不存在")),
            MaterialOwner::Feature { id } => Ok(&self.feature(id)?.project_id),
            MaterialOwner::Task { feature, id } => {
                require(
                    self.task(id)?.feature_id == *feature,
                    "材料 Task 不属于指定 Feature",
                )?;
                Ok(&self.feature(feature)?.project_id)
            }
        }
    }
    pub fn validate_material_owners(&self) -> Result<()> {
        for document in &self.documents {
            if let Some(recipe) = &document.test_recipe {
                require(document.kind == "test", "测试配方只适用于 Test 定义")?;
                recipe.validate()?;
            }
            let owner = material_owner(
                document.project_id.as_deref(),
                document.feature_id.as_deref(),
                document.task_id.as_deref(),
            )?;
            let project = self.material_project(&owner)?;
            if matches!(owner, MaterialOwner::Project { .. }) {
                require(
                    document.kind == "requirement",
                    "Project 正式文档必须是 Requirement 修订",
                )?;
            }
            for (reference, kind) in [
                (&document.requirement_id, "requirement"),
                (&document.design_id, "design"),
            ] {
                if let Some(id) = reference {
                    let input = self
                        .documents
                        .iter()
                        .find(|d| &d.id == id && d.kind == kind)
                        .ok_or_else(|| err("blocked", "文档输入修订不存在"))?;
                    let input_owner = material_owner(
                        input.project_id.as_deref(),
                        input.feature_id.as_deref(),
                        input.task_id.as_deref(),
                    )?;
                    require(
                        self.material_project(&input_owner)? == project,
                        "文档输入跨项目",
                    )?;
                    if let Some(feature) = &document.feature_id {
                        require(
                            self.document_in_feature(input, feature),
                            "文档输入不属于当前 Feature 的冻结范围",
                        )?;
                    }
                }
            }
            if let Some(id) = &document.artifact_id {
                let artifact = self
                    .artifacts
                    .iter()
                    .find(|a| &a.id == id)
                    .ok_or_else(|| err("blocked", "文档正文 Artifact 不存在"))?;
                require(
                    material_owner(
                        artifact.project_id.as_deref(),
                        artifact.feature_id.as_deref(),
                        artifact.task_id.as_deref(),
                    )? == owner,
                    "文档与正文 Artifact 归属不一致",
                )?;
            }
        }
        for snapshot in &self.snapshots {
            let owner = material_owner(
                snapshot.project_id.as_deref(),
                snapshot.feature_id.as_deref(),
                snapshot.task_id.as_deref(),
            )?;
            let project = self.material_project(&owner)?;
            for id in &snapshot.documents {
                let doc = self
                    .documents
                    .iter()
                    .find(|d| &d.id == id)
                    .ok_or_else(|| err("blocked", "Snapshot 文档修订不存在"))?;
                let doc_owner = material_owner(
                    doc.project_id.as_deref(),
                    doc.feature_id.as_deref(),
                    doc.task_id.as_deref(),
                )?;
                require(
                    self.material_project(&doc_owner)? == project,
                    "Snapshot 文档跨项目",
                )?;
                if let Some(feature) = &snapshot.feature_id {
                    require(
                        self.document_in_feature(doc, feature),
                        "Snapshot 文档不属于当前 Feature 范围",
                    )?;
                }
            }
        }
        for artifact in &self.artifacts {
            self.material_project(&material_owner(
                artifact.project_id.as_deref(),
                artifact.feature_id.as_deref(),
                artifact.task_id.as_deref(),
            )?)?;
        }
        for feature in &self.features {
            if let Some(id) = &feature.source_requirement {
                require(
                    self.documents.iter().any(|d| {
                        &d.id == id
                            && d.project_id.as_ref() == Some(&feature.project_id)
                            && d.kind == "requirement"
                    }),
                    "Feature 需求修订归属无效",
                )?;
            }
        }
        Ok(())
    }
}
