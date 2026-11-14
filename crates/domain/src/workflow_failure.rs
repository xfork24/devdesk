//! Failure facts describe a trusted boundary observation, never a model's retry suggestion.
use crate::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionFailureKind {
    Technical,
    Business,
    Cancelled,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionFailureEffect {
    NotApplied,
    Applied,
    Unknown,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionFailureDisposition {
    pub kind: ExecutionFailureKind,
    pub effect: ExecutionFailureEffect,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowFailure {
    pub code: String,
    pub message: String,
    pub source_ref: Id,
    pub kind: ExecutionFailureKind,
    pub effect: ExecutionFailureEffect,
}
impl WorkflowFailure {
    pub fn from_error(error: &Error, source_ref: Id, fallback: ExecutionFailureKind) -> Self {
        let disposition = error
            .execution_failure
            .unwrap_or(ExecutionFailureDisposition {
                kind: fallback,
                effect: ExecutionFailureEffect::Unknown,
            });
        Self {
            code: error.code.clone(),
            message: error.message.clone(),
            source_ref,
            kind: disposition.kind,
            effect: disposition.effect,
        }
    }
    pub fn automatic_retry_safe(&self) -> bool {
        self.kind == ExecutionFailureKind::Technical
            && self.effect == ExecutionFailureEffect::NotApplied
    }
}
