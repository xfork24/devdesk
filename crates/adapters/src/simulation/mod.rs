//! Built-in simulated providers. Real providers implement the same `core::ports` traits.
mod git;
mod harness;
mod sandbox;
mod tests;
use devdesk_core::ports::ExecutionAdapters;
pub use git::SimGit;
pub use harness::SimHarness;
pub use sandbox::SimSandbox;
use std::sync::Arc;
pub use tests::SimTests;

/// The default provider set; callers may replace any or all four trait objects.
pub fn adapters() -> ExecutionAdapters {
    let tests = Arc::new(SimTests::default());
    ExecutionAdapters {
        harness: Arc::new(SimHarness),
        git: Arc::new(SimGit),
        tests: tests.clone(),
        sandbox: Arc::new(SimSandbox),
        pull_requests: None,
        simulation: Some(tests),
    }
}
