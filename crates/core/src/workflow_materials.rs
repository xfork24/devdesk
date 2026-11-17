//! Bounded material metadata and exact, explicitly selected bodies.
use crate::*;
impl Engine {
    pub(crate) fn workflow_materials(&self, p: &Value) -> Result<Value> {
        self.store
            .workflow_reading(crate::read_models::WorkflowReadingView::Materials, p)
    }
}
