//! Native execution adapter contracts.
//!
//! These runtime interfaces are separate from `devdesk_domain::Harness` and
//! `devdesk_domain::Sandbox`, which describe persisted configuration and instances.
use devdesk_domain::{
    AdvanceObservation, Attempt, Document, ExecutionInstance, GitContext, Id, Origin, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// Base contract for an agent runtime: configuration, polling, cancellation acknowledgement and recovery.
/// Implementations return provider facts; the engine decides whether to accept them.
pub trait Harness: Send + Sync {
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStart {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_base: Option<devdesk_domain::AgentTaskBase>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<devdesk_domain::FrozenAgentTask>,
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
    fn execution_revision(&self, attempt: &Attempt, phase: &str) -> Result<String>;
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
    pub operation_id: Id,
    #[serde(default)]
    pub session_id: Option<Id>,
    pub phase: String,
    pub candidate: String,
    pub test_definitions: Vec<Id>,
    pub environment: String,
    pub origin: Origin,
    #[serde(default)]
    pub definitions: Vec<Document>,
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
