use devdesk_core::ports::Git;
use devdesk_domain::{AdvanceObservation, Attempt, GitContext, Origin, Result, require};

/// Simulated implementation of the `Git` execution contract.
pub struct SimGit;
impl Git for SimGit {
    // This identity is persisted in existing effect/advance records; keep it stable.
    fn identity(&self) -> String {
        "devdesk_adapters::SimGit".into()
    }
    fn reconcile_advance(
        &self,
        _context: &GitContext,
        operation: &str,
    ) -> Result<AdvanceObservation> {
        Ok(AdvanceObservation::Applied {
            revision: format!("sim-rev-candidate-{operation}"),
        })
    }
    fn execution_revision(&self, attempt: &Attempt, _phase: &str) -> Result<String> {
        Ok(format!("sim-rev-source-{}", attempt.id))
    }
    fn candidate(
        &self,
        context: &GitContext,
        source: &str,
        target: &str,
        operation: &str,
    ) -> Result<String> {
        require(
            context.origin == Origin::Simulated,
            "模拟 Git 不接受真实仓库执行来源",
        )?;
        require(
            source.starts_with("sim-rev-") && target.starts_with("sim-rev-"),
            "模拟 Git 仅接受模拟 revision",
        )?;
        Ok(format!("sim-rev-candidate-{operation}"))
    }
    fn advance(
        &self,
        context: &GitContext,
        actual: &str,
        expected: &str,
        candidate: &str,
        _operation_id: &str,
    ) -> Result<String> {
        require(
            context.origin == Origin::Simulated,
            "模拟 Git 不接受真实仓库执行来源",
        )?;
        require(actual == expected, "目标版本变化，拒绝推进")?;
        require(candidate.starts_with("sim-rev-candidate-"), "候选来源无效")?;
        Ok(candidate.into())
    }
}
