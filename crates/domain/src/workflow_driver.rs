//! Data policies for the installed workflow scheduling primitive.
use crate::*;
use std::collections::BTreeSet;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrchestrationPolicy {
    pub max_concurrency: usize,
    pub max_actions_per_tick: usize,
    pub lease_seconds: u64,
    #[serde(default)]
    pub max_retries: u64,
    #[serde(default)]
    pub retry_error_codes: BTreeSet<String>,
}
impl OrchestrationPolicy {
    pub fn validate(&self) -> Result<()> {
        require(
            (1..=32).contains(&self.max_concurrency)
                && (1..=64).contains(&self.max_actions_per_tick)
                && (5..=3600).contains(&self.lease_seconds)
                && self.max_retries <= 8
                && self.retry_error_codes.len() <= 32,
            "调度策略超出原语资源边界",
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrchestrationSelection {
    pub mode: String,
    #[serde(default)]
    pub policy_ref: Option<Id>,
}
impl OrchestrationSelection {
    pub fn validate(&self) -> Result<()> {
        require(
            ["manual", "rust", "agent"].contains(&self.mode.as_str()),
            "未知调度模式",
        )?;
        if self.mode == "rust" {
            require(self.policy_ref.is_some(), "Rust 调度必须选择精确策略")?;
        }
        if let Some(reference) = &self.policy_ref {
            parse_source_reference(reference)?;
        }
        Ok(())
    }
}
