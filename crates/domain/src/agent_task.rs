//! Technical payload bounds shared by the canonical Workflow task renderer.
use crate::*;
pub const TASK_TEXT_LIMIT: usize = 32 * 1024;
pub const TASK_WORKFLOW_LIMIT: usize = 1024 * 1024;
pub const TASK_PAYLOAD_LIMIT: usize = 512 * 1024;
/// Large-output reading mechanism named inside task contracts: the platform tool the
/// agent must page through, and the first-chunk size it receives inline.
pub const OUTPUT_READ_QUALIFIED_NAME: &str = "workflow.outputs.read";
pub const OUTPUT_READ_CALL_NAME: &str = "devdesk_workflow_outputs_read";
pub const OUTPUT_READ_PAGE_BYTES: usize = 32768;
/// Renderer identity recorded on frozen task contracts. Metadata only: acceptance of a
/// task contract is structural, never a version comparison.
pub const AGENT_TASK_RENDERER: &str = "agent-task/v6";
pub fn task_content_hash<T: Serialize>(value: &T) -> String {
    configuration_hash(value)
}
