//! Resolve a bounded failure message from the original observed value.
use crate::*;
pub(crate) fn failure_message(value: &Value, fallback: &str) -> String {
    value
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| value.as_str())
        .unwrap_or(fallback)
        .to_owned()
}
