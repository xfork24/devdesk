//! Versioned runtime definitions and concrete execution instances.
use crate::*;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub repository: String,
    pub path: String,
    pub mainline: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Harness {
    #[serde(default)]
    pub permission: Option<ResolvedPermission>,
    pub id: Id,
    pub provider: String,
    pub version: String,
    pub capabilities: Vec<String>,
    pub origin: Origin,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sandbox {
    #[serde(default)]
    pub permission: Option<ResolvedPermission>,
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
    pub operation_id: Id,
    pub harness: Harness,
    pub sandbox: Sandbox,
    pub external_session: Option<String>,
    pub external_environment: Option<String>,
    pub status: Status,
    pub started: Option<u64>,
    pub finished: Option<u64>,
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
    pub environment: String,
    pub origin: Origin,
    pub assertion_failed: bool,
    pub expected_failure: bool,
    pub failure_type: Option<String>,
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
