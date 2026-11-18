//! In-process technical attempt registry; persistent intents retain authority.
use crate::*;

/// Technical ownership of an already executing attempt. No lock is held during
/// I/O or commits; other callers skip the same attempt instead of waiting.
/// Durable grants remain the sole authority, including after process restart.
#[derive(Default)]
pub(crate) struct InFlight(std::sync::Mutex<std::collections::BTreeSet<Id>>);
pub(crate) struct Attempt<'a> {
    registry: &'a InFlight,
    id: Id,
}
impl InFlight {
    pub(crate) fn enter(&self, id: &str) -> Result<Option<Attempt<'_>>> {
        let mut active = self.0.lock().map_err(|e| err("storage", e.to_string()))?;
        if active.contains(id) {
            return Ok(None);
        }
        require(active.len() < 128, "并发副作用尝试超过技术预算")?;
        active.insert(id.into());
        Ok(Some(Attempt {
            registry: self,
            id: id.into(),
        }))
    }
    pub(crate) fn contains(&self, id: &str) -> Result<bool> {
        Ok(self
            .0
            .lock()
            .map_err(|e| err("storage", e.to_string()))?
            .contains(id))
    }
}
impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.registry.0.lock() {
            active.remove(&self.id);
        }
    }
}
