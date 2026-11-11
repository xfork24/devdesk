//! Stable factory identities used only by tests. Product selection reads catalog metadata.
#![allow(dead_code)]
use devdesk_domain::{ExecutionPermissionSelection, PermissionSelection};
pub const DEFAULT_MODEL_ID: &str = "builtin-model-gpt-6-1-sol-high:1";

pub const DEFAULT_HARNESS_ID: &str = "builtin-harness-codex-managed:1";

pub const DEFAULT_SANDBOX_ID: &str = "builtin-docker-sandbox:1";

pub const DEFAULT_STANDARD_ID: &str = "builtin-standard-engineering:3";

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

pub const DEFAULT_CAPABILITY_ID: &str = "builtin-capability-sol-high-codex:3";

pub fn default_configured_agent_id(role: &str) -> String {
    format!(
        "builtin-agent-sol-high-{role}:{}",
        if role == "project-manager" { 5 } else { 4 }
    )
}

pub const DEFAULT_PERMISSION_SANDBOX: &str = "builtin-docker-sandbox:1";
pub fn default_permission_selection() -> ExecutionPermissionSelection {
    ExecutionPermissionSelection {
        sandbox_ref: DEFAULT_PERMISSION_SANDBOX.into(),
        harness: PermissionSelection::FollowTarget,
        sandbox: PermissionSelection::FollowTarget,
    }
}
