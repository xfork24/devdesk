//! Stable identities and compile-time build inputs for the shipped native defaults.
use crate::*;

pub const DEFAULT_CODEX_VERSION: &str = "0.167.0";
pub const DEFAULTS_BUNDLE_ID: &str = "builtin-defaults-sol-high-codex-v1";
pub const DEFAULT_MODEL_ID: &str = "builtin-model-gpt-6-1-sol-high-v1";
pub const DEFAULT_HARNESS_ID: &str = "builtin-harness-codex-managed-v1";
pub const DEFAULT_SANDBOX_ID: &str = "builtin-sandbox-codex-docker-v1";
pub const DEFAULT_STANDARD_ID: &str = "builtin-standard-engineering-v1";
pub const DEFAULT_FEATURE_WORKFLOW_ID: &str = "builtin-workflow-feature-sol-high-v1";
pub const DEFAULT_TASK_WORKFLOW_ID: &str = "builtin-workflow-task-sol-high-v1";
pub const DEFAULT_NATIVE_TOOL_ID: &str = "builtin-tool-codex-native-v1";
pub const DEFAULT_EXPERIENCE_TOOL_ID: &str = "builtin-tool-agent-experience-v1";

pub fn default_agent_id(role: &str) -> String {
    format!("builtin-agent-sol-high-{role}-v1")
}
pub fn default_agent_instance_id(role: &str) -> String {
    format!("builtin-instance-sol-high-{role}-v1")
}
pub fn default_codex_recipe_digest() -> String {
    configuration_hash(&[(
        "Dockerfile",
        include_str!("../../../containers/codex/Dockerfile"),
    )])
}

pub fn default_role_id(role: &str) -> String {
    format!("builtin-role-{role}-v1")
}
