//! Stable factory identities used only by tests. Product selection reads catalog metadata.
#![allow(dead_code)]
use devdesk_domain::{ExecutionPermissionSelection, PermissionSelection};
pub const DEFAULT_MODEL_ID: &str = "builtin-model-gpt-6-1-sol-low:1";

pub const DEFAULT_HARNESS_ID: &str = "builtin-harness-codex-managed:1";

pub const DEFAULT_SANDBOX_ID: &str = "builtin-sandbox-docker:2";

pub const DEFAULT_STANDARD_ID: &str = "simplified-standard-engineering:9";

pub const DEFAULT_DSH_NATIVE_TOOL_ID: &str = "builtin-tool-dsh-native:1";

pub const DEFAULT_NATIVE_TOOL_ID: &str = "builtin-tool-codex-native:1";

pub fn default_agent_id(role: &str) -> String {
    default_configured_agent_id(role)
}

pub fn default_agent_instance_id(role: &str) -> String {
    format!("builtin-instance-sol-high-{role}-v1")
}

pub fn default_role_id(role: &str) -> String {
    format!("builtin-role-{role}:1")
}

pub const DEFAULT_CAPABILITY_ID: &str = "builtin-capability-sol-high-codex:4";

pub fn default_configured_agent_id(role: &str) -> String {
    format!(
        "simplified-agent-{role}:{}",
        match role {
            "project-manager" => 10,
            "tester" => 9,
            _ => 8,
        }
    )
}

pub const DEFAULT_PERMISSION_SANDBOX: &str = "builtin-sandbox-docker:2";
pub fn default_permission_selection() -> ExecutionPermissionSelection {
    ExecutionPermissionSelection {
        sandbox_ref: DEFAULT_PERMISSION_SANDBOX.into(),
        harness: PermissionSelection::FollowTarget,
        sandbox: PermissionSelection::FollowTarget,
    }
}

/// Explicit project test input for protocol fixtures; no external execution claim.
pub fn fixture_test_configuration() -> serde_json::Value {
    serde_json::json!({"language":"JavaScript","independent_runner":"node_test","regression":{
        "runner":"node_test","files":["tests/fixture.test.mjs"],"image":format!("sha256:{}","a".repeat(64)),"timeout_seconds":30}})
}

/// Compatibility scenarios opt into the detailed suite explicitly. Tests of
/// actual defaults must continue to use Engine::configuration_defaults directly.
pub fn detailed_selection(mut selection: serde_json::Value) -> serde_json::Value {
    selection["workflow_ref"] = serde_json::json!("builtin-workflow-feature-sol-high:12");
    selection["task_workflow_ref"] = serde_json::json!("builtin-workflow-task-sol-high:11");
    selection["standard_ref"] = serde_json::json!("builtin-standard-engineering:10");
    selection["sandbox_ref"] = serde_json::json!("builtin-sandbox-docker:2");
    selection["role_agent_bindings"] = serde_json::json!({});
    for role in [
        "analyst",
        "architect",
        "project-manager",
        "developer",
        "tester",
        "reviewer",
    ] {
        selection["role_agent_bindings"][format!("builtin-role-{role}:1")] =
            serde_json::json!(format!(
                "builtin-agent-sol-high-{role}:{}",
                if role == "project-manager" { 13 } else { 6 }
            ));
    }
    selection
}
