use devdesk_core::ports::Sandbox;
use devdesk_domain::{Origin, Resource, Status, id};

/// Simulated implementation of the `Sandbox` execution contract.
pub struct SimSandbox;
impl Sandbox for SimSandbox {
    // This identity is persisted in existing effect/advance records; keep it stable.
    fn identity(&self) -> String {
        "devdesk_adapters::SimSandbox".into()
    }
    fn resource(&self, run_id: &str) -> Resource {
        Resource {
            id: id(),
            run_id: run_id.into(),
            kind: "simulated-environment".into(),
            status: Status::Active,
            origin: Origin::Simulated,
        }
    }
    fn cleanup(&self, r: &mut Resource, fail: bool) {
        r.status = if fail {
            Status::CleanupFailed
        } else {
            Status::Released
        };
    }
}
