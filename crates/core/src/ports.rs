//! Native execution adapter contracts.
//!
//! These runtime interfaces are separate from `devdesk_domain::Harness` and
//! `devdesk_domain::Sandbox`, which describe persisted configuration and instances.
use devdesk_domain::{
    AdvanceObservation, Artifact, Document, EngineeringState, ExecutionInstance, GitContext, Id,
    MaterialOwner, Origin, Result, err,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Debug)]
pub struct CredentialCandidate {
    pub reference: String,
    pub status: Value,
}
/// A stable candidate identity is immutable. Implementations inspect an existing
/// candidate before storing, verify its exact content, and reconcile unknown I/O.
/// They may never overwrite another candidate or publish the authoritative binding.
pub trait CredentialCandidates: Send + Sync {
    fn ensure(
        &self,
        provider: &devdesk_domain::ProviderFacts,
        candidate_id: &str,
        secret: &str,
    ) -> Result<CredentialCandidate>;
}

/// Base contract for an agent runtime: configuration, polling, cancellation acknowledgement and recovery.
/// Implementations return provider facts; the engine decides whether to accept them.
pub trait Harness: Send + Sync {
    fn answer_tool_call(
        &self,
        _session: &devdesk_domain::Session,
        _request: &Value,
        _receipt: &devdesk_domain::ToolReceipt,
    ) -> Result<()> {
        Err(devdesk_domain::err(
            "unsupported",
            "Harness 不支持外部工具回执",
        ))
    }
    /// Stable provider + adapter version; production adapters must override this identity.
    fn identity(&self) -> String {
        std::any::type_name::<Self>().into()
    }

    fn open_session(&self, _request: &SessionStart) -> Result<SessionReceipt> {
        Err(devdesk_domain::err(
            "unsupported",
            "供应商不提供持久讨论会话",
        ))
    }
    /// The execution and quota reservation are durable before this call. None
    /// means preparation is pending; no conversation input has been submitted.
    fn begin_session(&self, request: &SessionStart) -> Result<Option<SessionReceipt>> {
        self.open_session(request).map(Some)
    }
    fn poll_session_start(
        &self,
        _session: &devdesk_domain::Session,
        _execution: &devdesk_domain::RuntimeExecution,
    ) -> Result<Option<SessionReceipt>> {
        Err(err("unsupported", "Harness 没有可核对的启动操作"))
    }
    fn dispatch_turn(&self, _request: &TurnDispatch) -> Result<()> {
        Err(devdesk_domain::err(
            "unsupported",
            "供应商不提供讨论输入派发",
        ))
    }
    fn poll_session(
        &self,
        _session: &devdesk_domain::Session,
    ) -> Result<Vec<ConversationObservation>> {
        Err(devdesk_domain::err("unsupported", "供应商不提供讨论事件"))
    }
    fn answer_interaction(
        &self,
        _session: &devdesk_domain::Session,
        _interaction: &devdesk_domain::PendingInteraction,
    ) -> Result<()> {
        Err(devdesk_domain::err("unsupported", "供应商不提供双向交互"))
    }
    fn stop_session(
        &self,
        _session: &devdesk_domain::Session,
        _execution: &devdesk_domain::RuntimeExecution,
    ) -> Result<WorkerStop> {
        Err(devdesk_domain::err(
            "unsupported",
            "供应商不提供 worker 停止证明",
        ))
    }
    fn session_log(&self, _execution: &devdesk_domain::RuntimeExecution) -> Result<Option<String>> {
        Ok(None)
    }
    fn cleanup_worker(
        &self,
        _session: &devdesk_domain::Session,
        _execution: &devdesk_domain::RuntimeExecution,
    ) -> Result<()> {
        Err(devdesk_domain::err(
            "unsupported",
            "供应商不提供 worker 清理",
        ))
    }
}

pub type CodeView = devdesk_domain::FrozenCode;
/// Read historical sources without activating their project or execution.
pub trait AgentContextReader: Send + Sync {
    fn project_knowledge(
        &self,
        _instance: &str,
        _origin_project: Option<&str>,
        _target_project: Option<&str>,
        _method: &str,
        _params: &Value,
    ) -> Result<Value> {
        Err(err("context_required", "读取项目资料需要明确的项目上下文"))
    }
    fn experience(
        &self,
        instance: &str,
        origin_project: Option<&str>,
        params: &Value,
    ) -> Result<Value>;
    fn workflow_outputs(
        &self,
        instance: &str,
        origin_project: Option<&str>,
        params: &Value,
    ) -> Result<Value>;
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStart {
    #[serde(default)]
    pub specifications: Vec<devdesk_domain::ConfigurationRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured_task: Option<devdesk_domain::ConfiguredAgentTask>,
    #[serde(default)]
    pub code_view: Option<CodeView>,
    pub session: devdesk_domain::Session,
    pub agent: devdesk_domain::Definition,
    pub skills: Vec<devdesk_domain::Definition>,
    pub execution: devdesk_domain::RuntimeExecution,
    pub project_root: String,
    pub snapshot: Option<devdesk_domain::Snapshot>,
    pub documents: Vec<Document>,
    #[serde(default)]
    pub reconciliation_turn: Option<devdesk_domain::ConversationTurn>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionReceipt {
    pub external_session: String,
    pub container_id: String,
    pub image_digest: String,
    pub operation_id: Id,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnDispatch {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_references: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured_task: Option<devdesk_domain::ConfiguredAgentTask>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub specifications: Vec<devdesk_domain::Definition>,
    pub session: devdesk_domain::Session,
    pub turn: devdesk_domain::ConversationTurn,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTerminal {
    Completed,
    Failed,
    Cancelled,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConversationObservation {
    ToolCall {
        turn: Id,
        request: Value,
        thread: String,
        external_turn: String,
        call: String,
        tool: String,
        arguments: Value,
        worktree: String,
    },
    Admitted {
        turn: Id,
        message: Option<String>,
        external_turn: Option<String>,
    },
    Consumed {
        turn: Id,
        message: Option<String>,
    },
    Message {
        turn: Id,
        key: String,
        content: String,
        kind: devdesk_domain::ConversationEventKind,
        mode: devdesk_domain::MessageMode,
        item: String,
    },
    Interaction {
        turn: Id,
        request: String,
        kind: devdesk_domain::InteractionKind,
        questions: Vec<devdesk_domain::InteractionQuestion>,
        tool: Option<String>,
        reason: String,
    },
    Terminal {
        turn: Id,
        outcome: ProviderTerminal,
        reason: String,
        quiescent: bool,
    },
}

/// Validations perform no I/O. Calls are asynchronously dispatched after persistence.
pub trait ToolExecutor: Send + Sync {
    fn validate_schema(&self, schema: &Value) -> Result<()>;
    fn validate_arguments(&self, schema: &Value, value: &Value) -> Result<()>;
    fn start(&self, invocation: &devdesk_domain::ToolInvocation) -> Result<()>;
    fn poll(
        &self,
        invocation: &devdesk_domain::ToolInvocation,
    ) -> Result<Option<devdesk_domain::ToolReceipt>>;
    fn cancel(&self, invocation: &devdesk_domain::ToolInvocation) -> Result<()>;
    fn discover(
        &self,
        source: &devdesk_domain::IntegrationDefinition,
        binding: &devdesk_domain::IntegrationBinding,
        image: &str,
        permissions: &devdesk_domain::ExecutionPermissions,
    ) -> Result<Value>;
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerStop {
    pub operation_id: Id,
    pub writer_revoked: bool,
    pub mode: String,
    pub requires_successor: bool,
}
/// Base contract for repository revisions, candidate construction and idempotent target advancement.
pub trait Git: Send + Sync {
    /// Stable provider + adapter version; production adapters must override this identity.
    fn identity(&self) -> String {
        std::any::type_name::<Self>().into()
    }
    fn identity_for(&self, _context: Option<&devdesk_domain::GitContext>) -> String {
        self.identity()
    }
    /// Read/freeze the code revision for an execution stage.
    fn workspace_revision(&self, _context: &GitContext) -> Result<String> {
        Err(devdesk_domain::err(
            "unsupported",
            "Git 适配器缺少工作空间版本查询",
        ))
    }
    fn repository_operation(
        &self,
        _operation: &devdesk_domain::RepositoryOperation,
    ) -> Result<devdesk_domain::RepositoryReceipt> {
        Err(devdesk_domain::err(
            "unsupported",
            "Git 适配器未提供仓库/worktree 管理",
        ))
    }
    fn reconcile_advance(
        &self,
        _context: &GitContext,
        _operation_id: &str,
    ) -> Result<AdvanceObservation> {
        Ok(AdvanceObservation::Unknown)
    }
    fn candidate(
        &self,
        context: &GitContext,
        source: &str,
        target: &str,
        operation: &str,
    ) -> Result<String>;
    /// Read the exact commit graph in the coordinator, without exposing repository metadata to a model.
    fn commit_parents(&self, _context: &GitContext, _revision: &str) -> Result<Vec<String>> {
        Err(devdesk_domain::err(
            "unsupported",
            "Git 适配器未提供提交父节点查询",
        ))
    }
    fn diff(&self, _context: &GitContext, _base: &str, _head: &str) -> Result<String> {
        Err(devdesk_domain::err(
            "unsupported",
            "Git 适配器未提供固定版本 diff",
        ))
    }
    fn preflight_advance(&self, _context: &GitContext, _expected: &str) -> Result<()> {
        Err(devdesk_domain::err(
            "unsupported",
            "Git 适配器未提供远端/策略预检",
        ))
    }
    fn advance(
        &self,
        context: &GitContext,
        actual: &str,
        expected: &str,
        candidate: &str,
        operation_id: &str,
    ) -> Result<String>;
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRequest {
    #[serde(default)]
    pub expect_assertion_failure: bool,
    #[serde(default)]
    pub recipe: Option<devdesk_domain::TestRecipe>,
    pub operation_id: Id,
    #[serde(default)]
    pub session_id: Option<Id>,
    pub candidate: String,
    pub environment: String,
    pub origin: Origin,
    #[serde(default)]
    pub code: Option<GitContext>,
    #[serde(default)]
    pub runtime: Option<ExecutionInstance>,
}
/// Base contract for executing and reconciling tests against a frozen request.
pub trait Tests: Send + Sync {
    /// Stable provider + adapter version; production adapters must override this identity.
    fn identity(&self) -> String {
        std::any::type_name::<Self>().into()
    }
    /// Accept only identities whose executor and immutable assets still match.
    fn accepts_identity(&self, identity: &str) -> bool {
        identity == self.identity()
    }
    fn poll_test(&self, _request: &TestRequest) -> Result<Option<Value>> {
        Err(devdesk_domain::err(
            "unsupported",
            "Tests 未提供真实异步执行",
        ))
    }
    fn stop_test(&self, _request: &TestRequest) -> Result<bool> {
        Err(devdesk_domain::err(
            "unsupported",
            "Tests 未提供真实停止证明",
        ))
    }
    fn cleanup_test(&self, _request: &TestRequest) -> Result<()> {
        Err(devdesk_domain::err("unsupported", "Tests 未提供真实清理"))
    }
    fn reconcile(&self, _request: &TestRequest) -> Result<Option<Value>> {
        Ok(None)
    }
}
/// A replaceable set of execution providers shared by engines in one workspace.
/// Cloning preserves provider instances, including their connection and recovery state.
#[derive(Clone)]
pub struct ExecutionAdapters {
    pub harness: Arc<dyn Harness>,
    pub git: Arc<dyn Git>,
    pub tests: Arc<dyn Tests>,
    pub pull_requests: Option<Arc<dyn PullRequests>>,
}

/// PR API facts are separate from Git refs and model output. No platform merge/bypass operation.
pub trait PullRequests: Send + Sync {
    fn identity(&self) -> String;
    fn ensure(
        &self,
        request: &devdesk_domain::PullRequestRequest,
    ) -> Result<devdesk_domain::PullRequestReceipt>;
    fn read(
        &self,
        request: &devdesk_domain::PullRequestRequest,
    ) -> Result<devdesk_domain::PullRequestReceipt>;
}

/// Workflow-specific scheduling hints; they never authorize dispatch or acceptance.
#[derive(Clone, Copy)]
pub enum WorkflowRuntimeWork {
    Conversations,
    QueuedConversations,
    Controllers,
    ControllerCalls,
    PlatformTools,
    AgentReplies,
    WorkflowIntents,
    Changes,
    Segments,
    RustDrivers,
}

pub trait EngineeringStore: Send + Sync {
    fn namespace(&self) -> crate::concurrency::StoreNamespace {
        crate::concurrency::StoreNamespace::Engineering {
            project_id: self.project_id(),
        }
    }
    /// A bounded, committed resource batch; implementations must not restore history.
    fn read_resources(
        &self,
        _keys: &[crate::concurrency::ResourceKey],
    ) -> Result<Vec<crate::concurrency::ResourceSnapshot>> {
        Err(err("unsupported", "存储未实现资源读取"))
    }
    /// Replay a finite commit without loading business state or invoking its action.
    fn replay_commit(
        &self,
        _command_id: &str,
        _fingerprint: &str,
    ) -> Result<Option<crate::concurrency::CommitReceipt>> {
        Err(err("unsupported", "存储未实现有限提交回执查询"))
    }
    fn orchestration_summary(
        &self,
        _query: &crate::read_models::ControllerQuery,
    ) -> Result<crate::read_models::OrchestrationSummary> {
        Err(err("unsupported", "存储未实现经理摘要读取"))
    }
    fn workflow_driver_summary(
        &self,
        _id: &str,
    ) -> Result<crate::read_models::WorkflowDriverSummary> {
        Err(err("unsupported", "存储未实现驱动摘要读取"))
    }
    fn workflow_decision(&self, _id: &str) -> Result<crate::read_models::DecisionDetail> {
        Err(err("unsupported", "存储未实现确切决定读取"))
    }
    fn workflow_management_summary(
        &self,
        _id: &str,
        _child_offset: u64,
    ) -> Result<crate::read_models::WorkflowManagementSummary> {
        Err(err("unsupported", "存储未实现流程管理摘要"))
    }
    fn workflow_progress(
        &self,
        _id: &str,
        _stage_offset: u64,
    ) -> Result<crate::read_models::WorkflowProgress> {
        Err(err("unsupported", "存储未实现工作流阶段进度"))
    }
    fn experience_headers(
        &self,
        _after: Option<&str>,
    ) -> Result<crate::read_models::ExperienceHeaderPage> {
        Err(err("unsupported", "存储未实现经验目录版本分页"))
    }
    fn experience_directory_source(
        &self,
        _id: &str,
    ) -> Result<Option<crate::read_models::ExperienceDirectorySource>> {
        Err(err("unsupported", "存储未实现有界经验目录来源"))
    }
    fn repository_registration_facts(
        &self,
    ) -> Result<crate::repository_updates::RepositoryRegistrationFacts> {
        Err(err("unsupported", "存储未实现仓库登记上下文"))
    }
    fn repository_refresh_facts(
        &self,
    ) -> Result<crate::repository_updates::RepositoryRefreshFacts> {
        Err(err("unsupported", "存储未实现主干同步上下文"))
    }
    fn repository_connect_projection_facts(
        &self,
        _operation: &str,
    ) -> Result<crate::repository_projection::RepositoryConnectProjectionFacts> {
        Err(err("unsupported", "存储未实现仓库接纳上下文"))
    }
    fn project_metadata(&self) -> Result<devdesk_domain::Project> {
        Err(err("unsupported", "存储未实现项目元数据读取"))
    }
    fn execution_workspace(
        &self,
        _query: &crate::read_models::ExecutionWorkspaceQuery,
    ) -> Result<crate::read_models::ExecutionWorkspace> {
        Err(err("unsupported", "存储未实现执行工作台摘要"))
    }
    /// Core-prepared changes and every business read dependency commit atomically.
    fn commit_changes(
        &self,
        changes: &crate::concurrency::PreparedChangeSet,
    ) -> Result<crate::concurrency::CommitReceipt> {
        self.submit_changes(changes).map(|outcome| outcome.receipt)
    }
    /// Distinguish this call's new commit from a durable replay. Only the new
    /// claim may authorize an external attempt; replay requires observation.
    fn submit_changes(
        &self,
        _changes: &crate::concurrency::PreparedChangeSet,
    ) -> Result<crate::concurrency::CommitOutcome> {
        Err(err("unsupported", "存储未实现资源版本提交"))
    }
    fn observe_query(&self, _stage: &'static str) -> Box<dyn QueryObservation> {
        Box::new(())
    }
    /// Correlate native scheduling/processing stages without exposing tool arguments.
    fn observe_native_call(
        &self,
        _call_id: &str,
        stage: &'static str,
    ) -> Box<dyn QueryObservation> {
        self.observe_query(stage)
    }
    /// Lookup through the same idempotency contract without creating a missing operation.
    fn replay(&self, id: &str, fingerprint: &str) -> Result<Option<Value>> {
        let mut missing = false;
        let result = self.transact(Some((id, fingerprint)), &mut |_| {
            missing = true;
            Err(err("operation_absent", "操作尚未登记"))
        });
        if missing { Ok(None) } else { result.map(Some) }
    }
    /// Conservative scheduling hint; providers without an indexed query remain always eligible.
    fn active_conversation_count(&self) -> Result<usize> {
        let state = self.snapshot()?;
        Ok(state
            .turns
            .iter()
            .filter(|turn| {
                turn.status != devdesk_domain::TurnStatus::Queued
                    && turn.status.active()
                    && state.executions.iter().any(|execution| {
                        execution.session_id == turn.session_id && !execution.writer_revoked
                    })
            })
            .count())
    }
    fn scheduling_hints(&self) -> Result<crate::read_models::SchedulingHints> {
        Ok(crate::read_models::SchedulingHints {
            pending: self.has_pending_work()?,
            live: self
                .snapshot()?
                .projects
                .iter()
                .all(|project| project.execution_mode == "live"),
            running_conversations: self.active_conversation_count()?,
        })
    }
    fn has_pending_work(&self) -> Result<bool> {
        Ok(true)
    }
    fn workflow_runtime_pending(&self, _kind: WorkflowRuntimeWork) -> Result<bool> {
        Ok(true)
    }
    /// Advisory lookup for one durable native call; authorization is checked by its executor.
    fn native_platform_tool_prepared(&self, _id: &str) -> Result<bool> {
        Ok(true)
    }
    fn conversation_running_count(&self) -> Result<usize> {
        let state = self.snapshot()?;
        Ok(state
            .turns
            .iter()
            .filter(|t| {
                t.status != devdesk_domain::TurnStatus::Queued
                    && t.status.active()
                    && state
                        .executions
                        .iter()
                        .any(|e| e.session_id == t.session_id && !e.writer_revoked)
            })
            .count())
    }
    fn snapshot(&self) -> Result<EngineeringState>;
    fn event_cursor(&self) -> Result<u64> {
        Ok(self.snapshot()?.events.last().map_or(0, |event| event.id))
    }
    fn event_hints_after(
        &self,
        cursor: u64,
        limit: usize,
    ) -> Result<Vec<crate::read_models::TailEvent>> {
        Ok(self
            .events_after(cursor, limit)?
            .into_iter()
            .map(|event| crate::read_models::TailEvent {
                event,
                omitted: false,
            })
            .collect())
    }
    fn events_after(&self, cursor: u64, limit: usize) -> Result<Vec<devdesk_domain::Event>> {
        Ok(self
            .snapshot()?
            .events
            .into_iter()
            .filter(|event| event.id > cursor)
            .take(limit.clamp(1, 100))
            .collect())
    }
    /// Weak listeners are invoked after commit and after releasing the store's locks.
    fn subscribe_changes(&self, _listener: std::sync::Weak<dyn Fn() + Send + Sync>) {}
    fn has_pending_tool_calls(&self) -> Result<bool> {
        Ok(true)
    }
    fn tool_call_page(&self, _after: Option<&str>) -> Result<crate::tool_calls::ToolCallPage> {
        Err(err("unsupported", "存储未实现工具调用分页"))
    }
    fn tool_call_facts(&self, _id: &str) -> Result<crate::tool_calls::ToolCallFacts> {
        Err(err("unsupported", "存储未实现工具调用有限上下文"))
    }
    /// One runtime record against exact immutable sources, without runtime history.
    fn encode_runtime_record(
        &self,
        _record: &devdesk_domain::ConfigurationRecord,
    ) -> Result<Value> {
        Err(err("unsupported", "存储未实现运行记录源编码"))
    }
    fn restore_runtime_record(&self, _value: &Value) -> Result<Value> {
        Err(err("unsupported", "存储未实现运行记录源恢复"))
    }
    fn workflow_intent_page(&self, _after: Option<&str>) -> Result<crate::WorkflowIntentPage> {
        Err(err("unsupported", "存储未实现待执行意图分页"))
    }
    fn workflow_intent_facts(
        &self,
        _id: &str,
        _runnable_only: bool,
    ) -> Result<Option<crate::WorkflowIntentFacts>> {
        Err(err("unsupported", "存储未实现工作流意图有限事实"))
    }
    fn workflow_intent_active(&self, _id: &str, _revision: Option<u64>) -> Result<bool> {
        Err(err("unsupported", "存储未实现工作流意图资格读取"))
    }
    /// Native step and ancestor witnesses; no partial business state.
    fn workflow_tool_facts(
        &self,
        _authority: &crate::tool_calls::WorkflowToolAuthority,
        _project: &str,
    ) -> Result<crate::tool_calls::WorkflowToolFacts> {
        Err(err("unsupported", "存储未实现工具步骤资格上下文"))
    }
    /// Scalar invalidation clock; concrete stores must not restore catalog bodies here.
    fn catalog_revision(&self) -> Result<u64> {
        Ok(self.configuration_snapshot()?.revision)
    }
    /// Adapters override this with a configuration-only, immutable cached read model.
    fn configuration_snapshot(&self) -> Result<Arc<crate::ConfigurationCatalogSnapshot>> {
        let state = self.snapshot()?;
        let ready = state
            .repository_worktrees
            .iter()
            .any(|w| w.kind == devdesk_domain::WorktreeKind::Mainline);
        Ok(Arc::new(crate::ConfigurationCatalogSnapshot::new(
            crate::catalog_projection(&state),
            0,
            ready,
        )?))
    }
    fn transact(
        &self,
        key: Option<(&str, &str)>,
        action: &mut dyn FnMut(&mut EngineeringState) -> Result<Value>,
    ) -> Result<Value>;
    fn root(&self) -> String;
    fn project_id(&self) -> Option<String> {
        None
    }
}
/// Synchronous query scope supplied by an outer diagnostics adapter.
pub trait QueryObservation {}
impl QueryObservation for () {}
pub trait ArtifactStore: Send + Sync {
    fn seal_scoped(&self, owner: &MaterialOwner, category: &str, body: &str) -> Result<Artifact> {
        match owner {
            MaterialOwner::Feature { id } => self.seal(id, None, category, body),
            MaterialOwner::Task { feature, id } => self.seal(feature, Some(id), category, body),
            MaterialOwner::Global | MaterialOwner::Project { .. } => {
                Err(err("unsupported", "ArtifactStore 未提供 Project 产物封存"))
            }
        }
    }
    fn seal(
        &self,
        feature: &str,
        task: Option<&str>,
        category: &str,
        body: &str,
    ) -> Result<Artifact>;
    fn read(&self, artifact: &Artifact) -> Result<Vec<u8>>;
}

/// Environment operations used while core assembles and verifies a Session binding.
pub trait SessionResources {
    fn shared_store(&self) -> &dyn EngineeringStore;
    fn architecture(&self) -> String;
    fn prepare_harness(
        &self,
        record: &devdesk_domain::ConfigurationRecord,
        resource: &devdesk_domain::ConfigurationRecord,
    ) -> Result<devdesk_domain::RuntimeInstallation>;
    fn installations(&self) -> Result<Vec<devdesk_domain::RuntimeInstallation>>;
    fn ensure_runtime(
        &self,
        provider: devdesk_domain::HarnessProvider,
        preferred: Option<&str>,
    ) -> Result<devdesk_domain::RuntimeInstallation>;
    fn runtime_verified(&self, id: &str) -> Result<bool>;
    fn installation_recipe(&self, installation: &devdesk_domain::RuntimeInstallation) -> String;
    fn validate_installation(
        &self,
        installation: &devdesk_domain::RuntimeInstallation,
    ) -> Result<()>;
    fn verify_permissions(
        &self,
        state: &EngineeringState,
        lock: &devdesk_domain::VersionedPermissionLock,
        installation: &devdesk_domain::RuntimeInstallation,
    ) -> Result<()>;
    fn compile_permissions(
        &self,
        state: &EngineeringState,
        lock: &devdesk_domain::VersionedPermissionLock,
        scope: Option<devdesk_domain::ExecutionScope>,
    ) -> Result<devdesk_domain::ExecutionPermissions>;
    fn adapter_identity(&self, provider: devdesk_domain::HarnessProvider) -> String;
    fn credential_reference(
        &self,
        provider: devdesk_domain::HarnessProvider,
        connection: Option<&devdesk_domain::ModelConnection>,
        state: &EngineeringState,
    ) -> Result<Option<String>>;
    fn model_profile_digest(
        &self,
        connection: &devdesk_domain::ModelConnection,
        model: &str,
    ) -> Result<String>;
    fn runtime_digest(
        &self,
        agent: &devdesk_domain::Definition,
        binding: &devdesk_domain::RuntimeBinding,
    ) -> Result<String>;
}

/// Application-level ID and time sources; domain only consumes their values.
pub trait ApplicationEnvironment: Send + Sync {
    fn allocate_id(&self) -> String;
    fn unix_millis(&self) -> u64;
}

/// Read-only project inventory and pure backend feasibility; adapters provide environment facts.
pub trait ProjectInventory: Send + Sync {
    fn snapshots(&self) -> Result<Vec<EngineeringState>>;
    fn runtime_facts(&self) -> Result<Value> {
        Ok(serde_json::json!({"available":false}))
    }
}
pub trait ConfigurationBackend: Send + Sync {
    fn prepare_permission_binding(&self, state: &EngineeringState, change: &Value)
    -> Result<Value>;
}
