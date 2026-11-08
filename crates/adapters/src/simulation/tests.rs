use devdesk_core::ports::TestRequest;
use devdesk_core::ports::Tests;
use devdesk_domain::Result;
use serde_json::{Value, json};

/// Simulated implementation of the `Tests` execution contract.
pub struct SimTests;
impl Tests for SimTests {
    // This identity is persisted in existing effect/advance records; keep it stable.
    fn identity(&self) -> String {
        "devdesk_adapters::SimTests".into()
    }
    fn reconcile(&self, request: &TestRequest) -> Result<Option<Value>> {
        let mut value = self.validate(request, "success");
        value["status"] = json!("failed");
        value["failure_type"] = json!("interrupted");
        Ok(Some(value))
    }
    fn validate(&self, request: &TestRequest, outcome: &str) -> Value {
        let node = &request.phase;
        let candidate = &request.candidate;
        json!({"operation_id":request.operation_id,"status":if outcome=="assertion_failure"{"failed"}else{"succeeded"},"failure_type":if outcome=="assertion_failure"{json!("assertion")}else{Value::Null},"candidate":candidate,"test_phase":node,"environment":request.environment,"origin":request.origin,"test_definitions":request.test_definitions,"assertion_failed":node=="red","expected_failure":node=="red"})
    }
}
