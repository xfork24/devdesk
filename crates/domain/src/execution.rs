//! Versioned runtime definitions and concrete execution instances.
use crate::*;
impl EngineeringState {
    /// A cancellation is not complete until both the logical Run and its writer have stopped.
    pub fn run_cancellation_pending(&self, worktree: &str) -> bool {
        self.runs.iter().any(|run| {
            run.worktree == worktree && run.cancel_requested
                && (run.status == Status::Running
                    || self.turns.iter().any(|t| t.run_id.as_ref() == Some(&run.id) && t.status.active())
                    || self.executions.iter().any(|e| e.run_id.as_ref() == Some(&run.id) && !e.writer_revoked))
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub repository: String,
    pub path: String,
    pub mainline: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Harness {
    #[serde(default)]
    pub permission: Option<PermissionBinding>,
    pub id: Id,
    pub provider: String,
    pub version: String,
    pub capabilities: Vec<String>,
    pub origin: Origin,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sandbox {
    #[serde(default)]
    pub permission: Option<PermissionBinding>,
    pub id: Id,
    pub provider: String,
    pub version: String,
    pub platform: String,
    pub image: Option<String>,
    pub network: bool,
    pub origin: Origin,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionInstance {
    pub id: Id,
    pub run_id: Id,
    pub harness: Harness,
    pub sandbox: Sandbox,
    pub external_session: Option<String>,
    pub external_environment: Option<String>,
    pub status: Status,
    pub started: Option<u64>,
    pub finished: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reviewer {
    Human { actor: String },
    Agent { agent: Id, run: Id, session: Id },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewAssignment {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spec_refs: Vec<Id>,
    pub delivery: Id,
    pub spec: Id,
    pub reviewer: Reviewer,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeRequestRef {
    pub provider: String,
    pub repository: String,
    pub external_id: String,
    pub source_revision: String,
    pub target_revision: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestOutcome {
    Succeeded,
    Failed,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub operation_id: Id,
    pub status: TestOutcome,
    pub candidate: String,
    pub test_phase: String,
    pub test_definitions: Vec<Id>,
    pub environment: String,
    pub origin: Origin,
    pub assertion_failed: bool,
    pub expected_failure: bool,
    pub failure_type: Option<String>,
}

/// Persistent adapter intent and receipt. The ID is the adapter's idempotency key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    Harness,
    Cancel,
    Test,
    Cleanup,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectStatus {
    Pending,
    Observed,
    Applied,
    Discarded,
    NeedsReconciliation,
    Failed,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectRecord {
    pub id: Id,
    pub run_id: Id,
    pub kind: EffectKind,
    pub status: EffectStatus,
    pub request: Value,
    pub receipt: Option<Value>,
    pub revision: u64,
    pub created: u64,
    pub updated: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvanceStatus {
    NotApplied,
    Prepared,
    Dispatched,
    Observed,
    Applied,
    NeedsReconciliation,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeAdvance {
    pub id: Id,
    pub context: Option<GitContext>,
    pub provider: String,
    pub target: Id,
    pub expected_target: String,
    pub candidate: String,
    pub delivery: Id,
    pub status: AdvanceStatus,
    pub receipt: Option<String>,
    pub revision: u64,
    pub reason: String,
}

/// Supplier proof distinguishes an unknown outcome from a confirmed absence of side effects.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AdvanceObservation {
    Unknown,
    Applied { revision: String },
    NotApplied { actual_target: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeLocation {
    pub owner: Id,
    pub branch: String,
    pub worktree: String,
}
/// Immutable routing information: an adapter must not infer a global repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitContext {
    pub project: Id,
    pub workspace: Workspace,
    pub source: CodeLocation,
    pub target: CodeLocation,
    pub origin: Origin,
}
