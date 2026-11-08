//! Execution adapter contracts. Implement these traits for either simulated or real providers.
//!
//! These runtime interfaces are separate from `devdesk_domain::Harness` and
//! `devdesk_domain::Sandbox`, which describe persisted configuration and instances.
use devdesk_domain::{
    AdvanceObservation, Attempt, Document, ExecutionInstance, GitContext, Id, Origin, Resource,
    Result, Run,
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
    fn configure(&self, node: &str, scenario: &str) -> String;
    fn poll(&self, run: &Run, input: &Value, now: u64) -> Result<Option<Value>>;
    fn reconcile(&self, _run: &Run) -> Result<Option<Value>> {
        Ok(None)
    }
}
/// Base contract for repository revisions, candidate construction and idempotent target advancement.
pub trait Git: Send + Sync {
    /// Stable provider + adapter version; production adapters must override this identity.
    fn identity(&self) -> String {
        std::any::type_name::<Self>().into()
    }
    /// Read/freeze the code revision for an execution stage; simulation syntax belongs to the adapter.
    fn execution_revision(&self, attempt: &Attempt, phase: &str) -> Result<String>;
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
    fn validate(&self, request: &TestRequest, outcome: &str) -> Value;
    fn reconcile(&self, _request: &TestRequest) -> Result<Option<Value>> {
        Ok(None)
    }
}
/// Base contract for describing execution resources and releasing their external instances.
pub trait Sandbox: Send + Sync {
    /// Stable provider + adapter version; production adapters must override this identity.
    fn identity(&self) -> String {
        std::any::type_name::<Self>().into()
    }
    /// Pure resource intent; implementations must not create external resources here.
    fn resource(&self, run_id: &str) -> Resource;
    fn cleanup(&self, resource: &mut Resource, fail: bool);
}
/// A replaceable set of execution providers shared by engines in one workspace.
/// Cloning preserves provider instances, including their connection and recovery state.
#[derive(Clone)]
pub struct ExecutionAdapters {
    pub harness: Arc<dyn Harness>,
    pub git: Arc<dyn Git>,
    pub tests: Arc<dyn Tests>,
    pub sandbox: Arc<dyn Sandbox>,
}
