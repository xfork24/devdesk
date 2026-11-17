//! Bounded conversation query dispatch. Business writes have native commands.
use crate::*;
impl Engine {
    pub(crate) fn conversation_query(&self, method: &str, p: &Value) -> Option<Result<Value>> {
        matches!(method, "query.sessions" | "query.session").then(|| {
            let mut value = self.store.conversation_query(method, p)?;
            if method == "query.session"
                && let Some(root) = value["controller"]["data"]["root_instance_id"].as_str()
            {
                value["approval_context"] = json!(self.controller_approval_context(root)?);
            }
            Ok(value)
        })
    }
}
