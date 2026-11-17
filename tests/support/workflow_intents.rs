//! Explicit fixture runtime bindings preserve the original coordinator request.
use devdesk_core::{Command, Engine};
use devdesk_domain::Result;
use serde_json::Value;
pub fn execute(
    engine: &Engine,
    method: &str,
    command: Command,
    original_fingerprint: &str,
) -> Result<Value> {
    if method == "session.open" {
        engine.execute_resolved(method, command, original_fingerprint)
    } else {
        engine.execute(method, command)
    }
}
