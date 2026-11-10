//! Technical payload bounds shared by the canonical Workflow task renderer.
use crate::*;
pub const TASK_TEXT_LIMIT: usize = 32 * 1024;
pub const TASK_WORKFLOW_LIMIT: usize = 1024 * 1024;
pub const TASK_PAYLOAD_LIMIT: usize = 512 * 1024;
pub fn task_content_hash<T: Serialize>(value: &T) -> String {
    configuration_hash(value)
}
