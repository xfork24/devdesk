//! Data policies for the installed workflow scheduling primitive.
use crate::*;
use std::collections::BTreeSet;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowScheduling {
    #[serde(default)]
    pub agent: AgentSchedulingBudget,
    pub max_concurrency: usize,
    pub max_actions_per_tick: usize,
    pub lease_seconds: u64,
    #[serde(default)]
    pub max_retries: u64,
    #[serde(default)]
    pub retry_error_codes: BTreeSet<String>,
}
impl WorkflowScheduling {
    pub fn validate(&self) -> Result<()> {
        require(
            (1..=32).contains(&self.max_concurrency)
                && (1..=64).contains(&self.max_actions_per_tick)
                && (5..=3600).contains(&self.lease_seconds)
                && self.max_retries <= 8
                && self.retry_error_codes.len() <= 32
                && (1..=3600).contains(&self.agent.timeout_seconds),
            "Workflow 调度配置超出原语资源边界",
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrchestrationSelection {
    pub mode: String,
}
impl OrchestrationSelection {
    pub fn validate(&self) -> Result<()> {
        require(
            ["manual", "rust", "agent"].contains(&self.mode.as_str()),
            "未知调度模式",
        )?;
        Ok(())
    }
}

impl Default for WorkflowScheduling {
    fn default() -> Self {
        Self {
            max_concurrency: 4,
            max_actions_per_tick: 16,
            lease_seconds: 60,
            max_retries: 0,
            retry_error_codes: BTreeSet::new(),
            agent: AgentSchedulingBudget::default(),
        }
    }
}
