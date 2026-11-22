//! Native execution adapter contracts.
//!
//! These runtime interfaces are separate from `devdesk_domain::Harness` and
//! `devdesk_domain::Sandbox`, which describe persisted configuration and instances.
use devdesk_domain::{
    AdvanceObservation, Artifact, ConfiguredAgentTask, Document, EngineeringState,
    ExecutionInstance, GitContext, Id, MaterialOwner, Origin, PullRequestReceipt,
    PullRequestRequest, RepositoryOperation, RepositoryReceipt, Result, Session, err,
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
    /// Notification is only a hint to poll durable/actual facts. It grants no
    /// execution authority and may be coalesced or lost across a restart.
    fn subscribe_wakeup(&self, _listener: std::sync::Weak<dyn Fn() + Send + Sync>) {}
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
    /// Read-only inspection of this exact operation. It must never stop,
    /// restart or recreate a worker; unknown results do not grant a new attempt.
    fn inspect_worker_stop(
        &self,
        _session: &devdesk_domain::Session,
        _execution: &devdesk_domain::RuntimeExecution,
    ) -> Result<Option<WorkerStop>> {
        Err(err("unsupported", "供应商未提供只读停止核对"))
    }
    fn inspect_worker_cleanup(
        &self,
        _session: &devdesk_domain::Session,
        _execution: &devdesk_domain::RuntimeExecution,
    ) -> Result<bool> {
        Err(err("unsupported", "供应商未提供只读清理核对"))
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
    /// Read the original durable receipt only; never start, stop or resend.
    fn inspect(
        &self,
        _invocation: &devdesk_domain::ToolInvocation,
    ) -> Result<Option<devdesk_domain::ToolReceipt>> {
        Err(err("unsupported", "工具执行器未实现只读回执核对"))
    }
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
    /// Inspect tracked configuration without running repository commands or fetching.
    fn discover_project_tests(
        &self,
        _project: &str,
        _mainline: &str,
    ) -> Result<devdesk_domain::ProjectTestDiscovery> {
        Ok(devdesk_domain::ProjectTestDiscovery::default())
    }
    fn observe_delivery_target(&self, _context: &GitContext) -> Result<Option<String>> {
        Err(err("unsupported", "Git 未实现远端目标只读观察"))
    }
    fn delivery_sync_ready(
        &self,
        _context: &GitContext,
        _expected: &str,
        _revision: &str,
    ) -> Result<bool> {
        Err(err("unsupported", "Git 未实现同步前现场核对"))
    }
    fn fetch_delivery_target(&self, _context: &GitContext, _expected: &str) -> Result<()> {
        Err(err("unsupported", "Git 未实现隔离交付目标读取"))
    }
    /// Fetch a fixed PR test-merge object without publishing a branch.
    fn fetch_delivery_commit(&self, _context: &GitContext, _revision: &str) -> Result<()> {
        Err(err("unsupported", "Git 未实现确切 PR 候选对象读取"))
    }
    fn commit_tree(&self, _context: &GitContext, _revision: &str) -> Result<String> {
        Err(err("unsupported", "Git 未实现确切提交 tree 核对"))
    }
    fn sync_delivery(
        &self,
        _context: &GitContext,
        _expected: &str,
        _revision: &str,
        _operation: &str,
        _inspect_only: bool,
    ) -> Result<Option<Value>> {
        Err(err("unsupported", "Git 未实现远端合入后的本地同步"))
    }
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
    /// Read-only preflight for a writer: exact revision and no unsealed changes.
    /// The runtime must repeat this check before mounting the writable worktree.
    fn validate_workspace(&self, _context: &GitContext, _revision: &str) -> Result<()> {
        Err(err("unsupported", "Git 适配器缺少写入工作空间校验"))
    }
    /// Read the exact pending work without staging, committing or restoring it.
    /// None means this adapter has no physical workspace checkpoint support.
    fn working_state(&self, _context: &GitContext, _revision: &str) -> Result<Option<Value>> {
        Ok(None)
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
    /// Read only the original physical operation and its immutable marker.
    /// Absence is unknown, never authorization to dispatch it again.
    fn inspect_repository_operation(
        &self,
        _operation: &RepositoryOperation,
    ) -> Result<Option<RepositoryReceipt>> {
        Err(err("unsupported", "Git 缺少只读仓库操作核对"))
    }
    /// Observe the exact source publication, or create its absent ref under a
    /// fresh human recovery grant. An existing ref may never be moved.
    fn recover_source_publication(
        &self,
        _operation: &RepositoryOperation,
        _allow_create: bool,
    ) -> Result<RepositoryReceipt> {
        Err(err("unsupported", "Git 缺少源分支发布恢复"))
    }
    /// Resume explicitly requested onboarding only with proof that the
    /// original operation never reached its first local Git effect.
    fn resume_unstarted_connection(
        &self,
        _operation: &RepositoryOperation,
    ) -> Result<Option<RepositoryReceipt>> {
        Ok(None)
    }
    fn inspect_advance(
        &self,
        _context: &GitContext,
        _operation_id: &str,
    ) -> Result<AdvanceObservation> {
        Err(err("unsupported", "Git 缺少只读推进核对"))
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TestExecutionObservation {
    Unknown,
    NotStarted,
    Running { timed_out: bool },
    Completed { result: Value },
    Stopped,
}
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
    /// Inspect only the original operation. Never create, start or stop a worker.
    fn inspect_test(&self, _request: &TestRequest) -> Result<TestExecutionObservation> {
        Err(err("unsupported", "测试适配器未实现只读执行核对"))
    }
    fn inspect_test_cleanup(&self, _request: &TestRequest) -> Result<bool> {
        Err(err("unsupported", "测试适配器未实现只读清理核对"))
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

/// PR API facts are separate from Git refs and model output; no protection bypass.
pub trait PullRequests: Send + Sync {
    fn delivery_target(&self, _context: &GitContext) -> Result<Value> {
        Err(err("unsupported", "PR 适配器未实现目标分支只读观察"))
    }
    fn ensure_delivery(
        &self,
        _request: &PullRequestRequest,
        _inspect_only: bool,
    ) -> Result<Option<PullRequestReceipt>> {
        Err(err("unsupported", "PR 适配器未实现精简交付发布"))
    }
    fn delivery_candidate(
        &self,
        _request: &PullRequestRequest,
    ) -> Result<Option<devdesk_domain::PullRequestMergeCandidate>> {
        Err(err(
            "unsupported",
            "PR 适配器未实现源分支 PR 的确切试合并观察",
        ))
    }
    fn delivery_status(
        &self,
        _request: &devdesk_domain::PullRequestMergeRequest,
        _inspect_only: bool,
    ) -> Result<Option<Value>> {
        Err(err("unsupported", "PR 适配器未实现确切回归状态登记"))
    }
    fn merge_delivery(
        &self,
        _request: &devdesk_domain::PullRequestMergeRequest,
        _inspect_only: bool,
    ) -> Result<Option<PullRequestReceipt>> {
        Err(err("unsupported", "PR 适配器未实现受保护合入"))
    }
    fn delivery_merge_ready(
        &self,
        _request: &devdesk_domain::PullRequestMergeRequest,
    ) -> Result<bool> {
        Err(err("unsupported", "PR 适配器未实现远端门禁观察"))
    }
    fn identity(&self) -> String;
    fn ensure(
        &self,
        request: &devdesk_domain::PullRequestRequest,
    ) -> Result<devdesk_domain::PullRequestReceipt>;
    fn read(
        &self,
        request: &devdesk_domain::PullRequestRequest,
    ) -> Result<devdesk_domain::PullRequestReceipt>;
    fn inspect(&self, _request: &PullRequestRequest) -> Result<Option<PullRequestReceipt>> {
        Err(err("unsupported", "PR 适配器缺少只读 marker 核对"))
    }
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
    fn workspace_reading(&self, _params: &Value) -> Result<Value> {
        Err(err("unsupported", "存储未实现有界工作台读取"))
    }
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
    fn workflow_decision_context(
        &self,
        _instance: &str,
        _execution: Option<&str>,
    ) -> Result<crate::read_models::DecisionContext> {
        Err(err("unsupported", "存储未实现有界审批展示依据"))
    }
    fn workflow_management_summary(
        &self,
        _id: &str,
        _child_offset: u64,
    ) -> Result<crate::read_models::WorkflowManagementSummary> {
        Err(err("unsupported", "存储未实现流程管理摘要"))
    }
    /// Selected work adoption with optional history from the same read.
    fn work_configuration_view(
        &self,
        owner: &str,
        project: Option<&str>,
        include_history: bool,
    ) -> Result<Value> {
        crate::standards::work_configuration_view(
            &self.snapshot()?,
            owner,
            project,
            include_history,
        )
    }
    /// Standards comparison and usage only need catalog, owners and plan metadata.
    fn standard_work_snapshot(&self, _usage_ref: Option<&str>) -> Result<EngineeringState> {
        self.snapshot()
    }
    fn workflow_progress(
        &self,
        _id: &str,
        _stage_offset: u64,
    ) -> Result<crate::read_models::WorkflowProgress> {
        Err(err("unsupported", "存储未实现工作流阶段进度"))
    }
    fn workflow_detail(
        &self,
        _query: &crate::read_models::WorkflowDetailQuery,
    ) -> Result<Option<crate::read_models::WorkflowDetailFacts>> {
        Err(err("unsupported", "存储未实现有界流程详情"))
    }
    fn execution_task_detail(&self, _turn: &str) -> Result<Option<Value>> {
        Err(err("unsupported", "存储未实现精确冻结任务读取"))
    }
    fn project_records(&self, _method: &str, _params: &Value) -> Result<Value> {
        Err(err("unsupported", "存储未实现有限项目资料读取"))
    }
    /// Operational pages are filtered before decoding exact records, without
    /// restoring an aggregate engineering snapshot.
    fn operational_read(&self, _method: &str, _params: &Value) -> Result<Value> {
        Err(err("unsupported", "存储未实现运行记录分页"))
    }
    fn conversation_query(&self, _method: &str, _params: &Value) -> Result<Value> {
        Err(err("unsupported", "存储未实现有限会话读取"))
    }
    fn workflow_reading(
        &self,
        _view: crate::read_models::WorkflowReadingView,
        _params: &Value,
    ) -> Result<Value> {
        Err(err("unsupported", "存储未实现流程范围内的有界读取"))
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
    fn feature_branches(&self, _feature: &str, _task_offset: u64) -> Result<Value> {
        Err(err("unsupported", "存储未实现 Feature 分支查询"))
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
    /// Prepare the explicit factory reset, materializing execution evidence before
    /// removing its configuration sources. Ordinary publications remain immutable.
    fn prepare_configuration_reset(
        &self,
        _records: &[devdesk_domain::ConfigurationRecord],
        _catalog_revision: u64,
    ) -> Result<crate::configuration_publication::ConfigurationPublication> {
        Err(err("unsupported", "存储未实现完全重置配置"))
    }
    /// Reuse bounded resource reads only during one synchronous advisory query.
    /// A command must acquire its own authoritative facts after this scope ends.
    fn advisory_read_scope(&self) -> Box<dyn QueryObservation> {
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
        self.replay_commit(id, fingerprint)
            .map(|r| r.map(|r| r.result))
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
            stages: crate::SchedulingStage::ALL.to_vec(),
            pending: self.has_pending_work()?,
            live: self
                .snapshot()?
                .projects
                .iter()
                .all(|project| project.execution_mode == "live"),
            running_conversations: self.active_conversation_count()?,
        })
    }
    fn scheduling_hints_at(
        &self,
        _clock: &devdesk_domain::ExecutionClockAnchor,
    ) -> Result<crate::read_models::SchedulingHints> {
        self.scheduling_hints()
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
    fn encode_configuration_record(
        &self,
        _catalog: &crate::ConfigurationCatalogSnapshot,
        _record: &devdesk_domain::ConfigurationRecord,
    ) -> Result<Value> {
        Err(err("unsupported", "存储未实现候选目录记录编码"))
    }
    fn encode_configuration_value(
        &self,
        _catalog: &crate::ConfigurationCatalogSnapshot,
        _value: &Value,
    ) -> Result<Value> {
        Err(err("unsupported", "存储未实现候选源值编码"))
    }
    fn restore_runtime_value(&self, _value: &Value) -> Result<Value> {
        Err(err("unsupported", "存储未实现有限运行事实源恢复"))
    }
    fn encode_runtime_value(&self, _value: &Value) -> Result<Value> {
        Err(err("unsupported", "存储未实现有限运行事实源编码"))
    }
    /// Trusted task admission supplies an already validated exact renderer
    /// result. Persist its compact source descriptor and literal ledger body.
    fn encode_execution_task(&self, _task: &ConfiguredAgentTask) -> Result<(Value, String)> {
        Err(err("unsupported", "存储未实现显式任务源编码"))
    }
    /// An original coordinator command contains a validated renderer result,
    /// before a Turn exists. Encode its source without inventing a Turn/task.
    fn encode_workflow_input_intent(
        &self,
        _record: &devdesk_domain::ConfigurationRecord,
    ) -> Result<Value> {
        Err(err("unsupported", "存储未实现工作流输入意图源编码"))
    }
    fn provider_tool_intent_page(
        &self,
        _after: Option<&str>,
    ) -> Result<crate::tool_calls::ToolCallPage> {
        Err(err("unsupported", "存储未实现原生工具创建意图分页"))
    }
    fn provider_poll_page(&self, _after: Option<&str>) -> Result<crate::tool_calls::ToolCallPage> {
        Err(err("unsupported", "存储未实现原生观察交接分页"))
    }
    fn conversation_capture_page(
        &self,
        _after: Option<&str>,
    ) -> Result<crate::tool_calls::ToolCallPage> {
        Err(err("unsupported", "存储未实现完成会话捕获分页"))
    }
    fn conversation_capture_facts(
        &self,
        _session: &Session,
        _turn: &str,
        _terminal: &str,
        _worker_boundary: u64,
    ) -> Result<crate::ConversationCaptureFacts> {
        Err(err("unsupported", "存储未实现有界会话捕获事实"))
    }
    fn conversation_input_facts(
        &self,
        _session: &Session,
        _through_turn: Option<&str>,
    ) -> Result<crate::ConversationCaptureFacts> {
        Err(err("unsupported", "存储未实现输入前有界上下文事实"))
    }
    fn conversation_page(
        &self,
        _queued: bool,
        _after: Option<&str>,
    ) -> Result<crate::tool_calls::ToolCallPage> {
        Err(err("unsupported", "存储未实现会话轮询分页"))
    }
    fn conversation_runtime_facts(
        &self,
        _turn: &str,
    ) -> Result<Option<crate::ConversationRuntimeFacts>> {
        Err(err("unsupported", "存储未实现有界会话运行事实"))
    }
    fn conversation_answers_pending(
        &self,
        _turn: &str,
    ) -> Result<Vec<devdesk_domain::PendingInteraction>> {
        Err(err("unsupported", "存储未实现待返回交互分页"))
    }
    fn conversation_start_source(&self, _turn: &str) -> Result<crate::ConversationStartSource> {
        Err(err("unsupported", "存储未实现冻结会话派发事实"))
    }
    fn context_call_page(&self, _after: Option<&str>) -> Result<crate::tool_calls::ToolCallPage> {
        Err(err("unsupported", "存储未实现上下文调用分页"))
    }
    fn context_call_facts(&self, _id: &str) -> Result<crate::ContextCallFacts> {
        Err(err("unsupported", "存储未实现上下文调用原生事实"))
    }
    fn platform_calls_pending(&self) -> Result<bool> {
        Err(err("unsupported", "存储未实现平台调用待办读取"))
    }
    /// Finite runtime work index, never a configuration or project snapshot.
    fn runtime_record_page(
        &self,
        _kind: &str,
        _status: &str,
        _after: Option<&str>,
    ) -> Result<crate::tool_calls::ToolCallPage> {
        Err(err("unsupported", "存储未实现运行记录有界分页"))
    }
    fn runtime_record_page_at(
        &self,
        kind: &str,
        status: &str,
        after: Option<&str>,
        _clock: &devdesk_domain::ExecutionClockAnchor,
    ) -> Result<crate::tool_calls::ToolCallPage> {
        self.runtime_record_page(kind, status, after)
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
    /// Offline test fixtures only; production commands use prepared resources.
    #[cfg(feature = "test-fixtures")]
    fn seed_fixture_state(
        &self,
        _key: Option<(&str, &str)>,
        _action: &mut dyn FnMut(&mut EngineeringState) -> Result<Value>,
    ) -> Result<Value> {
        Err(err("read_only", "此存储不支持离线测试夹具写入"))
    }
    fn root(&self) -> String;
    fn project_id(&self) -> Option<String> {
        None
    }
}
/// Synchronous query scope supplied by an outer diagnostics adapter.
pub trait QueryObservation {}
impl QueryObservation for () {}
pub trait ArtifactStore: Send + Sync {
    fn seal_prepared(&self, _seal: &devdesk_domain::ArtifactSeal, _body: &str) -> Result<Artifact> {
        Err(err("unsupported", "产物存储未提供预分配身份封存"))
    }
    /// Read and authenticate this exact preallocated identity. Missing bytes
    /// are distinct from an unreadable file or an observed hash mismatch.
    fn inspect_prepared(&self, _seal: &devdesk_domain::ArtifactSeal) -> Result<Option<Artifact>> {
        Err(err("unsupported", "产物存储未提供原身份封存核对"))
    }
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
    /// An optional process identity and monotonic counter for deterministic budgets.
    fn monotonic_clock(&self) -> Option<(Id, u64)> {
        None
    }
}

/// Read-only project inventory and pure backend feasibility; adapters provide environment facts.
pub trait ProjectInventory: Send + Sync {
    fn configuration_catalogs(&self) -> Result<Vec<Arc<crate::ConfigurationCatalogSnapshot>>>;
    fn runtime_facts(&self) -> Result<Value> {
        Ok(serde_json::json!({"available":false}))
    }
}
pub trait ConfigurationBackend: Send + Sync {
    fn prepare_permission_binding(&self, state: &EngineeringState, change: &Value)
    -> Result<Value>;
}
