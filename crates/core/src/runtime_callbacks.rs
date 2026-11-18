//! Bounded recovery of already granted operations. A persisted observation can
//! be admitted again; absence of an observation never grants external I/O.
use crate::{concurrency::*, *};

impl Engine {
    pub fn scheduling_hints(&self) -> Result<crate::read_models::SchedulingHints> {
        self.store
            .scheduling_hints_at(&crate::environment::execution_clock())
    }
    pub(crate) fn pending_runtime_records(
        &self,
        kind: &str,
        status: &str,
    ) -> Result<Vec<ResourceSnapshot>> {
        let cursor_key = format!("{kind}:{status}");
        let after = self
            .runtime_callback_cursors
            .lock()
            .map_err(|e| err("storage", e.to_string()))?
            .get(&cursor_key)
            .cloned();
        let clock = crate::environment::execution_clock();
        let mut page = self
            .store
            .runtime_record_page_at(kind, status, after.as_deref(), &clock)?;
        if page.ids.is_empty() && after.is_some() {
            page = self
                .store
                .runtime_record_page_at(kind, status, None, &clock)?;
        }
        if matches!(kind, "sealed_command_intent" | "configuration_transfer") {
            page.ids.truncate(4);
        }
        let keys = page
            .ids
            .iter()
            .map(|id| ResourceKey::new("governance", id))
            .collect::<Vec<_>>();
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == keys.len() && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "恢复分页事实读取不完整",
        )?;
        let mut cursors = self
            .runtime_callback_cursors
            .lock()
            .map_err(|e| err("storage", e.to_string()))?;
        // A bounded caller may process only the first row before yielding.
        // Advance one candidate, not the entire page: otherwise every short
        // pass wraps to the same first row and permanently skips its siblings.
        if let Some(id) = page.ids.first() {
            cursors.insert(cursor_key, id.clone());
        } else {
            cursors.remove(&cursor_key);
        }
        Ok(rows
            .into_iter()
            .filter(|r| {
                r.value.as_ref().is_some_and(|v| {
                    v["type"] == kind
                        && v["data"][if kind == "workflow_driver" {
                            "mode"
                        } else {
                            "status"
                        }] == status
                })
            })
            .collect())
    }
}
