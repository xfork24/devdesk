//! Explicit fixture runtime bindings preserve the original coordinator request.
use devdesk_core::{Command, Engine};
use devdesk_domain::Result;
use serde_json::Value;
pub fn execute(
    engine: &Engine,
    method: &str,
    mut command: Command,
    _original_fingerprint: &str,
) -> Result<Value> {
    if method == "session.open" {
        command
            .payload
            .as_object_mut()
            .ok_or_else(|| devdesk_domain::err("fixture", "正式会话原命令缺少参数"))?
            .remove("binding");
        engine.open_prepared_session(
            &crate::runtime::ResourceFixture {
                store: engine.store.as_ref(),
            },
            command,
        )
    } else {
        engine.execute(method, command)
    }
}
