//! Workflow test values use the current contract directly. No old-schema converter.
#![allow(dead_code)]
pub fn current(value: serde_json::Value) -> serde_json::Value {
    value
}
