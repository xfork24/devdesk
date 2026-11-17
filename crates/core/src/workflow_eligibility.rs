//! Read-only eligibility values. Commands repeat native authorization and CAS.
use crate::*;
#[derive(Debug, Clone, Serialize)]
pub struct WorkflowActionEligibility {
    pub allowed: bool,
    pub code: String,
    pub reason: String,
}
impl WorkflowActionEligibility {
    pub(crate) fn ready() -> Self {
        Self {
            allowed: true,
            code: "ready".into(),
            reason: String::new(),
        }
    }
    pub(crate) fn blocked(code: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            code: code.into(),
            reason: reason.into(),
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct WorkflowStepActions {
    pub dispatch: WorkflowActionEligibility,
    pub accept: WorkflowActionEligibility,
    pub retry: WorkflowActionEligibility,
}
