use devdesk_core::ports::TestRequest;
use devdesk_core::ports::Tests;
use devdesk_domain::Result;
use serde_json::{Value, json};

/// Simulated implementation of the `Tests` execution contract.
#[derive(Default)]
pub struct SimTests {
    scenarios: std::sync::Mutex<std::collections::HashMap<String, String>>,
}
impl devdesk_core::ports::SimulationControls for SimTests {
    fn configure_test(&self, operation: &str, scenario: &str) -> Result<()> {
        let mut map = self
            .scenarios
            .lock()
            .map_err(|_| devdesk_domain::err("fixture", "Simulation lock poisoned"))?;
        devdesk_domain::require(
            map.len() < 1024 || map.contains_key(operation),
            "模拟测试夹具超过并发限制",
        )?;
        map.insert(operation.into(), scenario.into());
        Ok(())
    }
}
impl Tests for SimTests {
    // This identity is persisted in existing effect/advance records; keep it stable.
    fn identity(&self) -> String {
        "devdesk_adapters::SimTests".into()
    }
    fn reconcile(&self, request: &TestRequest) -> Result<Option<Value>> {
        let mut value = self.validate(request);
        value["status"] = json!("failed");
        value["failure_type"] = json!("interrupted");
        Ok(Some(value))
    }
    fn validate(&self, request: &TestRequest) -> Value {
        if request.origin != devdesk_domain::Origin::Simulated {
            return json!({"status":"failed","failure_type":"adapter_contract","message":"SimTests cannot produce live evidence"});
        }
        let outcome = self
            .scenarios
            .lock()
            .unwrap()
            .remove(&request.operation_id)
            .unwrap_or_else(|| "success".into());
        let node = &request.phase;
        let candidate = &request.candidate;
        json!({"operation_id":request.operation_id,"status":if outcome=="assertion_failure"{"failed"}else{"succeeded"},"failure_type":if outcome=="assertion_failure"{json!("assertion")}else{Value::Null},"candidate":candidate,"test_phase":node,"environment":request.environment,"origin":request.origin,"test_definitions":request.test_definitions,"assertion_failed":node=="red","expected_failure":node=="red"})
    }
}
